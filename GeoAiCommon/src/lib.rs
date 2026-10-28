pub mod assets;
mod dialog;
pub mod profiles;
pub mod raster;
pub mod runtime;
mod sam;

use geokernel::{AiTask, AiTasks, ControlEvent, Runtime, ViewerEvent, ViewerTool, ViewerWindow};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub const PROVIDER: i32 = 1;
pub const OPACITY: i32 = 2;
pub const RUN: i32 = 3;
pub const SAVE: i32 = 4;
pub const CANCEL: i32 = 5;
const PROGRESS: i32 = 6;
const STAGE: i32 = 7;
pub const THRESHOLD: i32 = 8;

enum WorkDone {
    Input(PathBuf),
    Preview(Value, PathBuf),
    Started(AiTask, PathBuf),
    Exported(AiTask, std::result::Result<(), String>),
    Released(std::result::Result<(), String>),
}

pub type DynamicWorldExample = GeoAiExample;

pub struct GeoAiExample {
    pub window: ViewerWindow,
    pub result: Option<Value>,
    pub raster: Option<PathBuf>,
    pub closed: bool,
    pub provider: String,
    pub opacity: f64,
    pub threshold: f64,
    profile: profiles::Profile,
    pub sam: sam::SamState,
    tasks: AiTasks,
    job: Option<AiTask>,
    worker: Option<JoinHandle<std::result::Result<WorkDone, String>>>,
    controls_rx: Receiver<ControlEvent>,
    events: Receiver<ViewerEvent>,
    progress_rx: Receiver<(i32, String)>,
    progress_tx: Sender<(i32, String)>,
    cancellation: Arc<AtomicBool>,
    running: bool,
    closing: bool,
    exporting: bool,
    overlay: bool,
    render_busy: bool,
    release_since: Option<Instant>,
    run_after_release: bool,
}

