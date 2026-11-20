//! DSM to DTM: reference accuracy by area.
//!
//! Three linked native GeoKernel 2D maps keep the original DSM beside classical and AI error
//! maps on one shared colour scale. Select an area to compare numeric errors, their
//! distribution and individual source pixels. Scores use the numeric Float32 rasters; the maps
//! are display-only Byte RGB images. No inference runs here; the reference is evaluation-only.
mod accuracy;
mod dialog;
mod gdal;
mod map2d;
mod panes;
mod sample;

use accuracy::{Metric, Window, DISPLAY_LIMITS, PRESETS};
use gdal::Grids;
use geokernel::{Runtime, ViewerWindow};
use map2d::{Event, Extent};
use panes::{Panes, METHOD_COLORS};
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

const MODES: [&str; 3] = [
    "Signed reference errors",
    "Absolute reference errors",
    "Terrain estimates",
];
const RANGES: [&str; 4] = ["1 m", "2 m", "5 m", "10 m"];
const TITLES: [&str; 3] = [
    "ORIGINAL DSM · context",
    "CLASSICAL DTM · reference error",
    "AI DTM · reference error",
];
const HEADINGS: [[&str; 2]; 3] = [
    ["CLASSICAL DTM · signed error", "AI DTM · signed error"],
    ["CLASSICAL DTM · absolute error", "AI DTM · absolute error"],
    [
        "CLASSICAL DTM · terrain estimate",
        "AI DTM · experimental pix_grad",
    ],
];
const READY: &str = "Ready · select an evaluation area or click any source pixel.";
const IDLE: &str = "Select an area, choose a preset, or click a pixel to inspect errors.";
const SELECTING: &str =
    "Drag a box on any map. The same source pixels will be evaluated for both methods.";
const FULL_EXTENT: Extent = Extent {
    x_min: 2677090.0,
    y_min: 1251340.0,
    x_max: 2677360.0,
    y_max: 1251610.0,
};
const PARKED: [f64; 2] = [-1.0e9, -1.0e9];
const PAN: i32 = 0;
const ZOOM_BOX: i32 = 1;
const SELECT: i32 = 3;

