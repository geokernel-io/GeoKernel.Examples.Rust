//! DSM to DTM: live classical processing.
//!
//! Run the classical conversion (native-multiscale-morphological-tin-v1) on the input DSM, change
//! parameters, inspect the newly generated terrain and export numeric GeoTIFFs. Three linked
//! native GeoKernel 2D maps show the original DSM, the generated DTM and a selectable diagnostic
//! map. The reference is evaluation-only; it is never an input to processing.
mod dialog;
mod gdal;
mod map2d;
mod panes;
mod sample;
mod terrain;

use gdal::{Pixels, Raster};
use geokernel::{Runtime, ViewerWindow};
use map2d::{Event, Extent};
use panes::Panes;
use serde_json::{json, Value};
use std::{
    env,
    ffi::OsStr,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};
use terrain::{Parameters, Score, Terrain, PRESETS, PRESET_NAMES};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const TITLES: [&str; 3] = [
    "ORIGINAL DSM · input",
    "PROCESSED DTM · Run to generate",
    "REMOVED HEIGHT · Run to generate",
];
const MODES: [&str; 3] = [
    "Removed height: DSM − DTM",
    "Support / edge quality",
    "Signed reference error",
];
const DIAGNOSTIC_TITLES: [&str; 3] = [
    "REMOVED HEIGHT · DSM − DTM",
    "PROCESSING QUALITY · supports / edges",
    "REFERENCE ERROR · DTM − reference",
];
const LEGENDS: [&str; 3] = [
    "Removed height: white 0 m → dark red ≥20 m. Display saturation only. NoData gray.",
    "Quality: teal retained source · pale interpolated · orange edge-sensitive · purple hull fallback · gray NoData. Full flag combinations are saved in QA GeoTIFF.",
    "Signed error: blue ≤−2 m · white 0 · red ≥+2 m. Shared fixed scale; numeric scores are not clipped.",
];
const CHANGED: &str = "Controls changed. Run to update; displayed result and scores still use the last completed run.";
const EDGE_NOTE: &str = "Edge-sensitive band = half the maximum filter window. Use a larger DSM and crop afterward for better context. This prototype is independent of ArcGIS Terrain Filter.";
const PARKED: [f64; 2] = [-1.0e9, -1.0e9];
const PAN: i32 = 0;
const ZOOM_BOX: i32 = 1;

// Control panel IDs.
const SAMPLE: i32 = 1;
const OPEN: i32 = 2;
const REFERENCE: i32 = 3;
const PRESET: i32 = 4;
const WORKING: i32 = 5; // ..=10: the six parameters in Parameters order
const RUN: i32 = 11;
const CANCEL: i32 = 12;
const MODE: i32 = 13;
const SAVE: i32 = 14;
const PAN_TOOL: i32 = 15;
const BOX_TOOL: i32 = 16;
const ZOOM_IN: i32 = 17;
const ZOOM_OUT: i32 = 18;
const FIT: i32 = 19;
const PROGRESS: i32 = 20;

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

/// A loaded source: DSM, optional evaluation-only reference and its relief display.
struct Scene {
    dsm: Arc<Raster>,
    reference: Option<Arc<Raster>>,
    original: PathBuf,
    manifest: Value,
}

/// One completed run: numeric GeoTIFFs, display maps, scores and the provenance report.
#[derive(Clone)]
struct Run {
    terrain: Arc<Terrain>,
    dtm: PathBuf,
    removed: PathBuf,
    quality: PathBuf,
    display: PathBuf,
    diagnostics: [PathBuf; 3],
    all: Score,
    interior: Score,
    report: Value,
}

/// A finished job: a new scene/run to publish (or none, for a save) and its status message.
struct Outcome {
    scene: Option<Arc<Scene>>,
    run: Option<Arc<Run>>,
    message: String,
}

enum Message {
    Status(String, f64),
    Done(sample::Result<Outcome>),
}

type Progress<'a> = &'a dyn Fn(f64, &str);