impl GeoAiExample {
    pub fn new(runtime: &Runtime) -> Result<Self> {
        Self::with_profile(runtime, profiles::DYNAMIC_WORLD)
    }
    pub fn with_profile(runtime: &Runtime, profile: profiles::Profile) -> Result<Self> {
        let tasks = runtime.ai_tasks()?;
        let mut window = unsafe { ViewerWindow::new(runtime, profile.title, 1280, 850)? };
        window.add_navigation_toolbar();
        let initial_opacity = if profile.buildings { 100.0 } else { 60.0 };
        let mut panel = json!({"title":profile.title,"area":"right","width":390,"controls":[
            {"id":PROVIDER,"type":"combo","label":"Execution provider","options":["Auto","CPU","CUDA"],"value":"Auto"},
            {"id":OPACITY,"type":"slider","label":"Output opacity (right = opaque)","minimum":0,"maximum":100,"value":initial_opacity},
            {"id":RUN,"type":"button","text":profile.run_label},
            {"id":SAVE,"type":"button","text":"Save Shapefile"},
            {"id":CANCEL,"type":"button","text":"Cancel inference"},
            {"id":PROGRESS,"type":"progress","value":0,"format":"%p%"},
            {"id":STAGE,"type":"text","label":"Status","value":"Preparing source…"}
        ]});
        if profile.buildings {
            panel["controls"].as_array_mut().unwrap().insert(1,json!({"id":THRESHOLD,"type":"number","label":profile.threshold_label,"minimum":0.01,"maximum":0.99,"step":0.05,"decimals":2,"value":0.50}));
        }
        if profile.task.starts_with("sam3-") {
            panel["controls"]
                .as_array_mut()
                .unwrap()
                .extend(sam::controls());
        }
        if profile.task == "sen2sr-super-resolution" {
            panel["controls"][3]["text"] = json!("Save GeoTIFF");
        }
        let controls_rx = window.add_control_panel(&panel.to_string())?;
        window.add_log_panel("Inference diagnostics / classes")?;
        window.viewer().use_tool(ViewerTool::Pan);
        let events = window.subscribe_events();
        let (progress_tx, progress_rx) = mpsc::channel();
        let cancellation = Arc::new(AtomicBool::new(false));
        let cancel = cancellation.clone();
        let progress = progress_tx.clone();
        let worker = thread::spawn(move || {
            let input = assets::prepare(&profile.data, &cancel, &progress)?;
            raster::prepare(profile, &input, &cancel, &progress)?;
            Ok(WorkDone::Input(input))
        });
        let mut example = Self {
            window,
            result: None,
            raster: None,
            closed: false,
            provider: "auto".into(),
            opacity: initial_opacity,
            threshold: 0.50,
            profile,
            sam: sam::SamState::default(),
            tasks,
            job: None,
            worker: Some(worker),
            controls_rx,
            events,
            progress_rx,
            progress_tx,
            cancellation,
            running: false,
            closing: false,
            exporting: false,
            overlay: false,
            render_busy: false,
            release_since: None,
            run_after_release: false,
        };
        example.controls()?;
        Ok(example)
    }
    pub fn busy(&self) -> bool {
        self.worker.is_some() || self.running || self.release_since.is_some()
    }
    pub fn job_status(&self) -> Result<Option<Value>> {
        Ok(self.job.as_ref().map(|job| job.status()).transpose()?)
    }
    fn progress(&mut self, value: i32, message: &str) -> Result<()> {
        let value = value.clamp(0, 100);
        let message = message.replace("%p%", &format!("{value}%"));
        self.window
            .set_control_value(PROGRESS, value as f64, &format!("{value}%"))?;
        self.window.set_control_value(STAGE, 0.0, &message)?;
        self.window.set_status_text(&message)?;
        Ok(())
    }
    fn log(&mut self, message: &str) -> Result<()> {
        self.window.clear_log()?;
        self.window.append_log(
            &message
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('\n', "<br>"),
        )?;
        Ok(())
    }
    fn controls(&mut self) -> Result<()> {
        let ready = !self.busy() && !self.closing;
        self.window.set_control_enabled(RUN, ready)?;
        self.window.set_control_enabled(PROVIDER, ready)?;
        if self.profile.buildings {
            self.window.set_control_enabled(THRESHOLD, ready)?;
        }
        self.window
            .set_control_enabled(SAVE, ready && self.result.is_some())?;
        self.window.set_control_enabled(
            CANCEL,
            self.busy() && !self.exporting && self.release_since.is_none() && !self.closing,
        )?;
        self.window
            .set_control_enabled(OPACITY, self.overlay && !self.closing)?;
        if self.is_sam() {
            self.sam_controls()?;
        }
        Ok(())
    }
    fn source(&mut self, raster: PathBuf) -> Result<()> {
        self.raster = Some(raster.clone());
        if self.is_sam() {
            self.window.viewer().set_coordinate_system_preset("wgs84")?;
        }
        self.window.viewer().clear_layers()?;
        let display = raster.with_file_name(self.profile.display_name);
        if !self
            .window
            .viewer()
            .add_layer_file(&display.to_string_lossy())?
        {
            return Err("Cannot display RGB source".into());
        }
        self.window
            .viewer()
            .set_layer_name(0, self.profile.source_name)?;
        if self.is_sam() {
            self.window.viewer().set_coordinate_system_preset("wgs84")?;
        }
        self.window.viewer().full_extent()?;
        Ok(())
    }
    pub fn run(&mut self) -> Result<()> {
        if self.busy() || self.closing {
            return Ok(());
        }
        if self.is_sam() {
            if self.result.is_some() {
                return Ok(());
            }
            if !self.sam_valid()? {
                return Ok(());
            }
        }
        self.cancellation.store(false, Ordering::Relaxed);
        self.result = None;
        self.detach_result()?;
        if self.job.is_some() {
            self.run_after_release = true;
            self.release_since = Some(Instant::now());
        } else {
            self.start_run();
        }
        self.controls()
    }
    fn start_run(&mut self) {
        self.cancellation.store(false, Ordering::Relaxed);
        let cancel = self.cancellation.clone();
        let progress = self.progress_tx.clone();
        let tasks = self.tasks.clone();
        let raster = self.raster.clone();
        let provider = self.provider.clone();
        let profile = if self.is_sam() && !self.sam.text_mode {
            profiles::SAM_TRACKER
        } else {
            self.profile
        };
        let sam = self.sam.clone();
        let threshold = self.threshold;
        self.worker = Some(thread::spawn(move || {
            let raster = match raster {
                Some(value) => value,
                None => assets::prepare(&profile.data, &cancel, &progress)?,
            };
            raster::prepare(profile, &raster, &cancel, &progress)?;
            let manifest = assets::prepare(&profile.model, &cancel, &progress)?;
            assets::check(&cancel)?;
            let mut request = json!({"task":profile.task,"modelPackagePath":manifest.parent(),"rasterPath":raster,
                "provider":provider,"intraOpThreads":4,"maximumMilliseconds":600000});
            if profile.buildings {
                request["threshold"] = json!(threshold);
            } else if profile.task == "dynamic-world" {
                request["validityMaskPath"] =
                    json!(raster.with_file_name("dynamicworld_spain_valid_mask.tif"));
            }
            if profile.task == "sam3-text" {
                request["text"] = json!(sam.text.trim());
                request["scoreThreshold"] = json!(sam.score);
                request["maximumMilliseconds"] = json!(1800000);
            } else if profile.task == "sam3-tracker" {
                let prompts = raster::prompts(&raster, &sam.points, sam.bounds)?;
                request
                    .as_object_mut()
                    .unwrap()
                    .extend(prompts.as_object().unwrap().clone());
                request["maximumMilliseconds"] = json!(1800000);
            }
            assets::check(&cancel)?;
            let job = tasks.start(&request).map_err(|e| e.to_string())?;
            Ok(WorkDone::Started(job, raster))
        }));
    }
    fn detach_result(&mut self) -> Result<()> {
        if self.overlay {
            if self.profile.task == "sen2sr-super-resolution" {
                self.window.viewer().clear_layers()?;
                if let Some(raster) = &self.raster {
                    let source = raster.with_file_name(self.profile.display_name);
                    if !self
                        .window
                        .viewer()
                        .add_layer_file(&source.to_string_lossy())?
                    {
                        return Err("Cannot restore source preview".into());
                    }
                }
            } else {
                self.window.viewer().remove_layer(0)?;
            }
            self.overlay = false;
        }
        Ok(())
    }