// Control panel IDs.
const SELECT_AREA: i32 = 1;
const CURRENT: i32 = 2;
const FULL_SAMPLE: i32 = 3;
const MODE: i32 = 4;
const RANGE: i32 = 5;
const THRESHOLD: i32 = 6;
const PRESET: i32 = 7;
const PAN_TOOL: i32 = 8;
const BOX_TOOL: i32 = 9;
const ZOOM_IN: i32 = 10;
const ZOOM_OUT: i32 = 11;
const FULL: i32 = 12;
const EXPORT: i32 = 13;
const RETRY: i32 = 14;
const CANCEL: i32 = 15;
const PROGRESS: i32 = 16;

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
    Done(sample::Result<(Arc<Grids>, accuracy::Displays)>),
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
    area: Option<Window>,
    area_name: String,
    metrics: Option<[Metric; 2]>,
    revision: u64,
    probe: Option<(usize, usize)>,
    mode: usize,
    range: usize,
    threshold: f64,
    preset: usize,
    selecting: bool,
    presses: [Option<(i32, i32)>; 3],
    applied: [Option<Extent>; 3],
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
            SELECT_AREA,
            CURRENT,
            FULL_SAMPLE,
            MODE,
            THRESHOLD,
            PRESET,
            PAN_TOOL,
            BOX_TOOL,
            ZOOM_IN,
            ZOOM_OUT,
            FULL,
            EXPORT,
        ] {
            self.window.set_control_enabled(id, ready)?;
        }
        self.window
            .set_control_enabled(RANGE, ready && self.mode != 2)?;
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

    /// Instructions, the lower-error verdict, the metric table, the probed pixel and notes.
    fn refresh_log(&mut self) -> Result<()> {
        let table = match &self.metrics {
            None => "Compare lower errors for each metric.".to_owned(),
            Some(m) => {
                let rank = |c: f64, a: f64| {
                    if (c - a).abs() <= 1e-9 {
                        "Tie"
                    } else if c < a {
                        "Classical"
                    } else {
                        "AI · pix_grad"
                    }
                };
                // Metrics as rows, methods as columns: fits the narrow log panel.
                let cell = |i: usize, v: String| {
                    format!(
                        "<td align='right' style='color:{}'>{v}</td>",
                        METHOD_COLORS[i]
                    )
                };
                let row = |name: String, f: &dyn Fn(&Metric) -> String| {
                    format!(
                        "<tr><td>{name}</td>{}{}</tr>",
                        cell(0, f(&m[0])),
                        cell(1, f(&m[1]))
                    )
                };
                let rows = [
                    row("RMSE (m)".into(), &|x| format!("{:.3}", x.rmse)),
                    row("MAE (m)".into(), &|x| format!("{:.3}", x.mae)),
                    row("Bias (m)".into(), &|x| format!("{:.3}", x.bias)),
                    row("P95 |error| (m)".into(), &|x| format!("{:.3}", x.p95)),
                    row(format!("|error| &gt; {} m (%)", self.threshold), &|x| {
                        format!("{:.2}", x.above_percent())
                    }),
                ]
                .concat();
                format!(
                    "<b>THIS AREA ONLY</b><br>Lower RMSE: <b>{}</b> · Lower MAE: <b>{}</b><br>Different metrics can rank methods differently. No overall winner is implied.<br><br>\
                     <table cellpadding='3' border='1' style='border-collapse:collapse'><tr><th></th><th style='color:{}'>Classical</th><th style='color:{}'>AI · pix_grad</th></tr>{rows}</table><br>\
                     {} · {} common valid reference pixels · evaluated cell footprint {:.2} m² · reference coverage 100% · bias is estimate − reference",
                    rank(m[0].rmse, m[1].rmse), rank(m[0].mae, m[1].mae), METHOD_COLORS[0], METHOD_COLORS[1], self.area_name, m[0].count, m[0].count as f64 * 0.25)
            }
        };
        let probe = match (&self.grids, self.probe) {
            (Some(g), Some((x, y))) => {
                let i = y * g.width + x;
                let z = |b: usize| f64::from(g.z[b][i]);
                let outside = if self.area.is_some_and(|a| a.contains(x, y)) {
                    ""
                } else {
                    " · outside evaluation area"
                };
                format!(
                    "<b>SOURCE PIXEL</b> · column {x} / row {y}{outside}<br>DSM {:.2} m · Reference {:.2} m<br>Classical {:.2} m · error {:.3} m<br>AI {:.2} m · error {:.3} m",
                    z(0), z(3), z(1), z(1) - z(3), z(2), z(2) - z(3))
            }
            _ => "Click a map pixel to inspect its heights and signed errors.".to_owned(),
        };
        let log = format!(
            "<b>Where does each method work better?</b><br>Zürich · same DSM and reference · 250 × 250 m · 0.5 m source cells<br><br>\
             <i>{}</i><br><br>{table}<br><br>{probe}<br><br>\
             One shared reference and source grid. Geographic presets are not land-cover classes. Dimmed map pixels are outside the evaluated area.<br><br>\
             Prepared independent results; reference is evaluation-only. Source heights: LN02 metres, no vertical datum conversion. \
             Source filename years: DSM 2018 / reference 2019. Fixed development crop; these are not held-out benchmark results.<br><br>\
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
        self.set_selecting(false)?;
        self.set_busy(true)?;
        self.generation += 1;
        let folder = self.temporary.join(self.generation.to_string());
        let (tx, rx) = mpsc::channel();
        let (cancel, options) = (self.cancel.clone(), self.options.clone());
        self.messages = Some(rx);
        self.worker = Some(thread::spawn(move || {
            let result = (|| -> sample::Result<(Arc<Grids>, accuracy::Displays)> {
                let status = |text: String| {
                    let _ = tx.send(Message::Status(text, 0.0));
                };
                let assets = sample::Assets::zurich_v2();
                let sample_folder = sample::prepare(&assets, &options, &cancel, &status)?;
                sample::check(&cancel)?;
                gdal::configure_runtime(&sample_folder, &set_env)?;
                std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
                let message = "Reading heights and preparing shared-scale Byte RGB maps...";
                let _ = tx.send(Message::Status(message.into(), 0.0));
                let grids = Grids::read(&sample_folder, &cancel)?;
                let displays = accuracy::prepare_displays(&grids, &folder, &cancel, &|percent| {
                    let _ = tx.send(Message::Status(message.into(), percent));
                })?;
                Ok((Arc::new(grids), displays))
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
            .and_then(|(grids, displays)| self.publish(grids, &displays));
        self.set_busy(false)?;
        match outcome {
            Ok(()) => {
                if self.area.is_some() {
                    self.update_metrics()?;
                    self.draw_area();
                } else {
                    self.apply_preset(0)?;
                }
                self.instructions = IDLE.into();
                self.status(READY)
            }
            Err(error) => {
                let cancelled = self.cancel.load(Ordering::Relaxed);
                let text = if cancelled {
                    "Preparation cancelled; completed comparison retained.".to_owned()
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

    fn publish(&mut self, grids: Arc<Grids>, displays: &accuracy::Displays) -> Result<()> {
        for (pane, map) in self.panes.maps.iter().enumerate() {
            map.clear_shapes();
            map.clear_layers();
            // The empty map adopts the first layer's CRS (EPSG:2056); new layers go to index 0.
            let files: Vec<&PathBuf> = if pane == 0 {
                vec![&displays.original]
            } else {
                displays.methods[pane - 1].iter().collect()
            };
            for (k, file) in files.iter().enumerate() {
                let name = if pane == 0 {
                    "Original DSM".to_owned()
                } else {
                    format!("{} display {k}", if pane == 1 { "Classical" } else { "AI" })
                };
                map.add_layer(file, &name)?;
            }
            if map.layer_count() != files.len() as i32 {
                return Err("The display layers were not added.".into());
            }
            map.set_tool(PAN);
        }
        self.grids = Some(grids);
        self.applied = [None; 3];
        self.full_extent();
        self.select_display();
        Ok(())
    }

    fn cancel(&mut self) -> Result<()> {
        self.cancel.store(true, Ordering::Relaxed);
        self.status("Cancelling preparation...")
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
        for other in (0..3).filter(|o| *o != side) {
            self.panes.maps[other].set_extent(extent);
            self.applied[other] = self.panes.maps[other].extent();
        }
    }

    fn set_selecting(&mut self, on: bool) -> Result<()> {
        self.selecting = on;
        for map in &self.panes.maps {
            map.set_tool(if on { SELECT } else { PAN });
        }
        self.instructions = if on { SELECTING } else { IDLE }.into();
        if self.grids.is_some() && !self.busy {
            self.status(if on {
                "Area selection on · drag a box on any map (press Select again to cancel)."
            } else {
                READY
            })?;
        }
        Ok(())
    }

    fn select_display(&mut self) {
        if self.grids.is_none() {
            return;
        }
        let selected = if self.mode == 2 {
            8
        } else {
            (if self.mode == 1 { 4 } else { 0 }) + self.range
        };
        for m in 1..3 {
            for k in 0..9 {
                self.panes.maps[m].set_layer_visible(8 - k as i32, k == selected);
                // files 0..8 were prepended in order
            }
            self.panes.maps[m].refresh();
            self.panes.set_heading(m, HEADINGS[self.mode][m - 1]);
        }
    }

    /// Dim everything outside the evaluated cells (four translucent bands) and outline them.
    fn draw_area(&mut self) {
        let (Some(g), Some(area)) = (&self.grids, self.area) else {
            return;
        };
        let [x0, y0, x1, y1] = accuracy::window_extent(g, area);
        let t = &g.transform;
        let margin = 2000.0;
        let (ox0, oy0) = (t[0] - margin, t[3] + g.height as f64 * t[5] - margin);
        let (ox1, oy1) = (t[0] + g.width as f64 * t[1] + margin, t[3] + margin);
        let bands = [
            [ox0, y1, ox1, oy1],
            [ox0, oy0, ox1, y0],
            [ox0, y0, x0, y1],
            [x1, y0, ox1, y1],
        ];
        let dim = json!({"fillColor": "#ffffff", "fillOpacity": 100, "lineColor": "#ffffff", "lineWidth": 0}).to_string();
        let outline = json!({"lineColor": "#167d8d", "lineWidth": 2}).to_string();
        for map in &self.panes.maps {
            map.clear_shapes();
            for [a, b, c, d] in bands {
                if c > a && d > b {
                    map.add_polygon(&[[a, b], [c, b], [c, d], [a, d], [a, b]], &dim);
                }
            }
            map.add_line(
                &[[x0, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0]],
                &outline,
            );
            map.refresh(); // owned shapes are drawn on the next full map render
        }
    }

    /// The probed cell as an overlay point on every map. It is moved, never removed: removal is
    /// deferred to a Qt event loop this host never runs, so a removed point would reappear.
    fn draw_probe(&self) {
        let point = match (&self.grids, self.probe) {
            (Some(g), Some((x, y))) => [
                g.transform[0] + (x as f64 + 0.5) * 0.5,
                g.transform[3] - (y as f64 + 0.5) * 0.5,
            ],
            _ => PARKED,
        };
        let style = json!({"pointColor": "#dc3672", "pointSize": 11, "lineColor": "#ffffff", "lineWidth": 2}).to_string();
        for map in &self.panes.maps {
            map.set_overlay_point("probe", point, &style);
        }
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
                map2d::MAP_SELECTION_BOX_FINISHED if self.selecting => {
                    let e = event.extent;
                    self.choose_area(
                        [e.x_min, e.y_min, e.x_max, e.y_max],
                        "User-selected area",
                        None,
                    )?;
                    self.set_selecting(false)?;
                }
                map2d::MAP_MOUSE_DOWN if event.button == 1 => {
                    self.presses[side] = Some(event.screen)
                }
                map2d::MAP_MOUSE_UP if event.button == 1 => {
                    if let Some(press) = self.presses[side].take() {
                        if !self.selecting
                            && (press.0 - event.screen.0).abs() + (press.1 - event.screen.1).abs()
                                <= 3
                        {
                            self.inspect(event.world.0, event.world.1)?;
                        }
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    // ----- evaluation ---------------------------------------------------------------------
    fn choose_area(&mut self, extent: [f64; 4], name: &str, preset: Option<usize>) -> Result<()> {
        let Some(g) = self.grids.clone() else {
            return Ok(());
        };
        match accuracy::pixel_window(&g, extent) {
            Err(error) => return self.status(&error),
            Ok(window) => self.area = Some(window),
        }
        self.area_name = name.into();
        self.preset = preset.unwrap_or(4);
        self.window
            .set_control_value(PRESET, 0.0, PRESETS[self.preset])?;
        self.update_metrics()?;
        self.draw_area();
        self.status("Updated both methods on exactly the same source cells.")
    }

    fn apply_preset(&mut self, index: usize) -> Result<()> {
        let Some(g) = self.grids.clone() else {
            return Ok(());
        };
        if index == 4 {
            self.preset = 4;
            return self.set_selecting(true);
        }
        self.set_selecting(false)?;
        self.choose_area(
            accuracy::preset_area(&g, index),
            PRESETS[index],
            Some(index),
        )
    }

    fn update_metrics(&mut self) -> Result<()> {
        let (Some(g), Some(area)) = (&self.grids, self.area) else {
            return Ok(());
        };
        self.metrics = Some(accuracy::measure(g, area, self.threshold)?);
        self.revision += 1;
        Ok(())
    }

    fn inspect(&mut self, x: f64, y: f64) -> Result<()> {
        let Some(g) = &self.grids else { return Ok(()) };
        if self.busy {
            return Ok(());
        }
        let (col, row) = ((x - g.transform[0]) / 0.5, (g.transform[3] - y) / 0.5);
        if !(col >= 0.0 && row >= 0.0 && col < g.width as f64 && row < g.height as f64) {
            return self.status("Click inside the source raster.");
        }
        self.probe = Some((col as usize, row as usize));
        self.draw_probe();
        Ok(())
    }

    fn export(&mut self) -> Result<()> {
        let (Some(g), Some(area), Some(metrics)) = (self.grids.clone(), self.area, &self.metrics)
        else {
            return Ok(());
        };
        let owner = self.window.viewer().get_native_handle()?;
        let Some(path) = dialog::save_json(owner)? else {
            return Ok(());
        };
        accuracy::export_report(
            std::path::Path::new(&path),
            &g,
            area,
            &self.area_name,
            self.threshold,
            metrics,
            &self.assets_sha256,
        )?;
        self.status("Saved selected-area metrics and source provenance.")
    }

    fn handle(&mut self, event: geokernel::ControlEvent) -> Result<()> {
        let ready = self.grids.is_some() && !self.busy;
        match event.id {
            RETRY => self.start()?,
            CANCEL if self.busy => self.cancel()?,
            MODE => {
                if let Some(i) = MODES.iter().position(|m| *m == event.text) {
                    self.mode = i;
                    self.window.set_control_enabled(RANGE, ready && i != 2)?;
                    self.select_display();
                }
            }
            RANGE => {
                if let Some(i) = RANGES.iter().position(|r| *r == event.text) {
                    self.range = i;
                    self.select_display();
                }
            }
            THRESHOLD if ready && (0.1..=20.0).contains(&event.number) => {
                self.threshold = event.number;
                self.update_metrics()?;
            }
            // Programmatic preset updates echo back as events; only a new choice acts.
            PRESET if ready => {
                if let Some(i) = PRESETS.iter().position(|p| *p == event.text) {
                    if i != self.preset {
                        self.apply_preset(i)?;
                    }
                }
            }
            // The native panel has no checkbox: the Select button toggles area selection.
            SELECT_AREA if ready => self.set_selecting(!self.selecting)?,
            CURRENT if ready => {
                if let Some(e) = self.panes.maps[0].extent() {
                    self.choose_area(
                        [e.x_min, e.y_min, e.x_max, e.y_max],
                        "Current map view",
                        None,
                    )?;
                }
            }
            FULL_SAMPLE if ready => self.apply_preset(0)?,
            PAN_TOOL | BOX_TOOL if ready => {
                self.set_selecting(false)?;
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
        self.panes.draw_panel(
            self.mode,
            DISPLAY_LIMITS[self.range],
            self.metrics.as_ref(),
            self.revision,
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
            "DSM to DTM | Where is each method more accurate? — GeoKernel",
            1720,
            970,
        )?
    };
    let controls = window.add_control_panel(&json!({
        "title": "Reference accuracy by area",
        "controls": [
            {"id": SELECT_AREA, "type": "button", "text": "Select evaluation area (toggle)"},
            {"id": CURRENT, "type": "button", "text": "Evaluate current view"},
            {"id": FULL_SAMPLE, "type": "button", "text": "Full sample"},
            {"id": MODE, "type": "combo", "label": "Map comparison", "options": MODES, "value": MODES[0]},
            {"id": RANGE, "type": "combo", "label": "Display limit", "options": RANGES, "value": RANGES[1]},
            {"id": THRESHOLD, "type": "number", "label": "Large error > (m)", "minimum": 0.1, "maximum": 20, "step": 0.5, "decimals": 1, "value": 2},
            {"id": PRESET, "type": "combo", "label": "Study-area preset", "options": PRESETS, "value": PRESETS[0]},
            {"id": PAN_TOOL, "type": "button", "text": "Pan"},
            {"id": BOX_TOOL, "type": "button", "text": "Zoom box"},
            {"id": ZOOM_IN, "type": "button", "text": "Zoom in"},
            {"id": ZOOM_OUT, "type": "button", "text": "Zoom out"},
            {"id": FULL, "type": "button", "text": "Full extent"},
            {"id": EXPORT, "type": "button", "text": "Export area report"},
            {"id": RETRY, "type": "button", "text": "Retry / reload sample"},
            {"id": CANCEL, "type": "button", "text": "Cancel loading"},
            {"id": PROGRESS, "type": "progress", "label": "Loading", "value": 0, "format": "%p%"}
        ]
    }).to_string())?;
    window.add_log_panel("This area")?;
    window.show()?;
    window.process_events();
    let parent = window.viewer().get_native_handle()?;
    let bin = PathBuf::from(env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    let (events_tx, events) = mpsc::channel();
    // The panes (and their native maps) are dropped before the owning window and Qt runtime.
    let panes =
        unsafe { Panes::new(parent, &bin.join("GeoKernel.Viewer.dll"), events_tx, TITLES)? };
    let temporary = env::temp_dir().join(format!("geokernel-dsm-accuracy-{}", std::process::id()));
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
        area: None,
        area_name: String::new(),
        metrics: None,
        revision: 0,
        probe: None,
        mode: 0,
        range: 1,
        threshold: 2.0,
        preset: 0,
        selecting: false,
        presses: [None; 3],
        applied: [None; 3],
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
