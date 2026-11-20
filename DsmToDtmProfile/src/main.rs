//! DSM to DTM: interactive elevation profile.
//!
//! Draw an A → B transect on two linked native GeoKernel 2D maps and compare the original
//! DSM, classical DTM, AI DTM and provider reference heights on one chart. Profiles sample
//! the numeric Float32 rasters; the maps show matching display reliefs. No inference runs
//! here; the reference DTM is evaluation-only.
mod dialog;
mod gdal;
mod map2d;
mod panes;
mod profile;
mod sample;

use gdal::Grids;
use geokernel::{Runtime, ViewerWindow};
use map2d::{Event, Extent};
use panes::{Panes, COLORS};
use profile::Profile;
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
    time::Duration,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const SURFACES: [&str; 3] = [
    "Classical DTM",
    "AI DTM · pix_grad",
    "Reference DTM · evaluation only",
];
const HEADINGS: [&str; 3] = [
    "CLASSICAL DTM · prepared estimate",
    "AI DTM · experimental pix_grad",
    "REFERENCE DTM · evaluation only",
];
const MODES: [&str; 2] = ["Compare elevations", "Compare errors against reference"];
const READY: &str = "Ready · draw A → B on either map; move over the chart to inspect heights.";
const FULL_EXTENT: Extent = Extent {
    x_min: 2677090.0,
    y_min: 1251340.0,
    x_max: 2677360.0,
    y_max: 1251610.0,
};
const PAN: i32 = 0;
const ZOOM_BOX: i32 = 1;

// Control panel IDs.
const DRAW: i32 = 1;
const EXAMPLE: i32 = 2;
const CLEAR: i32 = 3;
const SURFACE: i32 = 4;
const SPACING: i32 = 5;
const MODE: i32 = 6;
const PAN_TOOL: i32 = 7;
const BOX_TOOL: i32 = 8;
const ZOOM_IN: i32 = 9;
const ZOOM_OUT: i32 = 10;
const FULL: i32 = 11;
const EXPORT: i32 = 12;
const RETRY: i32 = 13;
const CANCEL: i32 = 14;
const PROGRESS: i32 = 15;

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

enum Message {
    Status(String, f64),
    Done(sample::Result<(Arc<Grids>, [PathBuf; 4])>),
}

fn marker(color: &str) -> String {
    json!({"pointColor": color, "pointSize": 18, "lineColor": "#ffffff", "lineWidth": 2})
        .to_string()
}

struct App {
    window: ViewerWindow,
    panes: Panes,
    events: mpsc::Receiver<Event>,
    options: Arc<sample::Options>,
    assets_sha256: String,
    temporary: PathBuf,
    cancel: Arc<AtomicBool>,
    messages: Option<mpsc::Receiver<Message>>,
    worker: Option<thread::JoinHandle<()>>,
    grids: Option<Arc<Grids>>,
    profile: Option<Profile>,
    revision: u64,
    draft: Option<[f64; 2]>,
    drawing: bool,
    errors: bool,
    surface: usize,
    spacing: f64,
    hovered: i64,
    presses: [Option<(i32, i32)>; 2],
    applied: [Option<Extent>; 2],
    instructions: String,
    log: String,
    busy: bool,
    generation: u32,
}

impl App {
    fn set_busy(&mut self, busy: bool) -> Result<()> {
        self.busy = busy;
        let ready = !busy && self.grids.is_some();
        for id in [
            DRAW, EXAMPLE, CLEAR, SURFACE, SPACING, PAN_TOOL, BOX_TOOL, ZOOM_IN, ZOOM_OUT, FULL,
        ] {
            self.window.set_control_enabled(id, ready)?;
        }
        self.window
            .set_control_enabled(EXPORT, !busy && self.profile.is_some())?;
        self.window.set_control_enabled(RETRY, !busy)?;
        self.window.set_control_enabled(CANCEL, busy)?;
        if !busy {
            self.window.set_control_value(PROGRESS, 0.0, "")?;
        }
        Ok(())
    }

    fn status(&mut self, text: &str) -> Result<()> {
        self.window.set_status_text(text).map_err(Into::into)
    }