    fn show_completed(&mut self, result: Value, display_path: Option<String>) -> Result<()> {
        if self.profile.task == "sen2sr-super-resolution" {
            self.window.viewer().clear_layers()?;
            let source = result["sourcePreviewPath"]
                .as_str()
                .ok_or("Missing comparison source")?;
            if !self.window.viewer().add_layer_file(source)? {
                return Err("Cannot display comparison source".into());
            }
            self.window
                .viewer()
                .set_layer_name(0, "Source — shared contrast")?;
        }
        let path = result[if self.profile.buildings || self.is_sam() {
            "vectorPath"
        } else {
            "previewPath"
        }]
        .as_str()
        .ok_or("Result preview is missing")?;
        let path = display_path.as_deref().unwrap_or(path);
        if !self.window.viewer().add_layer_file(path)? {
            return Err("Cannot display inference result".into());
        }
        self.overlay = true;
        self.window.viewer().set_layer_name(0, self.profile.title)?;
        if self.profile.buildings || self.is_sam() {
            let style = json!({
                "fillColor": "#ef5350",
                "fillOpacity": if self.profile.buildings { 255 } else { 110 },
                "lineColor": "#ffdd55",
                "lineWidth": 1.5
            });
            if !self
                .window
                .viewer()
                .set_layer_style_json(0, &style.to_string())?
            {
                return Err("Cannot style building footprints".into());
            }
        }
        self.apply_opacity(self.opacity)?;
        let interpretation = if self.profile.buildings {
            format!(
                "Instances: {} · threshold: {:.2}",
                result["instances"], self.threshold
            )
        } else if self.is_sam() {
            self.sam.accepted = false;
            format!(
                "{} draft instances. Review, then Accept or Discard.",
                result["instances"]
            )
        } else if self.profile.task == "sen2sr-super-resolution" {
            format!("{} × {} → {} × {}, 4 Float32 RGBN bands. Shared-contrast comparison. Save GeoTIFF to retain reflectance and AI provenance.",result["sourceWidth"],result["sourceHeight"],result["width"],result["height"])
        } else {
            "Invalid input pixels are excluded.".into()
        };
        let summary = if self.profile.task == "sen2sr-super-resolution" {
            format!(
                "Provider: {}\nElapsed: {} ms\n\n{}",
                result["provider"], result["elapsedMilliseconds"], interpretation
            )
        } else {
            format!(
                "Provider: {}\nPolygons: {}\nElapsed: {} ms\n\n{}",
                result["provider"],
                result["polygons"],
                result["elapsedMilliseconds"],
                interpretation
            )
        };
        self.log(&format!(
            "{summary}\n\nSession-only result. Use Save to retain it.\n\n{}",
            self.profile.legend
        ))?;
        self.result = Some(result);
        self.running = false;
        self.progress(100, "Inference complete.")?;
        Ok(())
    }
    pub fn apply_opacity(&mut self, value: f64) -> Result<()> {
        self.opacity = value.clamp(0.0, 100.0);
        if self.overlay {
            if !self
                .window
                .viewer()
                .set_layer_opacity(0, self.opacity / 100.0)?
            {
                return Err("Cannot set result opacity".into());
            }
            self.window.viewer().refresh_layers()?;
        }
        Ok(())
    }
    pub fn export_to(&mut self, destination: String) -> Result<()> {
        let format = if self.profile.task == "sen2sr-super-resolution" {
            "geotiff"
        } else {
            "shapefile"
        };
        self.export_format(destination, format)
    }
    pub fn export_format(&mut self, destination: String, format: &str) -> Result<()> {
        if self.busy() || self.closing || self.result.is_none() {
            return Ok(());
        }
        if self.is_sam() && !self.sam.accepted {
            return Ok(());
        }
        let format = format.to_owned();
        if let Some(job) = self.job.take() {
            self.exporting = true;
            self.worker = Some(thread::spawn(move || {
                let result = job
                    .export(&destination, &format)
                    .map(|_| ())
                    .map_err(|e| e.to_string());
                Ok(WorkDone::Exported(job, result))
            }));
        }
        self.controls()
    }
    pub fn cancel(&self) -> Result<()> {
        self.cancellation.store(true, Ordering::Relaxed);
        if let Some(job) = &self.job {
            let state = job.status()?;
            if matches!(
                state["state"].as_str(),
                Some("queued" | "running" | "cancelling")
            ) {
                if let Err(error) = job.cancel() {
                    if !matches!(
                        job.status()?["state"].as_str(),
                        Some("completed" | "failed" | "cancelled")
                    ) {
                        return Err(error.into());
                    }
                }
            }
        }
        Ok(())
    }
    pub fn begin_close(&mut self) -> Result<()> {
        if self.closing {
            return Ok(());
        }
        self.closing = true;
        self.run_after_release = false;
        self.cancel()?;
        self.window.viewer().clear_layers()?;
        self.overlay = false;
        self.result = None;
        self.controls()
    }
    fn failed(&mut self, error: &str) -> Result<()> {
        self.running = false;
        self.log(error)?;
        self.progress(
            0,
            if error == "Cancelled." {
                "Cancelled."
            } else {
                "Operation failed; see diagnostics."
            },
        )?;
        if self.result.is_none() && self.job.is_some() {
            if self.overlay {
                self.window.viewer().remove_layer(0)?;
                self.overlay = false;
            }
            self.release_since = Some(Instant::now());
        }
        Ok(())
    }
    pub fn tick(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.window.process_events();
        while let Ok(event) = self.events.try_recv() {
            if self.is_sam() && event.data.event_type == 20 && event.data.int_value2 == 1 {
                self.select_point(event.data.extent.x_min, event.data.extent.y_min)?;
            }
            if event.data.event_type == 15 {
                self.render_busy = event.data.int_value != 0;
            } else if event.data.event_type == 29 && !self.busy() && !self.closing {
                self.window.set_status_text(
                    &event
                        .text
                        .replace("%p%", &format!("{}%", event.data.int_value)),
                )?;
            }
        }
        while let Ok((value, text)) = self.progress_rx.try_recv() {
            if !self.closing {
                self.progress(value, &text)?;
            }
        }
        while let Ok(event) = self.controls_rx.try_recv() {
            if self.closing {
                continue;
            }
            if self.is_sam() && self.sam_control(&event)? {
                continue;
            }
            match event.id {
                RUN => self.run()?,
                OPACITY => self.apply_opacity(event.number)?,
                PROVIDER if !self.busy() => self.provider = event.text.to_lowercase(),
                THRESHOLD if self.profile.buildings && !self.busy() => {
                    self.threshold = event.number.clamp(0.01, 0.99)
                }
                CANCEL if !self.exporting => self.cancel()?,
                SAVE if !self.busy() && self.result.is_some() => {
                    let owner = self.window.viewer().get_native_handle()?;
                    let format = if self.profile.task == "sen2sr-super-resolution" {
                        "geotiff"
                    } else {
                        "shapefile"
                    };
                    match dialog::save_format(owner, self.profile.export_name, format) {
                        Ok(Some(path)) => self.export_to(path)?,
                        Ok(None) => {}
                        Err(e) => self.log(&e.to_string())?,
                    }
                }
                _ => {}
            }
        }
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.is_finished())
        {
            let result = self
                .worker
                .take()
                .unwrap()
                .join()
                .unwrap_or_else(|_| Err("Background worker panicked".into()));
            match result {
                Ok(WorkDone::Input(raster)) if !self.closing => {
                    self.source(raster)?;
                    self.log(&format!(
                        "{}\nClick Run. No predictions loaded at startup.\n\n{}",
                        self.profile.description, self.profile.legend
                    ))?;
                    self.progress(0, "Ready. Click Run to process the image.")?;
                }
                Ok(WorkDone::Input(_)) => {}
                Ok(WorkDone::Preview(result, path))
                    if !self.closing && !self.cancellation.load(Ordering::Relaxed) =>
                {
                    self.show_completed(result, Some(path.to_string_lossy().into_owned()))?;
                }
                Ok(WorkDone::Preview(_, _)) => {
                    if !self.closing {
                        self.failed("Cancelled.")?;
                    }
                }
                Ok(WorkDone::Started(job, raster)) => {
                    self.job = Some(job);
                    if !self.closing {
                        if self.raster.is_none() {
                            self.source(raster)?;
                        }
                        self.running = true;
                    } else {
                        self.cancel()?;
                    }
                }
                Ok(WorkDone::Exported(job, result)) => {
                    self.job = Some(job);
                    self.exporting = false;
                    match result {
                        Ok(()) if !self.closing => self.progress(100, "Result saved.")?,
                        Err(e) => self.log(&e)?,
                        _ => {}
                    }
                }
                Ok(WorkDone::Released(result)) => {
                    if let Err(e) = result {
                        self.log(&e)?;
                    }
                    let run = self.run_after_release
                        && !self.closing
                        && !self.cancellation.load(Ordering::Relaxed);
                    self.run_after_release = false;
                    if run {
                        self.start_run();
                    }
                }
                Err(error) => self.failed(&error)?,
            }
        }
        if self.running && !self.closing {
            let status = self.job.as_ref().unwrap().status()?;
            self.progress(
                status["percent"].as_i64().unwrap_or(0) as i32,
                status["message"].as_str().unwrap_or("Running inference…"),
            )?;
            match status["state"].as_str() {
                Some("completed") if !self.cancellation.load(Ordering::Relaxed) => {
                    let result = self.job.as_ref().unwrap().result()?;
                    self.running = false;
                    if self.profile.buildings {
                        let source = PathBuf::from(
                            result["vectorPath"]
                                .as_str()
                                .ok_or("Result polygons are missing")?,
                        );
                        let cancel = self.cancellation.clone();
                        self.progress(100, "Preparing building polygons for display…")?;
                        self.worker = Some(thread::spawn(move || {
                            let preview = raster::prepare_vector_preview(&source, &cancel)?;
                            Ok(WorkDone::Preview(result, preview))
                        }));
                    } else {
                        self.show_completed(result, None)?;
                    }
                }
                Some("completed" | "cancelled") => self.failed("Cancelled.")?,
                Some("failed") => {
                    self.failed(status["error"].as_str().unwrap_or("Inference failed"))?
                }
                _ => {}
            }
        }
        if self.closing
            && self.worker.is_none()
            && self.job.is_some()
            && self.release_since.is_none()
        {
            self.running = false;
            self.cancel()?;
            self.release_since = Some(Instant::now());
        }
        if let Some(since) = self.release_since {
            if !self.render_busy
                && since.elapsed() >= Duration::from_millis(32)
                && self.worker.is_none()
            {
                self.release_since = None;
                if let Some(job) = self.job.take() {
                    self.worker = Some(thread::spawn(move || {
                        Ok(WorkDone::Released(job.close().map_err(|e| e.to_string())))
                    }));
                }
            }
        }
        if self.closing && self.worker.is_none() && self.job.is_none() {
            self.window.unsubscribe_events();
            self.closed = true;
        }
        self.controls()
    }
}
