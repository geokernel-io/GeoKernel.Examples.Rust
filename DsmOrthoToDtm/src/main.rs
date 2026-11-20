//! DSM + orthophoto to DTM: three independent methods.
//!
//! Compare three newly computed terrain estimates on the same full DSM: the native classical filter
//! (DSM only), the experimental pix_grad model (DSM only) and the experimental rgb_grad_fm model (DSM
//! plus an aligned RGB orthophoto). Four linked native GeoKernel 2D maps keep the input context on the
//! left. The reference is evaluation-only; the methods do not feed each other.
// One method index addresses parallel arrays (names, heights, maps, scores) throughout.
#![allow(clippy::needless_range_loop)]
mod ai;
mod dialog;
mod gdal;
mod map2d;
mod ortho;
mod panes;
mod sample;
mod terrain;

use ai::METHODS;
use geokernel::{Runtime, ViewerWindow};
use map2d::{Event, Extent};
use ortho::{Comparison, Scene};
use panes::Panes;
use serde_json::{json, Value};
use std::{
    env,
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};
use terrain::{PRESETS, PRESET_NAMES};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const TITLES: [&str; 4] = [
    "RGB ORTHOPHOTO · input context",
    "CLASSICAL · DSM only",
    "PIX_GRAD · DSM only",
    "RGB_GRAD_FM · DSM + RGB",
];
const PENDING: [&str; 3] = [
    "CLASSICAL · Run to generate",
    "PIX_GRAD · Run to generate",
    "RGB_GRAD_FM · Run to generate",
];
const INPUTS: [&str; 2] = ["Original DSM relief", "RGB orthophoto"];
const MODES: [&str; 3] = [
    "Terrain relief",
    "Removed height: DSM − DTM",
    "Signed reference errors",
];
const SCOPES: [&str; 2] = ["Full input", "Shared comparison crop"];
const SUFFIX: [&str; 3] = ["DTM · relief", "REMOVED HEIGHT", "ERROR · DTM − reference"];
const NAMES: [&str; 3] = [
    "Classical · DSM",
    "pix_grad · DSM",
    "rgb_grad_fm · DSM + RGB",
];
const LEGENDS: [&str; 3] = [
    "Same relief light/contrast for all methods; source orthophoto or DSM remains on the left.",
    "Shared removed-height scale: white 0 m → red ≥20 m. Display saturation only; numeric heights are unchanged.",
    "Shared signed-error scale: blue ≤−2 m · white 0 · red ≥+2 m. Scores use identical valid reference cells in the selected scope.",
];
const CHANGED: &str =
    "Controls changed. Run to update; maps and scores retain the last completed settings.";
const FOOTNOTE: &str = "Different pretrained checkpoints and objectives: differences do not isolate the causal benefit of RGB. Process full input, then evaluate the same crop. Experimental development scene.";
const PARKED: [f64; 2] = [-1.0e9, -1.0e9];
const PAN: i32 = 0;
const ZOOM_BOX: i32 = 1;

// Control panel IDs.
const SAMPLE: i32 = 1;
const OPEN: i32 = 2;
const REFERENCE: i32 = 3;
const MODELS: i32 = 4;
const PIX_ZIP: i32 = 5;
const RGB_ZIP: i32 = 6;
const OVERLAP: i32 = 7;
const THREADS: i32 = 8;
const PRESET: i32 = 9;
const RUN: i32 = 10;
const CANCEL: i32 = 11;
const INPUT: i32 = 12;
const MODE: i32 = 13;
const SCOPE: i32 = 14;
const SAVE: i32 = 15;
const PAN_TOOL: i32 = 16;
const BOX_TOOL: i32 = 17;
const ZOOM_IN: i32 = 18;
const ZOOM_OUT: i32 = 19;
const FIT: i32 = 20;
const PROGRESS: i32 = 21;

fn set_env(name: &str, value: &OsStr) -> sample::Result<()> {
    env::set_var(name, value);
    // GDAL/Qt read the C runtime's environment, which Rust's Windows setter does not update.
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "ucrt")]
        extern "C" {
            fn _wputenv_s(name: *const u16, value: *const u16) -> i32;
        }
        let name: Vec<u16> = OsStr::new(name).encode_wide().chain(Some(0)).collect();
        let value: Vec<u16> = value.encode_wide().chain(Some(0)).collect();
        if unsafe { _wputenv_s(name.as_ptr(), value.as_ptr()) } != 0 {
            return Err("Cannot configure native runtime environment".into());
        }
    }
    Ok(())
}