    /// Instructions, line statistics, the hovered sample and the data notes in the log panel.
    fn refresh_log(&mut self) -> Result<()> {
        let stats = match &self.profile {
            None => "No transect selected.".to_owned(),
            Some(p) => format!(
                "<b>THIS TRANSECT ONLY</b><br>Length: {:.1} m · {} samples<br>Actual spacing: {:.3} m<br>\
                 Classical RMSE: {:.2} m · MAE {:.2} m<br>AI RMSE: {:.2} m · MAE {:.2} m<br>\
                 Lower error is better. These are line-sample errors, not full-area accuracy.",
                p.length, p.samples.len(), p.spacing, p.rmse[0], p.mae[0], p.rmse[1], p.mae[1]),
        };
        let cursor = match &self.profile {
            Some(p) if self.hovered >= 0 && (self.hovered as usize) < p.samples.len() => {
                let s = &p.samples[self.hovered as usize];
                format!(
                    "Distance: {:.1} m · E {:.2} · N {:.2}<br>DSM {:.2} m · Classical {:.2} m · AI {:.2} m · Reference {:.2} m",
                    s.distance, s.x, s.y, s.heights[0], s.heights[1], s.heights[2], s.heights[3])
            }
            _ => "Move over the chart to locate a sample on both maps.".to_owned(),
        };
        let legend = if self.errors {
            "Teal classical − reference · purple AI − reference · zero is closest"
        } else {
            "Gray DSM · teal classical · purple AI · dashed orange reference · map: A teal, B orange"
        };
        let log = format!(
            "<b>Compare along your own transect</b><br>Zürich · 250 × 250 m · 0.5 m height pixels · EPSG:2056<br><br>\
             <i>{}</i><br><br>{stats}<br><br>{cursor}<br><br>Chart: {legend}<br><br>\
             Profiles sample numeric Float32 heights with bilinear interpolation. Relief images are for display only. \
             Finer sampling cannot add source detail.<br><br>\
             Prepared independent results; no new inference. Reference is evaluation-only. Source LN02 heights, metres; \
             no vertical datum conversion. DSM/reference source years: 2018/2019. This is a fixed development scene.<br><br>\
             Source: Federal Office of Topography swisstopo · AI: David Perez, experimental pix_grad",
            self.instructions);
        if log != self.log {
            self.window.clear_log()?;
            self.window.append_log(&log)?;
            self.log = log;
        }
        Ok(())
    }