fn rgb(
    scene_dsm: &Raster,
    path: PathBuf,
    bands: sample::Result<[Vec<u8>; 3]>,
) -> sample::Result<PathBuf> {
    gdal::write_rgb(&path, scene_dsm, &bands?)
}

fn load_scene(
    dsm: &Path,
    reference: Option<&Path>,
    manifest: Value,
    folder: &Path,
    cancel: &AtomicBool,
    progress: Progress,
) -> sample::Result<Outcome> {
    progress(5.0, "Reading source DSM...");
    let dsm = gdal::read_raster(dsm, cancel)?;
    let reference = match reference {
        Some(path) => {
            progress(30.0, "Reading evaluation-only reference...");
            let reference = gdal::read_raster(path, cancel)?;
            gdal::validate_reference(&dsm, &reference)?;
            Some(Arc::new(reference))
        }
        None => None,
    };
    progress(60.0, "Preparing original DSM relief...");
    let original = rgb(
        &dsm,
        folder.join("original.tif"),
        terrain::relief(&dsm, &dsm.z, cancel),
    )?;
    sample::check(cancel)?;
    let scene = Scene {
        dsm: Arc::new(dsm),
        reference,
        original,
        manifest,
    };
    Ok(Outcome {
        scene: Some(Arc::new(scene)),
        run: None,
        message: "Source ready. Choose parameters and Run.".into(),
    })
}

fn scored(scene: &Scene, mut run: Run) -> Run {
    let reference = scene.reference.as_deref();
    let (all, interior) = terrain::evaluate(&scene.dsm, &run.terrain, reference);
    run.report = terrain::report(
        &scene.dsm,
        reference,
        &scene.manifest,
        &run.terrain,
        all,
        interior,
    );
    (run.all, run.interior) = (all, interior);
    run
}

fn process(
    scene: Arc<Scene>,
    parameters: Parameters,
    folder: &Path,
    cancel: &AtomicBool,
    progress: Progress,
) -> sample::Result<Outcome> {
    let d = &scene.dsm;
    let terrain = terrain::estimate(d, parameters, cancel, progress)?;
    let dtm = gdal::write_raster(d, &folder.join("dtm.tif"), Pixels::Heights(&terrain.dtm))?;
    sample::check(cancel)?;
    let removed = gdal::write_raster(
        d,
        &folder.join("removed.tif"),
        Pixels::Heights(&terrain.removed),
    )?;
    sample::check(cancel)?;
    let quality = gdal::write_raster(
        d,
        &folder.join("quality.tif"),
        Pixels::Quality(&terrain.quality),
    )?;
    let display = rgb(
        d,
        folder.join("terrain-relief.tif"),
        terrain::relief(d, &terrain.dtm, cancel),
    )?;
    let mut diagnostics = Vec::new();
    for mode in 0..3 {
        diagnostics.push(rgb(
            d,
            folder.join(format!("diagnostic-{mode}.tif")),
            terrain::diagnostic(d, &terrain, mode, scene.reference.as_deref(), cancel),
        )?);
    }
    progress(96.0, "Scoring reference after processing...");
    let run = Run {
        terrain: Arc::new(terrain),
        dtm,
        removed,
        quality,
        display,
        diagnostics: diagnostics
            .try_into()
            .map_err(|_| "Diagnostic maps missing.")?,
        all: Score::default(),
        interior: Score::default(),
        report: Value::Null,
    };
    let run = scored(&scene, run);
    sample::check(cancel)?;
    Ok(Outcome {
        scene: Some(scene),
        run: Some(Arc::new(run)),
        message: "Processing complete. Change parameters to run again, or save the result.".into(),
    })
}

