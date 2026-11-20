//! DSM to DTM: ground beneath buildings.
//!
//! Two linked native GeoKernel 3D viewers show the original DSM beside a prepared terrain
//! estimate (classical morphological/TIN or experimental DSM-only pix_grad). The surface
//! slider interpolates numeric heights between both endpoints. No inference runs here; the
//! reference DTM is used only for the profile chart and the stored crop scores.
mod gdal;
mod panes;
mod sample;

use gdal::Grids;
use geokernel::{Runtime, ViewerWindow};
use panes::Panes;
use serde_json::json;
use std::{
    env,
    ffi::OsStr,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const METHODS: [&str; 2] = ["Classical · morphological / TIN", "AI · DSM-only pix_grad"];
const READY: &str =
    "Ready · Original DSM stays visible | Drag: pan · Wheel: zoom · Middle/Ctrl+drag: orbit";

// Control panel IDs.
const METHOD: i32 = 1;
const SURFACE: i32 = 2;
const ORIGINAL: i32 = 3;
const TERRAIN: i32 = 4;
const EXAGGERATION: i32 = 5;
const RESET: i32 = 6;
const TOP: i32 = 7;
const ZOOM_IN: i32 = 8;
const ZOOM_OUT: i32 = 9;
const PROFILE: i32 = 10;
const RETRY: i32 = 11;
const CANCEL: i32 = 12;
const FOLDER: i32 = 13;
const PROGRESS: i32 = 14;

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

#[derive(Clone, Copy, PartialEq)]
struct Selection {
    method: usize,
    percent: u32,
}

struct Work {
    grids: Arc<Grids>,
    selection: Selection,
    original: Option<PathBuf>,
    result: PathBuf,
}

enum Message {
    Status(String),
    Done(sample::Result<Work>),
}

fn heading(selection: Selection) -> String {
    let name = if selection.method == 1 {
        "CLASSICAL"
    } else {
        "AI · pix_grad"
    };
    match selection.percent {
        100 => format!("{name} DTM · prepared terrain estimate"),
        0 => "ORIGINAL DSM · slider at 0%".into(),
        p => format!("{name} · {p}% removal transition (display only)"),
    }
}

fn percent_label(value: u32) -> String {
    match value {
        100 => "100% · prepared DTM endpoint".into(),
        0 => "0% · original DSM endpoint".into(),
        p => format!("{p}% · display interpolation"),
    }
}

fn info(grids: Option<&Grids>, selection: Selection) -> String {
    let metrics = grids.map_or_else(|| "Preparing the sample...".to_owned(), |g| format!(
        "<b>REFERENCE CHECK · this crop only</b><br>Classical: RMSE {:.2} m · MAE {:.2} m<br>AI: RMSE {:.2} m · MAE {:.2} m<br>\
         250,000 shared reference pixels. Lower error is better.<br>Scores describe the final DTM endpoints, not the transition slider.",
        g.metric("classical", "rmse_m"), g.metric("classical", "mae_m"), g.metric("ai", "rmse_m"), g.metric("ai", "mae_m")));
    format!(
        "<b>Ground beneath buildings</b><br>Zürich · 250 × 250 m · 0.5 m height pixels · EPSG:2056<br><br>\
         1&nbsp; Inspect the original roofs and trees.<br>2&nbsp; Choose a prepared terrain estimate.<br>3&nbsp; Move the surface slider to reveal the ground.<br><br>\
         Surface: <b>{}</b><br>Intermediate positions interpolate heights for display. They are not new terrain estimates.<br><br>{metrics}<br><br>\
         <i>Prepared results · no inference runs here. Both methods start from the original DSM independently.</i><br><br>\
         Profile: gray DSM · teal classical · purple AI · dashed orange reference, on a fixed building transect.<br>\
         Local 3D preview of source LN02 heights; no geoid conversion. DSM/reference source years: 2018/2019. Fixed development crop; scores are not a held-out accuracy benchmark.<br><br>\
         Left drag: pan · Wheel or right drag: zoom · Middle drag or Ctrl + left drag: orbit. Both cameras stay linked.<br><br>\
         Source: Federal Office of Topography swisstopo · AI: David Perez, experimental pix_grad",
        percent_label(selection.percent))
}

struct App {
    window: ViewerWindow,
    panes: Panes,
    options: Arc<sample::Options>,
    temporary: PathBuf,
    cancel: Arc<AtomicBool>,
    messages: Option<mpsc::Receiver<Message>>,
    worker: Option<thread::JoinHandle<()>>,
    loading: Vec<usize>,
    pending: Option<Work>,
    grids: Option<Arc<Grids>>,
    committed: Selection,
    requested: Selection,
    debounce: Option<Instant>,
    ready: bool,
    busy: bool,
    height_scale: f32,
    last_camera: [[f32; 6]; 2],
    serial: u32,
}

impl App {
    fn set_busy(&mut self, busy: bool) -> Result<()> {
        self.busy = busy;
        for id in [METHOD, SURFACE, ORIGINAL, TERRAIN] {
            self.window.set_control_enabled(id, !busy && self.ready)?;
        }
        self.window.set_control_enabled(RETRY, !busy)?;
        self.window.set_control_enabled(CANCEL, busy)?;
        self.window.set_control_value(PROGRESS, 0.0, "")?;
        Ok(())
    }

    fn status(&mut self, text: &str) -> Result<()> {
        self.window.set_status_text(text).map_err(Into::into)
    }

    fn failure(&mut self, message: &str) -> Result<()> {
        // Restore the last committed selection before re-enabling the controls.
        self.window
            .set_control_value(METHOD, 0.0, METHODS[self.committed.method - 1])?;
        self.window
            .set_control_value(SURFACE, f64::from(self.committed.percent), "")?;
        self.requested = self.committed;
        self.debounce = None;
        self.loading.clear();
        self.pending = None;
        self.set_busy(false)?;
        let cancelled = self.cancel.load(Ordering::Relaxed);
        self.status(if cancelled {
            "Loading cancelled. Previous complete views retained."
        } else {
            message
        })?;
        if !cancelled {
            self.window
                .append_log(&format!("<span style='color:#a33'>{message}</span>"))?;
        }
        Ok(())
    }

    /// Prepare a surface on a worker: the sample (first load only), grids and GeoTIFF.
    fn start_work(&mut self, selection: Selection, reload: bool) -> Result<()> {
        if self.busy || (!reload && (!self.ready || selection == self.committed)) {
            return Ok(());
        }
        self.cancel = Arc::new(AtomicBool::new(false));
        self.set_busy(true)?;
        self.serial += 1;
        let surface = self.temporary.join(format!("surface-{}.tif", self.serial));
        let (tx, rx) = mpsc::channel();
        let (cancel, options, grids) = (
            self.cancel.clone(),
            self.options.clone(),
            self.grids.clone(),
        );
        self.messages = Some(rx);
        self.worker = Some(thread::spawn(move || {
            let send = |text: String| {
                let _ = tx.send(Message::Status(text));
            };
            let result = (|| -> sample::Result<Work> {
                let (grids, original) = match (reload, grids) {
                    (false, Some(grids)) => {
                        send("Preparing the interpolated display surface...".into());
                        (grids, None)
                    }
                    _ => {
                        let assets = sample::Assets::zurich_v2();
                        let folder = sample::prepare(&assets, &options, &cancel, &send)?;
                        sample::check(&cancel)?;
                        gdal::configure_runtime(&folder, &set_env)?;
                        send(
                            "Reading numeric heights and preparing both native 3D scenes...".into(),
                        );
                        let grids = Arc::new(Grids::read(&folder, &cancel)?);
                        (grids, Some(folder.join("dsm.tif")))
                    }
                };
                sample::check(&cancel)?;
                let result = grids.surface_file(selection.method, selection.percent, &surface)?;
                Ok(Work {
                    grids,
                    selection,
                    original,
                    result,
                })
            })();
            let _ = tx.send(Message::Done(result));
        }));
        Ok(())
    }

    fn poll_work(&mut self) -> Result<()> {
        let Some(messages) = &self.messages else {
            return Ok(());
        };
        let mut done = None;
        let mut statuses = Vec::new();
        while let Ok(message) = messages.try_recv() {
            match message {
                Message::Status(text) => statuses.push(text),
                Message::Done(result) => done = Some(result),
            }
        }
        for text in statuses {
            self.status(&text)?;
        }
        let Some(result) = done else { return Ok(()) };
        self.messages = None;
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        match result {
            Err(error) => self.failure(&error),
            Ok(work) => {
                // Viewer3D prepares geometry on its own worker; the C API reports no percentage.
                let neutral = work.grids.folder.join("neutral_surface.tif");
                let mut started = || -> Result<()> {
                    self.loading.clear();
                    if let Some(original) = &work.original {
                        self.panes.viewers[0].load(original, &neutral, 500)?;
                        self.loading.push(0);
                    }
                    self.panes.viewers[1].load(&work.result, &neutral, 500)?;
                    self.loading.push(1);
                    Ok(())
                };
                if let Err(error) = started() {
                    return self.failure(&error.to_string());
                }
                self.pending = Some(work);
                self.status("Preparing native 3D terrain...")
            }
        }
    }

    fn poll_native(&mut self) -> Result<()> {
        if self.loading.is_empty() {
            return Ok(());
        }
        let mut states = Vec::new();
        for &index in &self.loading {
            match self.panes.viewers[index].poll() {
                Ok(state) => states.push(state),
                Err(error) => return self.failure(&error.to_string()),
            }
        }
        if states.contains(&0) {
            return Ok(());
        }
        self.loading.clear();
        if self.cancel.load(Ordering::Relaxed) || states.contains(&-2) {
            return self.failure("Loading cancelled. Previous complete views retained.");
        }
        let work = self.pending.take().ok_or("Missing prepared surface")?;
        let first = !self.ready;
        self.grids = Some(work.grids.clone());
        self.ready = true;
        self.committed = work.selection;
        for viewer in &self.panes.viewers {
            viewer.height(self.height_scale)?;
        }
        if first {
            self.reset_view()?;
        } else {
            self.sync_from(0)?;
        }
        self.panes.set_heading(1, &heading(self.committed));
        self.panes
            .layout(self.grids.as_deref(), self.committed.method)?;
        self.window.clear_log()?;
        self.window
            .append_log(&info(self.grids.as_deref(), self.committed))?;
        self.set_busy(false)?;
        self.status(READY)
    }

    fn cancel(&mut self) -> Result<()> {
        self.cancel.store(true, Ordering::Relaxed);
        for viewer in &self.panes.viewers {
            viewer.cancel();
        }
        self.status("Cancelling... Previous complete views are retained.")
    }

    // ----- cameras ----------------------------------------------------------------------
    fn reset_view(&mut self) -> Result<()> {
        let Some(grids) = self.grids.clone() else {
            return Ok(());
        };
        self.panes.viewers[0].reset()?;
        // Both scenes share the grid-centre origin at zero height: the crop centre at the mean
        // DSM height is (0, mean * scale * exaggeration, 0) in render units.
        let y = (grids.mean_dsm() * grids.render_scale() * f64::from(self.height_scale)) as f32;
        self.panes.viewers[0].move_camera(&[20.0, 48.0, 3.25, 0.0, y, 0.0])?;
        self.sync_from(0)
    }

    fn adjust(&mut self, change: impl Fn(&mut [f32; 6])) -> Result<()> {
        if !self.ready {
            return Ok(());
        }
        let mut camera = self.panes.viewers[0].camera()?;
        change(&mut camera);
        self.panes.viewers[0].move_camera(&camera)?;
        self.sync_from(0)
    }

    fn sync_from(&mut self, source: usize) -> Result<()> {
        // Same grid, same origin and (to within 1e-5) the same render scale: render-space
        // cameras transfer directly between the two native scenes.
        let camera = self.panes.viewers[source].camera()?;
        self.panes.viewers[1 - source].move_camera(&camera)?;
        self.last_camera = [
            self.panes.viewers[0].camera()?,
            self.panes.viewers[1].camera()?,
        ];
        Ok(())
    }

    fn poll_cameras(&mut self) -> Result<()> {
        // Pause while a scene is being replaced: its interim camera must not propagate.
        if !self.ready || self.busy {
            return Ok(());
        }
        for i in 0..2 {
            let camera = self.panes.viewers[i].camera()?;
            if camera
                .iter()
                .zip(&self.last_camera[i])
                .map(|(a, b)| (a - b).abs())
                .sum::<f32>()
                >= 1e-5
            {
                return self.sync_from(i);
            }
        }
        Ok(())
    }

    fn set_exaggeration(&mut self, value: f32) -> Result<()> {
        let old = self.height_scale;
        self.height_scale = value;
        for viewer in &self.panes.viewers {
            let camera = if self.ready {
                Some(viewer.camera()?)
            } else {
                None
            };
            viewer.height(value)?;
            if let Some(mut camera) = camera {
                camera[4] *= value / old;
                viewer.move_camera(&camera)?;
            }
        }
        if self.ready {
            self.sync_from(0)?;
        }
        Ok(())
    }

    fn handle(&mut self, event: geokernel::ControlEvent) -> Result<()> {
        match event.id {
            METHOD => {
                if let Some(index) = METHODS.iter().position(|m| *m == event.text) {
                    self.requested.method = index + 1;
                    self.debounce = None;
                    self.start_work(self.requested, false)?;
                }
            }
            SURFACE => {
                self.requested.percent = event.number.clamp(0.0, 100.0) as u32;
                if !self.busy {
                    self.status(&format!(
                        "Surface {} · rebuilding after a short pause",
                        percent_label(self.requested.percent)
                    ))?;
                }
                self.debounce = Some(Instant::now());
            }
            ORIGINAL if self.ready && !self.busy => {
                self.window.set_control_value(SURFACE, 0.0, "")?;
            }
            TERRAIN if self.ready && !self.busy => {
                self.window.set_control_value(SURFACE, 100.0, "")?;
            }
            EXAGGERATION if (0.25..=4.0).contains(&event.number) => {
                self.set_exaggeration(event.number as f32)?
            }
            RESET if self.ready => self.reset_view()?,
            TOP => self.adjust(|c| {
                c[0] = 0.0;
                c[1] = 85.0;
            })?,
            ZOOM_IN => self.adjust(|c| c[2] = (c[2] * 0.8).clamp(0.05, 20.0))?,
            ZOOM_OUT => self.adjust(|c| c[2] = (c[2] * 1.25).clamp(0.05, 20.0))?,
            PROFILE => {
                self.panes.set_chart_visible(event.text == "Shown");
            }
            RETRY => self.start_work(
                Selection {
                    method: self.requested.method,
                    percent: self.requested.percent,
                },
                true,
            )?,
            CANCEL if self.busy => self.cancel()?,
            FOLDER => {
                std::fs::create_dir_all(&self.options.data_root)?;
                std::process::Command::new("explorer.exe")
                    .arg(self.options.data_root.canonicalize()?)
                    .spawn()?;
            }
            _ => {}
        }
        Ok(())
    }
}

fn run(runtime: &Runtime) -> Result<()> {
    let options = Arc::new(sample::Options::parse()?);
    let mut window = unsafe {
        ViewerWindow::new(
            runtime,
            "DSM to DTM | Ground beneath buildings — GeoKernel",
            1520,
            960,
        )?
    };
    let controls = window.add_control_panel(&json!({
        "title": "Ground beneath buildings",
        "controls": [
            {"id": METHOD, "type": "combo", "label": "Terrain estimate", "options": METHODS, "value": METHODS[0]},
            {"id": SURFACE, "type": "slider", "label": "Surface (0% DSM · 100% DTM)", "minimum": 0, "maximum": 100, "value": 100},
            {"id": ORIGINAL, "type": "button", "text": "Original · 0%"},
            {"id": TERRAIN, "type": "button", "text": "Terrain · 100%"},
            {"id": EXAGGERATION, "type": "number", "label": "Vertical exaggeration (×)", "minimum": 0.25, "maximum": 4, "step": 0.25, "decimals": 2, "value": 1},
            {"id": RESET, "type": "button", "text": "Reset view"},
            {"id": TOP, "type": "button", "text": "Top view"},
            {"id": ZOOM_IN, "type": "button", "text": "Zoom in"},
            {"id": ZOOM_OUT, "type": "button", "text": "Zoom out"},
            {"id": PROFILE, "type": "combo", "label": "Profile chart", "options": ["Shown", "Hidden"], "value": "Shown"},
            {"id": RETRY, "type": "button", "text": "Retry / reload sample"},
            {"id": CANCEL, "type": "button", "text": "Cancel loading"},
            {"id": FOLDER, "type": "button", "text": "Data folder"},
            {"id": PROGRESS, "type": "progress", "label": "Working", "value": 0, "textVisible": false}
        ]
    }).to_string())?;
    window.add_log_panel("About this comparison")?;
    window.append_log(&info(
        None,
        Selection {
            method: 1,
            percent: 100,
        },
    ))?;
    window.show()?;
    window.process_events();
    let parent = window.viewer().get_native_handle()?;
    let bin = PathBuf::from(env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    // The panes (and their native viewers) are dropped before the owning window and Qt runtime.
    let panes = unsafe { Panes::new(parent, &bin.join("GeoKernel.Viewer3D.dll"))? };
    let temporary = env::temp_dir().join(format!("geokernel-dsm-to-dtm-{}", std::process::id()));
    std::fs::create_dir_all(&temporary)?;
    let initial = Selection {
        method: 1,
        percent: 100,
    };
    let mut app = App {
        window,
        panes,
        options,
        temporary: temporary.clone(),
        cancel: Arc::new(AtomicBool::new(true)),
        messages: None,
        worker: None,
        loading: Vec::new(),
        pending: None,
        grids: None,
        committed: initial,
        requested: initial,
        debounce: None,
        ready: false,
        busy: false,
        height_scale: 1.0,
        last_camera: [[0.0; 6]; 2],
        serial: 0,
    };
    app.panes.layout(None, 1)?;
    app.start_work(initial, true)?;
    let result = (|| -> Result<()> {
        while app.window.is_visible()? {
            app.window.process_events();
            if !app.window.is_visible()? {
                break;
            }
            app.panes
                .layout(app.grids.as_deref(), app.committed.method)?;
            while let Ok(event) = controls.try_recv() {
                if let Err(error) = app.handle(event) {
                    app.status(&error.to_string())?;
                }
            }
            if app
                .debounce
                .is_some_and(|t| t.elapsed() >= Duration::from_millis(200))
                && !app.busy
            {
                app.debounce = None;
                app.start_work(app.requested, false)?;
            }
            app.poll_work()?;
            app.poll_native()?;
            app.poll_cameras()?;
            thread::sleep(Duration::from_millis(16));
        }
        Ok(())
    })();
    // Closing during preparation: stop the worker, then let viewer destruction join native loads.
    app.cancel.store(true, Ordering::Relaxed);
    if let Some(worker) = app.worker.take() {
        let _ = worker.join();
    }
    drop(app.panes);
    drop(app.window);
    let _ = std::fs::remove_dir_all(&temporary);
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