/// Use GEOKERNEL_BIN when supplied, otherwise the verified published 1.5.40 SDK.
fn load_runtime() -> Result<Runtime> {
    if !cfg!(windows) {
        return Err("This example currently supports Windows x64".into());
    }
    let bin = match env::var_os("GEOKERNEL_BIN") {
        Some(bin) => PathBuf::from(bin),
        None => {
            let example = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let status = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(example.join("AcquireSdk.ps1"))
                .status()?;
            if !status.success() {
                return Err("Published SDK preparation failed".into());
            }
            example.join("../packages/GeoKernel/1.5.40/windows-x64")
        }
    };
    let bin = bin.canonicalize()?;
    let plugins = if bin.join("plugins/platforms/qwindows.dll").is_file() {
        bin.join("plugins")
    } else {
        bin.clone()
    };
    set_env("GEOKERNEL_BIN", bin.as_os_str())?;
    set_env("QT_PLUGIN_PATH", plugins.as_os_str())?;
    set_env(
        "QT_QPA_PLATFORM_PLUGIN_PATH",
        plugins.join("platforms").as_os_str(),
    )?;
    set_env("GDAL_DATA", bin.join("gdal-data").as_os_str())?;
    let mut paths = vec![bin.clone()];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    set_env("PATH", &env::join_paths(paths)?)?;
    Ok(unsafe { Runtime::load(bin)? })
}

/// A finished job: a new scene/comparison or models to publish, and its status message.
struct Outcome {
    scene: Option<Arc<Scene>>,
    comparison: Option<Arc<Comparison>>,
    models: Option<[PathBuf; 2]>,
    message: String,
}

impl Outcome {
    fn message(message: impl Into<String>) -> Self {
        Self {
            scene: None,
            comparison: None,
            models: None,
            message: message.into(),
        }
    }
}

enum Message {
    Status(String, f64),
    Done(sample::Result<Outcome>),
}

type Progress<'a> = ortho::Progress<'a>;

fn load_scene(
    dsm: &Path,
    rgb: &Path,
    reference: Option<&Path>,
    manifest: Value,
    folder: &Path,
    cancel: &AtomicBool,
    progress: Progress,
) -> sample::Result<Outcome> {
    progress(5.0, "Reading source DSM...");
    let dsm = gdal::read_raster(dsm, cancel)?;
    progress(25.0, "Checking RGB channels, grid and coverage...");
    let rgb = ortho::read_ortho(&dsm, rgb, cancel)?;
    let reference = match reference {
        Some(path) => {
            progress(50.0, "Reading evaluation-only reference...");
            let reference = gdal::read_raster(path, cancel)?;
            gdal::validate_reference(&dsm, &reference)?;
            Some(Arc::new(reference))
        }
        None => None,
    };
    if manifest["standard_full_tile"] == json!(true)
        && (!gdal::same_projection(&dsm.projection, "EPSG:2056")?
            || dsm.width != 2000
            || dsm.height != 2000
            || dsm.pixel != 0.5
            || dsm.extent() != [2_677_000.0, 1_251_000.0, 2_678_000.0, 1_252_000.0])
    {
        return Err("Unexpected Zurich sample grid.".into());
    }
    progress(70.0, "Preparing original DSM relief...");
    let original = gdal::write_rgb(
        &folder.join("original.tif"),
        &dsm,
        &terrain::relief(&dsm, &dsm.z, cancel)?,
    )?;
    sample::check(cancel)?;
    let scene = Scene {
        dsm: Arc::new(dsm),
        rgb: Arc::new(rgb),
        reference,
        original,
        manifest,
    };
    Ok(Outcome {
        scene: Some(Arc::new(scene)),
        ..Outcome::message("Aligned inputs ready. Prepare both models and Run.")
    })
}

struct App {
    window: ViewerWindow,
    panes: Panes,
    events: mpsc::Receiver<Event>,
    options: Arc<sample::Options>,
    archive_sha256: Arc<[String; 2]>,
    temporary: PathBuf,
    cancel: Arc<AtomicBool>,
    messages: Option<mpsc::Receiver<Message>>,
    worker: Option<thread::JoinHandle<()>>,
    scene: Option<Arc<Scene>>,
    comparison: Option<Arc<Comparison>>,
    models: Option<[PathBuf; 2]>,
    setup_models: bool,
    overlap: i32,
    threads: i32,
    preset: usize,
    input: usize,
    mode: usize,
    scope: usize,
    tool: i32,
    probe: Option<String>,
    probe_point: Option<[f64; 2]>,
    state: String,
    presses: [Option<(i32, i32)>; 4],
    applied: [Option<Extent>; 4],
    log: String,
    busy: bool,
    generation: u32,
}