/// Copy through a temporary sibling, flushed to disk, then replace the target.
fn atomic_write(
    target: &Path,
    write: &dyn Fn(&mut fs::File) -> std::io::Result<()>,
    cancel: &AtomicBool,
) -> sample::Result<()> {
    let name = target
        .file_name()
        .ok_or("Invalid output name.")?
        .to_string_lossy();
    let temporary = target.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| -> sample::Result<()> {
        let mut file = fs::File::create(&temporary).map_err(|e| e.to_string())?;
        write(&mut file).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        sample::check(cancel)?;
        fs::rename(&temporary, target).map_err(|e| e.to_string())
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn save(
    run: &Run,
    destinations: &[PathBuf; 4],
    cancel: &AtomicBool,
    progress: Progress,
) -> sample::Result<Outcome> {
    let mut saved = Vec::new();
    let result = (|| -> sample::Result<()> {
        for (i, source) in [&run.dtm, &run.removed, &run.quality]
            .into_iter()
            .enumerate()
        {
            progress(
                i as f64 * 30.0,
                &format!(
                    "Saving {}",
                    destinations[i]
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                ),
            );
            let bytes = fs::read(source).map_err(|e| e.to_string())?;
            sample::check(cancel)?;
            atomic_write(&destinations[i], &|file| file.write_all(&bytes), cancel)?;
            saved.push(destinations[i].display().to_string());
        }
        sample::check(cancel)?;
        let text = serde_json::to_string_pretty(&run.report).map_err(|e| e.to_string())?;
        atomic_write(
            &destinations[3],
            &|file| file.write_all(text.as_bytes()),
            cancel,
        )
    })();
    match result {
        Err(error) if !saved.is_empty() && !cancel.load(Ordering::Relaxed) => Err(format!(
            "{error}\nFiles already committed:\n{}",
            saved.join("\n")
        )),
        Err(error) => Err(error),
        Ok(()) => Ok(Outcome {
            scene: None,
            run: None,
            message: "Saved DTM, removed heights, QA flags and provenance JSON.".into(),
        }),
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    let canonical = |p: &Path| {
        std::path::absolute(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .display()
            .to_string()
            .to_lowercase()
    };
    canonical(a) == canonical(b)
}

struct App {
    window: ViewerWindow,
    panes: Panes,
    events: mpsc::Receiver<Event>,
    options: Arc<sample::Options>,
    temporary: PathBuf,
    cancel: Arc<AtomicBool>,
    messages: Option<mpsc::Receiver<Message>>,
    worker: Option<thread::JoinHandle<()>>,
    scene: Option<Arc<Scene>>,
    run: Option<Arc<Run>>,
    parameters: Parameters,
    preset: usize,
    mode: usize,
    tool: i32,
    probe: Option<String>,
    probe_point: Option<[f64; 2]>,
    state: String,
    presses: [Option<(i32, i32)>; 3],
    applied: [Option<Extent>; 3],
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
        let (has_scene, has_run) = (self.scene.is_some(), self.run.is_some());
        for id in [
            SAMPLE,
            OPEN,
            PRESET,
            WORKING,
            WORKING + 1,
            WORKING + 2,
            WORKING + 3,
            WORKING + 4,
            WORKING + 5,
        ] {
            self.window.set_control_enabled(id, !busy)?;
        }
        for id in [RUN, REFERENCE, PAN_TOOL, BOX_TOOL, ZOOM_IN, ZOOM_OUT, FIT] {
            self.window.set_control_enabled(id, !busy && has_scene)?;
        }
        for id in [SAVE, MODE] {
            self.window.set_control_enabled(id, !busy && has_run)?;
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

    /// Source, state, scores and the probed pixel in the log panel; legend and summary below the maps.
    fn refresh_text(&mut self) -> Result<()> {
        let source = match &self.scene {
            None => "No source loaded.".to_owned(),
            Some(s) => format!(
                "{}<br>{} × {} · {} m cells<br>{}",
                s.dsm.path.file_name().unwrap_or_default().to_string_lossy(),
                s.dsm.width,
                s.dsm.height,
                g(s.dsm.pixel),
                if s.reference.is_some() {
                    "Aligned reference attached"
                } else {
                    "No reference: heights can still be processed"
                }
            ),
        };
        let line = |name: &str, s: &Score| {
            if s.count > 0 {
                format!(
                    "{name} ({} px)<br>RMSE {:.3} m · MAE {:.3} m · bias {:.3} m",
                    s.count, s.rmse, s.mae, s.bias
                )
            } else {
                format!("{name}: no common valid reference pixels")
            }
        };
        let metrics = match (&self.scene, &self.run) {
            (_, None) => "Reference errors appear after a completed run.".to_owned(),
            (Some(s), Some(r)) if s.reference.is_some() => {
                format!("{}<br><br>{}", line("All valid reference cells", &r.all), line("Interior outside edge band", &r.interior))
            }
            _ => "No reference attached. Scores are unavailable; processing does not require a reference.".to_owned(),
        };
        let probe = self
            .probe
            .clone()
            .unwrap_or_else(|| "Click any map to inspect source heights.".to_owned());
        let log = format!(
            "<b>DSM → DTM · Run the conversion</b><br>Live multiscale filtering + TIN. Change parameters, then Run.<br><br>\
             {source}<br><br><i>{}</i><br><br>{metrics}<br><br>{probe}<br><br>\
             Sample: Federal Office of Topography swisstopo. Reference is evaluation-only. Heights are metres; no vertical datum conversion.",
            self.state
        );
        if log != self.log {
            self.window.clear_log()?;
            self.window.append_log(&log)?;
            self.log = log;
        }
        let footer = match &self.run {
            None => format!("Original DSM remains visible. Both relief maps use identical lighting and contrast.\n{}\n{EDGE_NOTE}",
                if self.scene.is_some() { "Source loaded. Run to generate a new terrain estimate." } else { "No processing has run yet." }),
            Some(r) => {
                let (t, p) = (&r.terrain, &r.terrain.parameters);
                format!(
                    "{}\nLast completed run · {} supports · {:.2} s · working {} m · max window {} m · base {} m · slope {} · relief {} · keep {} m · \
                     edge band {} m · hull fallback {} pixels · reference is evaluation-only.\n{EDGE_NOTE}",
                    LEGENDS[self.mode], t.supports, t.seconds, g(t.working), g(p.window), g(p.threshold), g(p.slope), g(p.terrain), g(p.retain),
                    g(p.window * 0.5), t.fallback
                )
            }
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

    fn load_sample(&mut self) -> Result<()> {
        let (folder, options) = (self.folder()?, self.options.clone());
        self.begin(move |cancel, progress| {
            progress(0.0, "Checking cached sample files...");
            let assets = sample::Assets::zurich_v2();
            let sample_folder = sample::prepare(&assets, &options, cancel, &|text: String| {
                progress(0.0, &text)
            })?;
            sample::check(cancel)?;
            gdal::configure_runtime(&sample_folder, &set_env)?;
            let manifest: Value = serde_json::from_str(
                &fs::read_to_string(sample_folder.join("source-manifest.json"))
                    .map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
            load_scene(
                &sample_folder.join("dsm.tif"),
                Some(&sample_folder.join("dtm_reference.tif")),
                manifest,
                &folder,
                cancel,
                progress,
            )
        })
    }

    fn owner(&mut self) -> Result<usize> {
        Ok(self.window.viewer().get_native_handle()?)
    }

    fn open_dsm(&mut self) -> Result<()> {
        let Some(path) = dialog::open_tif(
            self.owner()?,
            "Open DSM: metre heights in projected metre CRS",
        )?
        else {
            return Ok(());
        };
        let folder = self.folder()?;
        self.begin(move |cancel, progress| {
            load_scene(Path::new(&path), None, json!({}), &folder, cancel, progress)
        })
    }

    fn open_reference(&mut self) -> Result<()> {
        let Some(scene) = self.scene.clone() else {
            return Ok(());
        };
        let Some(path) =
            dialog::open_tif(self.owner()?, "Open aligned reference (evaluation only)")?
        else {
            return Ok(());
        };
        let (folder, completed) = (self.folder()?, self.run.clone());
        self.begin(move |cancel, progress| {
            progress(10.0, "Reading evaluation-only reference...");
            let reference = gdal::read_raster(Path::new(&path), cancel)?;
            gdal::validate_reference(&scene.dsm, &reference)?;
            let next = Arc::new(Scene {
                dsm: scene.dsm.clone(),
                reference: Some(Arc::new(reference)),
                original: scene.original.clone(),
                manifest: scene.manifest.clone(),
            });
            let run = match completed {
                None => None,
                Some(run) => {
                    let mut run = (*run).clone();
                    let bands = terrain::diagnostic(
                        &next.dsm,
                        &run.terrain,
                        2,
                        next.reference.as_deref(),
                        cancel,
                    );
                    run.diagnostics[2] = rgb(&next.dsm, folder.join("reference-error.tif"), bands)?;
                    Some(Arc::new(scored(&next, run)))
                }
            };
            Ok(Outcome {
                scene: Some(next),
                run,
                message: "Reference attached; processing heights unchanged.".into(),
            })
        })
    }

    fn process(&mut self) -> Result<()> {
        let Some(scene) = self.scene.clone() else {
            return Ok(());
        };
        let (folder, parameters) = (self.folder()?, self.parameters);
        self.begin(move |cancel, progress| process(scene, parameters, &folder, cancel, progress))
    }

    fn save(&mut self) -> Result<()> {
        let (Some(scene), Some(run)) = (self.scene.clone(), self.run.clone()) else {
            return Ok(());
        };
        let owner = self.owner()?;
        let Some(target) = dialog::save_tif(owner)? else {
            return Ok(());
        };
        let mut target = PathBuf::from(target);
        if !target
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("tif"))
        {
            target.set_extension("tif");
        }
        let stem = target.with_extension("");
        let with = |suffix: &str| PathBuf::from(format!("{}{suffix}", stem.display()));
        let destinations = [
            target.clone(),
            with(".removed.tif"),
            with(".qa.tif"),
            with(".json"),
        ];
        let sources: Vec<&Path> = [
            Some(scene.dsm.path.as_path()),
            scene.reference.as_ref().map(|r| r.path.as_path()),
        ]
        .into_iter()
        .flatten()
        .collect();
        if destinations
            .iter()
            .any(|d| sources.iter().any(|s| same_file(d, s)))
        {
            dialog::warn(
                owner,
                "Cannot export",
                "Choose output names different from the source and reference.",
            );
            return Ok(());
        }
        if destinations[1..].iter().any(|d| d.exists())
            && !dialog::confirm(
                owner,
                "Replace existing result companions?",
                "Removed-height, QA or JSON companion files already exist. Replace the result set?",
            )
        {
            return Ok(());
        }
        self.begin(move |cancel, progress| save(&run, &destinations, cancel, progress))
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
        for (text, percent) in statuses {
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
                self.status(&message)
            }
            Err(error) => {
                let text = if cancelled {
                    "Cancelled. Last completed source/result retained.".to_owned()
                } else {
                    error.to_string()
                };
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
        let Some(scene) = outcome.scene else {
            return Ok(outcome.message);
        };
        let changed = self
            .scene
            .as_ref()
            .is_none_or(|s| !Arc::ptr_eq(&s.dsm, &scene.dsm));
        self.scene = Some(scene.clone());
        self.run = outcome.run;
        if changed {
            self.replace(0, &scene.original, "Original DSM")?;
            for map in &self.panes.maps[1..] {
                map.clear_layers();
                map.refresh();
            }
            self.probe = None;
            self.probe_point = None;
            self.draw_probe();
            self.applied = [None; 3];
            self.fit();
        }
        match self.run.clone() {
            Some(run) => {
                self.replace(1, &run.display, "Processed DTM")?;
                self.show_diagnostic()?;
            }
            None => {
                self.panes.set_heading(1, TITLES[1]);
                self.panes.set_heading(2, TITLES[2]);
            }
        }
        Ok(outcome.message)
    }

    // ----- maps ---------------------------------------------------------------------------
    /// Swap a map's raster but keep its view; the empty map adopts the raster's CRS.
    fn replace(&mut self, pane: usize, path: &Path, name: &str) -> Result<()> {
        let map = &self.panes.maps[pane];
        // Keep this map's view; a map receiving its first layer takes the linked view.
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

    fn fit(&mut self) {
        let Some(scene) = &self.scene else { return };
        let [x0, y0, x1, y1] = scene.dsm.extent();
        let pad = scene.dsm.pixel * 10.0;
        self.panes.maps[0].set_extent(Extent {
            x_min: x0 - pad,
            y_min: y0 - pad,
            x_max: x1 + pad,
            y_max: y1 + pad,
        });
        self.sync_from(0);
    }

    fn sync_from(&mut self, side: usize) {
        let Some(extent) = self.panes.maps[side].extent() else {
            return;
        };
        self.applied[side] = Some(extent);
        // Empty maps have no CRS yet; they take the view when their first layer arrives.
        for other in (0..3).filter(|o| *o != side && self.panes.maps[*o].layer_count() > 0) {
            self.panes.maps[other].set_extent(extent);
            self.applied[other] = self.panes.maps[other].extent();
        }
    }

    fn show_diagnostic(&mut self) -> Result<()> {
        let (Some(scene), Some(run)) = (self.scene.clone(), self.run.clone()) else {
            return Ok(());
        };
        if self.mode == 2 && scene.reference.is_none() {
            self.mode = 0;
            self.window.set_control_value(MODE, 0.0, MODES[0])?;
            self.status("Attach an aligned reference to show errors.")?;
        }
        self.replace(2, &run.diagnostics[self.mode], DIAGNOSTIC_TITLES[self.mode])?;
        self.panes
            .set_heading(1, "PROCESSED DTM · live native result");
        self.panes.set_heading(2, DIAGNOSTIC_TITLES[self.mode]);
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
        let mut text = format!("<b>Source pixel</b> · {col} / {row}<br>");
        if d.valid[i] == 0 {
            text += "NoData";
        } else {
            text += &format!("DSM {:.2} m", d.z[i]);
            if let Some(run) = &self.run {
                let t = &run.terrain;
                text += &format!(
                    "<br>DTM {:.2} m · removed {:.2} m<br>QA bits: {}",
                    t.dtm[i], t.removed[i], t.quality[i]
                );
            }
            if let Some(reference) = scene.reference.as_ref().filter(|r| r.valid[i] != 0) {
                text += &format!("<br>Reference {:.2} m", reference.z[i]);
                if let Some(run) = &self.run {
                    text += &format!(
                        " · error {:.3} m",
                        f64::from(run.terrain.dtm[i]) - f64::from(reference.z[i])
                    );
                }
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

    fn set_parameters(&mut self, p: Parameters) -> Result<()> {
        for (k, value) in [
            p.working,
            p.window,
            p.threshold,
            p.slope,
            p.terrain,
            p.retain,
        ]
        .into_iter()
        .enumerate()
        {
            self.window
                .set_control_value(WORKING + k as i32, value, "")?;
        }
        self.parameters = p;
        Ok(())
    }

    fn handle(&mut self, event: geokernel::ControlEvent) -> Result<()> {
        if event.id == CANCEL && self.busy {
            self.cancel.store(true, Ordering::Relaxed);
            return self.status("Cancelling; completed result will be retained...");
        }
        if self.busy {
            return Ok(());
        }
        match event.id {
            SAMPLE => self.load_sample()?,
            OPEN => self.open_dsm()?,
            REFERENCE => self.open_reference()?,
            RUN => self.process()?,
            SAVE => self.save()?,
            // Programmatic preset updates echo back as events; only a new choice acts.
            PRESET => {
                if let Some(i) = PRESET_NAMES.iter().position(|p| *p == event.text) {
                    if i != self.preset {
                        self.preset = i;
                        self.set_parameters(PRESETS[i])?;
                    }
                }
            }
            id if (WORKING..WORKING + 6).contains(&id) => {
                let mut values = [
                    self.parameters.working,
                    self.parameters.window,
                    self.parameters.threshold,
                    self.parameters.slope,
                    self.parameters.terrain,
                    self.parameters.retain,
                ];
                let k = (id - WORKING) as usize;
                if values[k] != event.number {
                    values[k] = event.number;
                    let [working, window, threshold, slope, terrain, retain] = values;
                    self.parameters = Parameters {
                        working,
                        window,
                        threshold,
                        slope,
                        terrain,
                        retain,
                    };
                    if self.run.is_some() {
                        self.state = CHANGED.into();
                    }
                }
            }
            MODE => {
                if let Some(i) = MODES.iter().position(|m| *m == event.text) {
                    if i != self.mode {
                        self.mode = i;
                        self.show_diagnostic()?;
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
            "DSM to DTM | Live classical processing — GeoKernel",
            1720,
            960,
        )?
    };
    let p = PRESETS[0];
    let controls = window.add_control_panel(
        &json!({
            "title": "DSM → DTM · Run the conversion",
            "controls": [
                {"id": SAMPLE, "type": "button", "text": "Load Zurich sample"},
                {"id": OPEN, "type": "button", "text": "Open DSM GeoTIFF..."},
                {"id": REFERENCE, "type": "button", "text": "Open reference (optional)..."},
                {"id": PRESET, "type": "combo", "label": "Preset", "options": PRESET_NAMES, "value": PRESET_NAMES[0]},
                {"id": WORKING, "type": "number", "label": "Working grid (m)", "minimum": 0.25, "maximum": 20, "step": 0.5, "decimals": 2, "value": p.working},
                {"id": WORKING + 1, "type": "number", "label": "Max. window (m)", "minimum": 4, "maximum": 200, "step": 4, "decimals": 1, "value": p.window},
                {"id": WORKING + 2, "type": "number", "label": "Base threshold (m)", "minimum": 0.05, "maximum": 3, "step": 0.05, "decimals": 2, "value": p.threshold},
                {"id": WORKING + 3, "type": "number", "label": "Slope allowance", "minimum": 0, "maximum": 0.5, "step": 0.02, "decimals": 2, "value": p.slope},
                {"id": WORKING + 4, "type": "number", "label": "Relief allowance", "minimum": 0, "maximum": 2, "step": 0.1, "decimals": 2, "value": p.terrain},
                {"id": WORKING + 5, "type": "number", "label": "Keep tolerance (m)", "minimum": 0, "maximum": 1, "step": 0.05, "decimals": 2, "value": p.retain},
                {"id": RUN, "type": "button", "text": "Run DSM → DTM"},
                {"id": CANCEL, "type": "button", "text": "Cancel"},
                {"id": MODE, "type": "combo", "label": "Right map", "options": MODES, "value": MODES[0]},
                {"id": SAVE, "type": "button", "text": "Save result GeoTIFFs..."},
                {"id": PAN_TOOL, "type": "button", "text": "Pan"},
                {"id": BOX_TOOL, "type": "button", "text": "Zoom box"},
                {"id": ZOOM_IN, "type": "button", "text": "Zoom in"},
                {"id": ZOOM_OUT, "type": "button", "text": "Zoom out"},
                {"id": FIT, "type": "button", "text": "Fit scene"},
                {"id": PROGRESS, "type": "progress", "label": "Progress", "value": 0, "format": "%p%"}
            ]
        })
        .to_string(),
    )?;
    window.add_log_panel("Source and scores")?;
    window.show()?;
    window.process_events();
    let parent = window.viewer().get_native_handle()?;
    let bin = PathBuf::from(env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    let (events_tx, events) = mpsc::channel();
    // The panes (and their native maps) are dropped before the owning window and Qt runtime.
    let panes =
        unsafe { Panes::new(parent, &bin.join("GeoKernel.Viewer.dll"), events_tx, TITLES)? };
    let temporary =
        env::temp_dir().join(format!("geokernel-dsm-processing-{}", std::process::id()));
    fs::create_dir_all(&temporary)?;
    let mut app = App {
        window,
        panes,
        events,
        options,
        temporary: temporary.clone(),
        cancel: Arc::new(AtomicBool::new(true)),
        messages: None,
        worker: None,
        scene: None,
        run: None,
        parameters: PRESETS[0],
        preset: 0,
        mode: 0,
        tool: PAN,
        probe: None,
        probe_point: None,
        state: "Load a DSM to begin.".into(),
        presses: [None; 3],
        applied: [None; 3],
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
    // Closing during a job: stop and join the worker before the maps are released.
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