    // ----- loading ------------------------------------------------------------------------
    fn start(&mut self) -> Result<()> {
        if self.busy {
            return Ok(());
        }
        self.cancel = Arc::new(AtomicBool::new(false));
        self.set_busy(true)?;
        self.generation += 1;
        let folder = self.temporary.join(self.generation.to_string());
        let (tx, rx) = mpsc::channel();
        let (cancel, options) = (self.cancel.clone(), self.options.clone());
        self.messages = Some(rx);
        self.worker = Some(thread::spawn(move || {
            let result = (|| -> sample::Result<(Arc<Grids>, [PathBuf; 4])> {
                let status = |text: String| {
                    let _ = tx.send(Message::Status(text, 0.0));
                };
                let assets = sample::Assets::zurich_v2();
                let sample_folder = sample::prepare(&assets, &options, &cancel, &status)?;
                sample::check(&cancel)?;
                gdal::configure_runtime(&sample_folder, &set_env)?;
                std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                let message = "Reading numeric heights and preparing matching relief maps...";
                let _ = tx.send(Message::Status(message.into(), 0.0));
                let grids = Grids::read(&sample_folder, &cancel)?;
                let mut reliefs: [PathBuf; 4] = Default::default();
                for (band, path) in reliefs.iter_mut().enumerate() {
                    let gray = profile::relief(&grids, band, &cancel)?;
                    *path = gdal::write_rgb(
                        &folder.join(format!("relief-{band}.tif")),
                        grids.width,
                        grids.height,
                        &grids.transform,
                        &grids.projection,
                        &gray,
                    )?;
                    let _ = tx.send(Message::Status(message.into(), (band as f64 + 1.0) * 25.0));
                }
                Ok((Arc::new(grids), reliefs))
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
        let outcome = result
            .map_err(Into::into)
            .and_then(|(grids, reliefs)| self.publish(grids, &reliefs));
        self.set_busy(false)?;
        match outcome {
            Ok(()) => {
                self.example()?;
                self.status(READY)
            }
            Err(error) => {
                let cancelled = self.cancel.load(Ordering::Relaxed);
                let text = if cancelled {
                    "Loading cancelled.".to_owned()
                } else {
                    error.to_string()
                };
                self.status(&text)?;
                if !cancelled {
                    self.window
                        .append_log(&format!("<span style='color:#a33'>{error}</span>"))?;
                }
                Ok(())
            }
        }
    }

    /// DSM relief left; all three estimate reliefs right with one visible.
    fn publish(&mut self, grids: Arc<Grids>, reliefs: &[PathBuf; 4]) -> Result<()> {
        for (pane, map) in self.panes.maps.iter().enumerate() {
            map.clear_shapes();
            map.clear_layers();
            // The empty map adopts the first layer's CRS (EPSG:2056); new layers go to index 0.
            let bands: &[usize] = if pane == 0 { &[0] } else { &[1, 2, 3] };
            for &band in bands {
                map.add_layer(&reliefs[band], profile::SURFACE_NAMES[band])?;
            }
            if map.layer_count() != bands.len() as i32 {
                return Err("The relief layers were not added.".into());
            }
            map.set_tool(PAN);
        }
        self.grids = Some(grids);
        self.applied = [None, None];
        self.full_extent();
        self.select_surface(self.surface);
        Ok(())
    }

    fn cancel(&mut self) -> Result<()> {
        self.cancel.store(true, Ordering::Relaxed);
        self.status("Cancelling loading...")
    }

    // ----- maps ---------------------------------------------------------------------------
    fn full_extent(&mut self) {
        self.panes.maps[0].set_extent(FULL_EXTENT);
        self.sync_from(0);
    }

    fn sync_from(&mut self, side: usize) {
        let Some(extent) = self.panes.maps[side].extent() else {
            return;
        };
        self.applied[side] = Some(extent);
        self.panes.maps[1 - side].set_extent(extent);
        self.applied[1 - side] = self.panes.maps[1 - side].extent();
    }

    fn select_surface(&mut self, choice: usize) {
        self.surface = choice;
        if self.grids.is_none() {
            return;
        }
        // Bands 1, 2, 3 were prepended in that order: band b sits at index 3 - b.
        for band in 1..=3 {
            self.panes.maps[1].set_layer_visible(3 - band as i32, band == choice);
        }
        self.panes.maps[1].refresh();
        self.panes.set_heading(1, HEADINGS[choice - 1]);
    }

    fn handle_events(&mut self) -> Result<()> {
        for event in map2d::drain(&self.events) {
            if self.grids.is_none() {
                continue;
            }
            let side = event.side;
            match event.kind {
                map2d::VISIBLE_EXTENT_CHANGED => {
                    if self.panes.maps[side].extent() != self.applied[side] {
                        self.sync_from(side);
                    }
                }
                map2d::MAP_MOUSE_DOWN if event.button == 1 => {
                    self.presses[side] = Some(event.screen)
                }
                map2d::MAP_MOUSE_UP if event.button == 1 => {
                    // A click (not a pan drag) picks a point; dragging still pans.
                    if let Some(press) = self.presses[side].take() {
                        if (press.0 - event.screen.0).abs() + (press.1 - event.screen.1).abs() <= 3
                        {
                            self.pick([event.world.0, event.world.1])?;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Native owned shapes on both maps: white halo + teal line, A teal and B orange markers.
    /// No A/B text labels: the runtime removes them with deleteLater, which a host that pumps
    /// processEvents (rather than running a Qt event loop) never executes, so they reappear.
    fn draw_transect(&mut self) {
        for map in &self.panes.maps {
            map.clear_shapes();
            if let Some(a) = self.draft {
                map.add_point(a, &marker(COLORS[1]));
            } else if let Some(p) = &self.profile {
                let line = [p.start, p.end];
                map.add_line(
                    &line,
                    &json!({"lineColor": "#ffffff", "lineWidth": 5}).to_string(),
                );
                map.add_line(
                    &line,
                    &json!({"lineColor": COLORS[1], "lineWidth": 2.5}).to_string(),
                );
                map.add_point(p.start, &marker(COLORS[1]));
                map.add_point(p.end, &marker(COLORS[3]));
            }
            map.refresh(); // owned shapes are drawn on the next full map render
        }
        self.draw_cursor();
    }

    /// The chart's hovered sample as a lightweight overlay point (no map re-render). The point
    /// is parked far outside the map instead of removed: removal is deferred to a Qt event
    /// loop this host never runs, and a removed point would reappear on the next view change.
    fn draw_cursor(&self) {
        let style = json!({"pointColor": "#dd3470", "pointSize": 11, "lineColor": "#ffffff", "lineWidth": 2}).to_string();
        let point = match &self.profile {
            Some(p)
                if self.draft.is_none()
                    && self.hovered >= 0
                    && (self.hovered as usize) < p.samples.len() =>
            {
                let s = &p.samples[self.hovered as usize];
                [s.x, s.y]
            }
            _ => [-1.0e9, -1.0e9],
        };
        for map in &self.panes.maps {
            map.set_overlay_point("cursor", point, &style);
        }
    }

    // ----- transect -----------------------------------------------------------------------
    fn set_drawing(&mut self, on: bool) -> Result<()> {
        if self.drawing == on && self.draft.is_none() {
            return Ok(());
        }
        self.drawing = on;
        self.draft = None;
        self.instructions = if on {
            for map in &self.panes.maps {
                map.set_tool(PAN);
            }
            "Click A on either map. Dragging still pans; wheel zooms."
        } else {
            "Draw a new A → B transect, or inspect the current profile."
        }
        .into();
        self.draw_transect();
        self.status(if on {
            "Draw mode on · click A, then B on either map (press Draw again to cancel)."
        } else {
            READY
        })
    }

    fn pick(&mut self, point: [f64; 2]) -> Result<()> {
        let Some(grids) = self.grids.clone() else {
            return Ok(());
        };
        if self.busy || !self.drawing {
            return Ok(());
        }
        if profile::height(&grids, 0, point[0], point[1]).is_none() {
            return self.status("Click inside the valid raster pixel centres.");
        }
        let Some(a) = self.draft else {
            self.draft = Some(point);
            self.instructions =
                "A selected. Click B on either map. Toggle Draw off to cancel.".into();
            self.draw_transect();
            return Ok(());
        };
        self.draft = None;
        if let Err(error) = self.commit(a, point) {
            self.draft = Some(a);
            return self.status(&error.to_string());
        }
        self.drawing = false;
        self.instructions =
            "Your transect is ready. Hover over the chart to locate matching heights on both maps."
                .into();
        self.status(READY)
    }

    fn example(&mut self) -> Result<()> {
        let Some(grids) = self.grids.clone() else {
            return Ok(());
        };
        self.drawing = false;
        self.draft = None;
        let (a, b) = profile::example(&grids);
        self.commit(a, b)?;
        self.instructions = "Example transect loaded. Select Draw transect, then click A and B to compare your own line.".into();
        Ok(())
    }

    fn clear(&mut self) -> Result<()> {
        self.drawing = false;
        self.draft = None;
        self.profile = None;
        self.revision += 1;
        self.hovered = -1;
        self.window.set_control_enabled(EXPORT, false)?;
        self.draw_transect();
        Ok(())
    }

    fn commit(&mut self, a: [f64; 2], b: [f64; 2]) -> Result<()> {
        let grids = self.grids.clone().ok_or("No sample loaded")?;
        self.profile = Some(profile::make(&grids, a, b, self.spacing)?);
        self.revision += 1;
        self.hovered = -1;
        self.window.set_control_enabled(EXPORT, !self.busy)?;
        self.draw_transect();
        Ok(())
    }

    fn export(&mut self) -> Result<()> {
        let (Some(profile), Some(grids)) = (&self.profile, self.grids.clone()) else {
            return Ok(());
        };
        let owner = self.window.viewer().get_native_handle()?;
        let Some(path) = dialog::save_csv(owner)? else {
            return Ok(());
        };
        profile::export(
            std::path::Path::new(&path),
            profile,
            &grids,
            &self.assets_sha256,
        )?;
        self.status("Saved numeric profile CSV and metadata sidecar.")
    }

    fn handle(&mut self, event: geokernel::ControlEvent) -> Result<()> {
        let ready = self.grids.is_some() && !self.busy;
        match event.id {
            RETRY => self.start()?,
            CANCEL if self.busy => self.cancel()?,
            MODE => {
                self.errors = event.text == MODES[1];
            }
            // The native panel has no checkbox: the Draw button toggles drawing mode.
            DRAW if ready => self.set_drawing(!self.drawing)?,
            EXAMPLE if ready => self.example()?,
            CLEAR if ready => self.clear()?,
            SURFACE if ready => {
                if let Some(index) = SURFACES.iter().position(|s| *s == event.text) {
                    self.select_surface(index + 1);
                }
            }
            SPACING if ready && (0.5..=10.0).contains(&event.number) => {
                self.spacing = event.number;
                if let Some((a, b)) = self.profile.as_ref().map(|p| (p.start, p.end)) {
                    self.commit(a, b)?;
                }
            }
            PAN_TOOL | BOX_TOOL if ready => {
                self.set_drawing(false)?;
                for map in &self.panes.maps {
                    map.set_tool(if event.id == PAN_TOOL { PAN } else { ZOOM_BOX });
                }
            }
            ZOOM_IN | ZOOM_OUT if ready => self.panes.maps[0].zoom(event.id == ZOOM_IN),
            FULL if ready => self.full_extent(),
            EXPORT if ready => self.export()?,
            _ => {}
        }
        Ok(())
    }

    fn tick(&mut self) -> Result<()> {
        self.panes.layout()?;
        self.handle_events()?;
        self.poll_work()?;
        let hovered = self.panes.hovered(self.profile.as_ref());
        if hovered != self.hovered {
            self.hovered = hovered;
            self.draw_cursor();
        }
        self.panes.draw_chart(
            self.profile.as_ref(),
            self.revision,
            self.errors,
            self.hovered,
        );
        self.refresh_log()
    }
}

fn run(runtime: &Runtime) -> Result<()> {
    let options = Arc::new(sample::Options::parse()?);
    let assets = sample::Assets::zurich_v2();
    let mut window = unsafe {
        ViewerWindow::new(
            runtime,
            "DSM to DTM | Interactive elevation profile — GeoKernel",
            1460,
            940,
        )?
    };
    let controls = window.add_control_panel(&json!({
        "title": "Interactive elevation profile",
        "controls": [
            {"id": DRAW, "type": "button", "text": "Draw transect · A → B (toggle)"},
            {"id": EXAMPLE, "type": "button", "text": "Example transect"},
            {"id": CLEAR, "type": "button", "text": "Clear transect"},
            {"id": SURFACE, "type": "combo", "label": "Right-hand map", "options": SURFACES, "value": SURFACES[0]},
            {"id": SPACING, "type": "number", "label": "Sample spacing (m)", "minimum": 0.5, "maximum": 10, "step": 0.5, "decimals": 2, "value": 0.5},
            {"id": MODE, "type": "combo", "label": "Chart", "options": MODES, "value": MODES[0]},
            {"id": PAN_TOOL, "type": "button", "text": "Pan"},
            {"id": BOX_TOOL, "type": "button", "text": "Zoom box"},
            {"id": ZOOM_IN, "type": "button", "text": "Zoom in"},
            {"id": ZOOM_OUT, "type": "button", "text": "Zoom out"},
            {"id": FULL, "type": "button", "text": "Full extent"},
            {"id": EXPORT, "type": "button", "text": "Export profile CSV"},
            {"id": RETRY, "type": "button", "text": "Retry / reload sample"},
            {"id": CANCEL, "type": "button", "text": "Cancel loading"},
            {"id": PROGRESS, "type": "progress", "label": "Loading", "value": 0, "format": "%p%"}
        ]
    }).to_string())?;
    window.add_log_panel("This transect")?;
    window.show()?;
    window.process_events();
    let parent = window.viewer().get_native_handle()?;
    let bin = PathBuf::from(env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    let (events_tx, events) = mpsc::channel();
    // The panes (and their native maps) are dropped before the owning window and Qt runtime.
    let panes = unsafe { Panes::new(parent, &bin.join("GeoKernel.Viewer.dll"), events_tx)? };
    let temporary = env::temp_dir().join(format!("geokernel-dsm-profile-{}", std::process::id()));
    std::fs::create_dir_all(&temporary)?;
    let mut app = App {
        window,
        panes,
        events,
        options,
        assets_sha256: assets.sha256,
        temporary: temporary.clone(),
        cancel: Arc::new(AtomicBool::new(true)),
        messages: None,
        worker: None,
        grids: None,
        profile: None,
        revision: 0,
        draft: None,
        drawing: false,
        errors: false,
        surface: 1,
        spacing: 0.5,
        hovered: -1,
        presses: [None, None],
        applied: [None, None],
        instructions: "Preparing sample data...".into(),
        log: String::new(),
        busy: false,
        generation: 0,
    };
    app.start()?;
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
    // Closing during preparation: stop and join the worker before the maps are released.
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