fn g(v: f64) -> String {
    let text = format!("{v:.3}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

impl App {
    fn set_busy(&mut self, busy: bool) -> Result<()> {
        self.busy = busy;
        let (has_scene, has_comparison) = (self.scene.is_some(), self.comparison.is_some());
        for id in [
            SAMPLE, OPEN, MODELS, PIX_ZIP, RGB_ZIP, OVERLAP, THREADS, PRESET, INPUT, SCOPE,
        ] {
            self.window.set_control_enabled(id, !busy)?;
        }
        for id in [REFERENCE, PAN_TOOL, BOX_TOOL, ZOOM_IN, ZOOM_OUT, FIT] {
            self.window.set_control_enabled(id, !busy && has_scene)?;
        }
        self.window
            .set_control_enabled(RUN, !busy && has_scene && self.models.is_some())?;
        for id in [SAVE, MODE] {
            self.window
                .set_control_enabled(id, !busy && has_comparison)?;
        }
        self.window.set_control_enabled(CANCEL, busy)?;
        if !busy {
            self.window.set_control_value(PROGRESS, 0.0, "")?;
        }
        Ok(())
    }

    fn status(&mut self, text: &str) -> Result<()> {
        self.window.set_status_text(text).map_err(Into::into)
    }

    /// Source, models, state and the probed pixel in the log panel; legend, scores and summary below the maps.
    fn refresh_text(&mut self) -> Result<()> {
        let name = |p: &Path| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        };
        let source = match &self.scene {
            None => "Preparing sample...".to_owned(),
            Some(s) => format!(
                "{} + {}<br>{} × {} · {} m cells<br>{}",
                name(&s.dsm.path),
                name(&s.rgb.path),
                s.dsm.width,
                s.dsm.height,
                g(s.dsm.pixel),
                if s.reference.is_some() {
                    "Aligned reference attached"
                } else {
                    "No reference"
                }
            ),
        };
        let models = if self.models.is_some() {
            "pix_grad + rgb_grad_fm ready<br>Verified 256 × 256 tiles · CPU"
        } else {
            "Models not prepared."
        };
        let probe = self.probe.clone().unwrap_or_else(|| {
            "Click a source pixel to inspect RGB and all terrain estimates.".to_owned()
        });
        let log = format!(
            "<b>DSM + RGB → DTM · Compare three methods</b><br>Classical · DSM-only AI · DSM+RGB AI. Each starts from the same full DSM.<br><br>\
             {source}<br><br>{models}<br><br><i>{}</i><br><br>{probe}<br><br>\
             Data: Federal Office of Topography swisstopo. AI weights: David Perez · CC BY 4.0. Reference is evaluation-only; no datum conversion.",
            self.state
        );
        if log != self.log {
            self.window.clear_log()?;
            self.window.append_log(&log)?;
            self.log = log;
        }
        let footer = match (&self.scene, &self.comparison) {
            (Some(scene), Some(r)) => {
                let scores = if self.scope == 1 { &r.crop } else { &r.full };
                let lines: Vec<String> = (0..3)
                    .map(|m| {
                        let s = &scores[m];
                        if s.count > 0 {
                            format!("{} · {} valid pixels · RMSE {:.3} m · MAE {:.3} m · bias {:.3} m", NAMES[m], s.count, s.rmse, s.mae, s.bias)
                        } else {
                            format!("{} · {} valid pixels · RMSE — · MAE — · bias —", NAMES[m], s.count)
                        }
                    })
                    .collect();
                let [_, _, w, h] = ortho::window(scene);
                let p = scene.dsm.pixel;
                format!(
                    "{}\n{}\nLast run · pix_grad {} tiles · rgb_grad_fm {} tiles · overlap {} px · CPU threads {} · classical working {} m / window {} m · \
                     evaluation: {} · common crop {w} × {h} cells ({} × {} m). All methods processed the full input; outside-crop map areas are dimmed.\n{FOOTNOTE}",
                    LEGENDS[self.mode],
                    lines.join("\n"),
                    r.ai[0].tiles, r.ai[1].tiles, r.overlap, r.threads, g(r.classical.working), g(r.classical.parameters.window),
                    if self.scope == 1 { "shared crop" } else { "full input" },
                    g(w as f64 * p), g(h as f64 * p)
                )
            }
            (Some(_), None) => format!("Inputs ready first; Run generates all three terrain estimates.\nAligned inputs loaded. Run to create all three terrain estimates.\n{FOOTNOTE}"),
            _ => format!("Inputs ready first; Run generates all three terrain estimates.\nNo prepared prediction is loaded.\n{FOOTNOTE}"),
        };
        self.panes.set_footer(&footer);
        Ok(())
    }

    // ----- jobs ---------------------------------------------------------------------------
    fn begin(
        &mut self,
        work: impl FnOnce(&AtomicBool, Progress) -> sample::Result<Outcome> + Send + 'static,
    ) -> Result<()> {
        if self.busy {
            return Ok(());
        }
        self.cancel = Arc::new(AtomicBool::new(false));
        self.set_busy(true)?;
        let (tx, rx) = mpsc::channel();
        let cancel = self.cancel.clone();
        self.messages = Some(rx);
        self.worker = Some(thread::spawn(move || {
            // Also called from the SDK's inference thread while the worker waits in the C call.
            let progress = |percent: f64, text: &str| {
                let _ = tx.send(Message::Status(text.to_owned(), percent));
            };
            let result = work(&cancel, &progress);
            let _ = tx.send(Message::Done(result));
        }));
        Ok(())
    }

    fn folder(&mut self) -> Result<PathBuf> {
        self.generation += 1;
        let folder = self.temporary.join(self.generation.to_string());
        fs::create_dir_all(&folder)?;
        Ok(folder)
    }

    fn owner(&mut self) -> Result<usize> {
        Ok(self.window.viewer().get_native_handle()?)
    }

    fn prepare_models(&mut self, selected: Option<usize>, local: Option<PathBuf>) -> Result<()> {
        let options = self.options.clone();
        let has_scene = self.scene.is_some();
        self.begin(move |cancel, progress| {
            let mut folders: Vec<PathBuf> = Vec::new();
            for index in 0..2 {
                let archive = if selected == Some(index) {
                    local.as_deref()
                } else {
                    None
                };
                let folder = ai::prepare_model(&options, index, archive, cancel, &|text| {
                    progress(0.0, &text)
                })
                .map_err(|e| {
                    format!("{e}\nUse the pinned model ZIPs or retry the published URLs.")
                })?;
                folders.push(folder);
            }
            let models: [PathBuf; 2] = folders
                .try_into()
                .map_err(|_| "Model preparation incomplete.")?;
            Ok(Outcome {
                models: Some(models),
                ..Outcome::message(if has_scene {
                    "Inputs and both models ready. Press Run."
                } else {
                    "Models ready; load aligned DSM and RGB."
                })
            })
        })
    }

    fn choose_model(&mut self, index: usize) -> Result<()> {
        let title = format!("Choose the pinned {} model ZIP", METHODS[index + 1]);
        if let Some(path) = dialog::open_zip(self.owner()?, &title)? {
            self.prepare_models(Some(index), Some(PathBuf::from(path)))?;
        }
        Ok(())
    }

    fn load_sample(&mut self) -> Result<()> {
        let (folder, options) = (self.folder()?, self.options.clone());
        self.setup_models = self.models.is_none();
        self.begin(move |cancel, progress| {
            let sample = sample::prepare(
                &sample::Assets::zurich_ortho(),
                &options.data_root,
                options.sample_archive.as_deref(),
                options.sample_url.as_deref(),
                cancel,
                &|text| progress(0.0, &text),
                sample::SAMPLE_HINT,
            )?;
            sample::check(cancel)?;
            gdal::configure_runtime(&sample, &set_env)?;
            let manifest = ai::read_json(&sample.join("source-manifest.json"))?;
            load_scene(
                &sample.join("dsm.tif"),
                &sample.join("ortho_rgb.tif"),
                Some(&sample.join("reference.tif")),
                manifest,
                &folder,
                cancel,
                progress,
            )
        })
    }

    fn open_inputs(&mut self) -> Result<()> {
        let owner = self.owner()?;
        let Some(dsm) = dialog::open_tif(owner, "Open metre-height DSM")? else {
            return Ok(());
        };
        let Some(rgb) = dialog::open_tif(owner, "Open exactly aligned RGB Byte orthophoto")? else {
            return Ok(());
        };
        let folder = self.folder()?;
        self.begin(move |cancel, progress| {
            load_scene(
                Path::new(&dsm),
                Path::new(&rgb),
                None,
                json!({}),
                &folder,
                cancel,
                progress,
            )
        })
    }

    fn open_reference(&mut self) -> Result<()> {
        let Some(scene) = self.scene.clone() else {
            return Ok(());
        };
        let Some(path) = dialog::open_tif(self.owner()?, "Open aligned evaluation-only reference")?
        else {
            return Ok(());
        };
        let (folder, completed, sha) = (
            self.folder()?,
            self.comparison.clone(),
            self.archive_sha256.clone(),
        );
        self.begin(move |cancel, progress| {
            progress(5.0, "Reading evaluation-only reference...");
            let reference = gdal::read_raster(Path::new(&path), cancel)?;
            gdal::validate_reference(&scene.dsm, &reference)?;
            let next = Scene {
                dsm: scene.dsm.clone(),
                rgb: scene.rgb.clone(),
                reference: Some(Arc::new(reference)),
                original: scene.original.clone(),
                manifest: scene.manifest.clone(),
            };
            let comparison = match completed {
                Some(result) => Some(Arc::new(ortho::attach_reference(
                    &next, &result, &folder, &sha, cancel, progress,
                )?)),
                None => None,
            };
            sample::check(cancel)?;
            Ok(Outcome {
                scene: Some(Arc::new(next)),
                comparison,
                ..Outcome::message("Reference updated; all generated height arrays are unchanged.")
            })
        })
    }

    fn process(&mut self) -> Result<()> {
        let (Some(scene), Some(models)) = (self.scene.clone(), self.models.clone()) else {
            return Ok(());
        };
        let parameters = ortho::effective_parameters(&scene.dsm, PRESETS[self.preset]);
        let (folder, overlap, threads, sha) = (
            self.folder()?,
            self.overlap,
            self.threads,
            self.archive_sha256.clone(),
        );
        self.begin(move |cancel, progress| {
            let result = ortho::compare(
                &scene, &models, parameters, overlap, threads, &folder, &sha, cancel, progress,
            )?;
            Ok(Outcome {
                scene: Some(scene),
                comparison: Some(Arc::new(result)),
                ..Outcome::message("All three methods completed from the same full source grid.")
            })
        })
    }

    fn save(&mut self) -> Result<()> {
        let Some(result) = self.comparison.clone() else {
            return Ok(());
        };
        let Some(parent) = dialog::choose_folder(
            self.owner()?,
            "Choose parent folder for a new complete result bundle",
        )?
        else {
            return Ok(());
        };
        self.begin(move |cancel, progress| {
            let target = ortho::export(&result, Path::new(&parent), cancel, progress)?;
            Ok(Outcome::message(format!(
                "Saved complete comparison bundle: {}",
                target.display()
            )))
        })
    }

    fn poll_work(&mut self) -> Result<()> {
        let Some(messages) = &self.messages else {
            return Ok(());
        };
        let mut done = None;
        let mut statuses = Vec::new();
        while let Ok(message) = messages.try_recv() {
            match message {
                Message::Status(text, percent) => statuses.push((text, percent)),
                Message::Done(result) => done = Some(result),
            }
        }
        if let Some((text, percent)) = statuses.pop() {
            self.status(&text)?;
            self.window.set_control_value(PROGRESS, percent, "%p%")?;
        }
        let Some(result) = done else { return Ok(()) };
        self.messages = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        let cancelled = self.cancel.load(Ordering::Relaxed);
        let outcome = match result {
            Ok(_) if cancelled => Err("cancelled".to_owned().into()),
            Ok(outcome) => self.publish(outcome),
            Err(error) => Err(error.into()),
        };
        self.set_busy(false)?;
        match outcome {
            Ok(message) => {
                self.state = message.clone();
                self.status(&message)?;
                if self.setup_models && self.scene.is_some() && self.models.is_none() {
                    self.setup_models = false;
                    self.prepare_models(None, None)?;
                }
                Ok(())
            }
            Err(error) => {
                let text = if cancelled {
                    "Cancelled; last completed comparison retained.".to_owned()
                } else {
                    error.to_string()
                };
                self.state = text.clone();
                self.status(&text)?;
                if !cancelled {
                    self.window
                        .append_log(&format!("<span style='color:#a33'>{error}</span>"))?;
                    self.log.clear();
                }
                Ok(())
            }
        }
    }

    fn publish(&mut self, outcome: Outcome) -> Result<String> {
        if let Some(models) = outcome.models {
            self.models = Some(models);
        }
        let Some(scene) = outcome.scene else {
            return Ok(outcome.message);
        };
        let changed = self
            .scene
            .as_ref()
            .is_none_or(|s| !Arc::ptr_eq(&s.dsm, &scene.dsm));
        self.scene = Some(scene.clone());
        self.comparison = outcome.comparison;
        if changed {
            self.show_input()?;
            for map in &self.panes.maps[1..] {
                map.clear_layers();
                map.refresh();
            }
            self.probe = None;
            self.probe_point = None;
            self.draw_probe();
            self.applied = [None; 4];
            self.fit();
        }
        if self.comparison.is_some() {
            self.display()?;
        } else {
            for (pane, title) in PENDING.iter().enumerate() {
                self.panes.set_heading(pane + 1, title);
            }
        }
        Ok(outcome.message)
    }

    // ----- maps ---------------------------------------------------------------------------
    /// Swap a map's raster but keep its view; a map's first layer joins the linked view.
    fn replace(&mut self, pane: usize, path: &Path, name: &str) -> Result<()> {
        let map = &self.panes.maps[pane];
        let extent = if map.layer_count() > 0 {
            map.extent()
        } else if pane > 0 && self.panes.maps[0].layer_count() > 0 {
            self.panes.maps[0].extent()
        } else {
            None
        };
        map.clear_layers();
        map.add_layer(path, name)?;
        map.set_tool(self.tool);
        if let Some(extent) = extent {
            map.set_extent(extent);
            self.applied[pane] = map.extent();
        }
        map.refresh();
        Ok(())
    }

    fn show_input(&mut self) -> Result<()> {
        let Some(scene) = self.scene.clone() else {
            return Ok(());
        };
        let rgb = self.input == 1;
        let path = if rgb {
            scene.rgb.path.clone()
        } else {
            scene.original.clone()
        };
        self.replace(
            0,
            &path,
            if rgb {
                "RGB orthophoto"
            } else {
                "Original DSM"
            },
        )?;
        self.panes.set_heading(
            0,
            if rgb {
                "RGB ORTHOPHOTO · source input"
            } else {
                "ORIGINAL DSM · source input"
            },
        );
        Ok(())
    }

    fn fit(&mut self) {
        let Some(scene) = self.scene.clone() else {
            return;
        };
        let [x0, y0, x1, y1] = if self.scope == 1 {
            ortho::crop_extent(&scene)
        } else {
            scene.dsm.extent()
        };
        let (dx, dy) = ((x1 - x0) * 0.025, (y1 - y0) * 0.025);
        self.draw_area(&scene);
        self.panes.maps[0].set_extent(Extent {
            x_min: x0 - dx,
            y_min: y0 - dy,
            x_max: x1 + dx,
            y_max: y1 + dy,
        });
        self.sync_from(0);
    }

    /// Dim everything outside the shared crop (four translucent owned polygons) and outline it.
    fn draw_area(&self, scene: &Scene) {
        let [x0, y0, x1, y1] = ortho::crop_extent(scene);
        let [ox0, oy0, ox1, oy1] = scene.dsm.extent();
        let m = (ox1 - ox0).max(oy1 - oy0);
        let (a0, b0, a1, b1) = (ox0 - m, oy0 - m, ox1 + m, oy1 + m);
        let bands = [
            [a0, y1, a1, b1],
            [a0, b0, a1, y0],
            [a0, y0, x0, y1],
            [x1, y0, a1, y1],
        ];
        let dim = json!({"fillColor": "#ffffff", "fillOpacity": 155, "lineWidth": 0, "lineColor": "#ffffff"}).to_string();
        let outline = json!({"lineColor": "#098b97", "lineWidth": 2}).to_string();
        for map in &self.panes.maps {
            map.clear_shapes();
            if self.scope == 1 {
                for [a, b, c, d] in bands {
                    map.add_polygon(&[[a, b], [c, b], [c, d], [a, d], [a, b]], &dim);
                }
                map.add_line(
                    &[[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]],
                    &outline,
                );
            }
            map.refresh(); // owned shapes are drawn on the next full map render
        }
    }

    fn sync_from(&mut self, side: usize) {
        let Some(extent) = self.panes.maps[side].extent() else {
            return;
        };
        self.applied[side] = Some(extent);
        // Empty maps have no CRS yet; they take the view when their first layer arrives.
        for other in (0..4).filter(|o| *o != side && self.panes.maps[*o].layer_count() > 0) {
            self.panes.maps[other].set_extent(extent);
            self.applied[other] = self.panes.maps[other].extent();
        }
    }

    fn display(&mut self) -> Result<()> {
        let (Some(scene), Some(result)) = (self.scene.clone(), self.comparison.clone()) else {
            return Ok(());
        };
        if self.mode == 2 && scene.reference.is_none() {
            self.mode = 0;
            self.window.set_control_value(MODE, 0.0, MODES[0])?;
            self.status("Attach an aligned reference to show errors.")?;
        }
        for method in 0..3 {
            let heading = format!("{} {}", METHODS[method].to_uppercase(), SUFFIX[self.mode]);
            self.replace(method + 1, &result.maps[method][self.mode], &heading)?;
            self.panes.set_heading(method + 1, &heading);
        }
        Ok(())
    }

    /// The probed cell as an overlay point on every map. It is moved, never removed: removal is
    /// deferred to a Qt event loop this host never runs, so a removed point would reappear.
    fn draw_probe(&self) {
        let style = json!({"pointColor": "#d73573", "pointSize": 11, "lineColor": "#ffffff", "lineWidth": 2}).to_string();
        for map in &self.panes.maps {
            map.set_overlay_point("probe", self.probe_point.unwrap_or(PARKED), &style);
        }
    }

    fn inspect(&mut self, x: f64, y: f64) {
        let Some(scene) = self.scene.clone() else {
            return;
        };
        if self.busy {
            return;
        }
        let d = &scene.dsm;
        let (cx, cy) = (
            (x - d.transform[0]) / d.pixel,
            (d.transform[3] - y) / d.pixel,
        );
        if !(cx >= 0.0 && cy >= 0.0 && cx < d.width as f64 && cy < d.height as f64) {
            return;
        }
        let (col, row) = (cx as usize, cy as usize);
        let i = row * d.width + col;
        let [r, gr, b] = [0, 1, 2].map(|k| scene.rgb.bands[k][i]);
        let mut text = format!("<b>Source pixel</b> {col} / {row}<br>RGB {r} / {gr} / {b}<br>");
        if d.valid[i] == 0 {
            text += "DSM NoData";
        } else {
            text += &format!("DSM {:.2} m", d.z[i]);
            if let Some(result) = &self.comparison {
                for method in 0..3 {
                    text += &format!("<br>{} {:.2} m", METHODS[method], result.heights(method)[i]);
                }
            }
            if let Some(reference) = scene.reference.as_ref().filter(|r| r.valid[i] != 0) {
                text += &format!("<br>Reference {:.2} m", reference.z[i]);
            }
        }
        self.probe = Some(text);
        self.probe_point = Some([
            d.transform[0] + (col as f64 + 0.5) * d.pixel,
            d.transform[3] - (row as f64 + 0.5) * d.pixel,
        ]);
        self.draw_probe();
    }

    fn handle_events(&mut self) {
        for event in map2d::drain(&self.events) {
            if self.scene.is_none() {
                continue;
            }
            let side = event.side;
            match event.kind {
                map2d::VISIBLE_EXTENT_CHANGED => {
                    let map = &self.panes.maps[side];
                    if map.layer_count() > 0 && map.extent() != self.applied[side] {
                        self.sync_from(side);
                    }
                }
                map2d::MAP_MOUSE_DOWN if event.button == 1 => {
                    self.presses[side] = Some(event.screen)
                }
                map2d::MAP_MOUSE_UP if event.button == 1 => {
                    if let Some(press) = self.presses[side].take() {
                        if self.tool == PAN
                            && (press.0 - event.screen.0).abs() + (press.1 - event.screen.1).abs()
                                <= 3
                        {
                            self.inspect(event.world.0, event.world.1);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn changed(&mut self) {
        if self.comparison.is_some() {
            self.state = CHANGED.into();
        }
    }

    fn handle(&mut self, event: geokernel::ControlEvent) -> Result<()> {
        if event.id == CANCEL && self.busy {
            self.cancel.store(true, Ordering::Relaxed);
            return self.status("Cancelling; completed comparison retained...");
        }
        if self.busy {
            return Ok(());
        }
        match event.id {
            SAMPLE => self.load_sample()?,
            OPEN => self.open_inputs()?,
            REFERENCE => self.open_reference()?,
            MODELS => self.prepare_models(None, None)?,
            PIX_ZIP => self.choose_model(0)?,
            RGB_ZIP => self.choose_model(1)?,
            RUN => self.process()?,
            SAVE => self.save()?,
            // Programmatic updates echo back as events; only a new value acts.
            OVERLAP | THREADS => {
                let value = event.number.round() as i32;
                let target = if event.id == OVERLAP {
                    &mut self.overlap
                } else {
                    &mut self.threads
                };
                if *target != value {
                    *target = value;
                    self.changed();
                }
            }
            PRESET => {
                if let Some(i) = PRESET_NAMES.iter().position(|p| *p == event.text) {
                    if i != self.preset {
                        self.preset = i;
                        self.changed();
                    }
                }
            }
            INPUT => {
                if let Some(i) = INPUTS.iter().position(|m| *m == event.text) {
                    if i != self.input {
                        self.input = i;
                        self.show_input()?;
                    }
                }
            }
            MODE => {
                if let Some(i) = MODES.iter().position(|m| *m == event.text) {
                    if i != self.mode {
                        self.mode = i;
                        self.display()?;
                    }
                }
            }
            SCOPE => {
                if let Some(i) = SCOPES.iter().position(|m| *m == event.text) {
                    if i != self.scope {
                        self.scope = i;
                        self.fit();
                    }
                }
            }
            PAN_TOOL | BOX_TOOL => {
                self.tool = if event.id == PAN_TOOL { PAN } else { ZOOM_BOX };
                for map in &self.panes.maps {
                    map.set_tool(self.tool);
                }
            }
            ZOOM_IN | ZOOM_OUT => self.panes.maps[0].zoom(event.id == ZOOM_IN),
            FIT => self.fit(),
            _ => {}
        }
        Ok(())
    }

    fn tick(&mut self) -> Result<()> {
        self.panes.layout()?;
        self.handle_events();
        self.poll_work()?;
        self.refresh_text()
    }
}

fn run(runtime: &Runtime) -> Result<()> {
    let options = Arc::new(sample::Options::parse()?);
    let mut window = unsafe {
        ViewerWindow::new(
            runtime,
            "DSM + Orthophoto to DTM | Three independent methods — GeoKernel",
            1920,
            980,
        )?
    };
    let controls = window.add_control_panel(
        &json!({
            "title": "DSM + RGB → DTM · Compare three methods",
            "controls": [
                {"id": SAMPLE, "type": "button", "text": "Load Zurich DSM + orthophoto"},
                {"id": OPEN, "type": "button", "text": "Open aligned DSM + RGB..."},
                {"id": REFERENCE, "type": "button", "text": "Open reference (optional)..."},
                {"id": MODELS, "type": "button", "text": "Download / verify both models"},
                {"id": PIX_ZIP, "type": "button", "text": "Use pix_grad model ZIP..."},
                {"id": RGB_ZIP, "type": "button", "text": "Use RGB model ZIP..."},
                {"id": OVERLAP, "type": "number", "label": "AI overlap (px)", "minimum": 0, "maximum": 128, "step": 32, "decimals": 0, "value": 64},
                {"id": THREADS, "type": "number", "label": "CPU threads", "minimum": 1, "maximum": 8, "step": 1, "decimals": 0, "value": 4},
                {"id": PRESET, "type": "combo", "label": "Classical preset", "options": PRESET_NAMES, "value": PRESET_NAMES[0]},
                {"id": RUN, "type": "button", "text": "Run all three methods"},
                {"id": CANCEL, "type": "button", "text": "Cancel"},
                {"id": INPUT, "type": "combo", "label": "Input context", "options": INPUTS, "value": INPUTS[1]},
                {"id": MODE, "type": "combo", "label": "Compare estimates", "options": MODES, "value": MODES[0]},
                {"id": SCOPE, "type": "combo", "label": "Evaluation scope", "options": SCOPES, "value": SCOPES[1]},
                {"id": SAVE, "type": "button", "text": "Save comparison folder..."},
                {"id": PAN_TOOL, "type": "button", "text": "Pan"},
                {"id": BOX_TOOL, "type": "button", "text": "Zoom box"},
                {"id": ZOOM_IN, "type": "button", "text": "Zoom in"},
                {"id": ZOOM_OUT, "type": "button", "text": "Zoom out"},
                {"id": FIT, "type": "button", "text": "Fit comparison"},
                {"id": PROGRESS, "type": "progress", "label": "Progress", "value": 0, "format": "%p%"}
            ]
        })
        .to_string(),
    )?;
    window.add_log_panel("Source, models and probe")?;
    window.show()?;
    window.process_events();
    let parent = window.viewer().get_native_handle()?;
    let bin = PathBuf::from(env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    let (events_tx, events) = mpsc::channel();
    // The panes (and their native maps) are dropped before the owning window and Qt runtime.
    let panes = unsafe {
        Panes::new(
            parent,
            &bin.join("GeoKernel.Viewer.dll"),
            events_tx,
            &TITLES,
        )?
    };
    let temporary = env::temp_dir().join(format!("geokernel-dsm-ortho-{}", std::process::id()));
    fs::create_dir_all(&temporary)?;
    let mut app = App {
        window,
        panes,
        events,
        options,
        archive_sha256: Arc::new([
            sample::Assets::model(0).sha256,
            sample::Assets::model(1).sha256,
        ]),
        temporary: temporary.clone(),
        cancel: Arc::new(AtomicBool::new(true)),
        messages: None,
        worker: None,
        scene: None,
        comparison: None,
        models: None,
        setup_models: false,
        overlap: 64,
        threads: 4,
        preset: 0,
        input: 1,
        mode: 0,
        scope: 1,
        tool: PAN,
        probe: None,
        probe_point: None,
        state: "Load data and models, then Run.".into(),
        presses: [None; 4],
        applied: [None; 4],
        log: String::new(),
        busy: false,
        generation: 0,
    };
    app.set_busy(false)?;
    app.load_sample()?;
    let result = (|| -> Result<()> {
        while app.window.is_visible()? {
            app.window.process_events();
            if !app.window.is_visible()? {
                break;
            }
            while let Ok(event) = controls.try_recv() {
                if let Err(error) = app.handle(event) {
                    app.status(&error.to_string())?;
                }
            }
            if let Err(error) = app.tick() {
                app.status(&error.to_string())?;
            }
            thread::sleep(Duration::from_millis(16));
        }
        Ok(())
    })();
    // Closing during a job: stop and join the worker before the maps are released. The inference
    // call has no cancellation hook, so a running AI pass finishes first (a few seconds).
    app.cancel.store(true, Ordering::Relaxed);
    if let Some(worker) = app.worker.take() {
        let _ = worker.join();
    }
    drop(app.panes);
    drop(app.window);
    let _ = fs::remove_dir_all(&temporary);
    result
}

fn main() -> Result<()> {
    let runtime = load_runtime()?;
    let result = run(&runtime);
    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    result?;
    shutdown?;
    Ok(())
}
