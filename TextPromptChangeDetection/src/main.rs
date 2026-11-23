//! Text-prompt change detection: what changed between dates, for any concept SAM 3 can segment
//! from text. Each date of the Almere / Oosterwold series is segmented once and the objects are
//! tracked from date to date (geokernel `ChangeTasks`, C ABI `GeoKernelChange_*`). The timeline
//! shows the objects present on each date, the new ones and the ones gone since the previous
//! date; each interval's changes go through a QA review queue and the accepted ones are saved.
//! Rust port of the Qt example.
//!
//! Needs geokernel with the change task API (newer than 1.5.40): `Runtime::change_tasks`.
//! `text-prompt-change-detection [--smoke]` (--smoke: run the demo area, print the result, quit)
use geokernel::{present_on, ChangeTask, ChangeTasks, Extent, Runtime, ViewerWindow};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const TITLE: &str = "Text-prompt change detection";
struct Asset {
    url: &'static str,
    sha: &'static str,
    required: &'static str,
}
const SERIES: Asset = Asset {
    url: "https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/almere_oosterwold_series_2016_2025_v1.zip",
    sha: "080742d758d9c471ee23cebd379821f61c2e5b28a28947f20ac6e20f5cc44f4d",
    required: "oosterwold_2016-epsg25831-50cm.tif",
};
const SAM3: Asset = Asset {
    url: "https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/meta-sam3-text-fp32-geokernel-onnx-v1.zip",
    sha: "95d9bc3ebdec6a6147a9bfa97325712bfbb77fccbd6bb3f20be453c381820931",
    required: "sam3-package.json",
};
// Beeldmateriaal Nederland / PDOK annual orthophotos, 0.5 m, EPSG:25831; no 2021 layer.
const YEARS: [i32; 9] = [2016, 2017, 2018, 2019, 2020, 2022, 2023, 2024, 2025];
const DEMO_AREA: [f64; 4] = [657885.0, 5800734.0, 658205.0, 5801054.0]; // EPSG:25831, 320 m
const DEMO_VIEW: Extent = Extent { x_min: 5.317289, y_min: 52.334073, x_max: 5.322132, y_max: 52.337039 };
const STYLES: [(&str, &str); 3] = [
    ("Present", r##"{"fillColor":"#FFD000","fillOpacity":40,"lineColor":"#D4A000","lineWidth":1.6}"##),
    ("New on this date", r##"{"fillColor":"#FF2020","fillOpacity":120,"lineColor":"#FF6060","lineWidth":1.6}"##),
    ("Gone since the previous date", r##"{"fillColor":"#2070FF","fillOpacity":0,"lineColor":"#2070FF","lineWidth":1.6}"##),
];
const DATE_OPTIONS: [&str; 2] = ["Series: 9 dates 2016 – 2025", "Two dates: 2016 → 2025"];
const AREA_OPTIONS: [&str; 2] = ["Demo area (320 m)", "Full dataset (1 km)"];
const PROMPT: i32 = 1;
const SCORE: i32 = 2;
const DATES: i32 = 3;
const AREA: i32 = 4;
const PROVIDER: i32 = 5;
const RUN: i32 = 6;
const CANCEL: i32 = 7;
const PROGRESS: i32 = 8;
const TIMELINE: i32 = 9;
const PLAY: i32 = 10;
const CHANGES: i32 = 11;
const REVIEWER: i32 = 12;
const APPROVE: i32 = 13;
const REJECT: i32 = 14;
const APPROVE_PENDING: i32 = 15;
const SAVE_ACCEPTED: i32 = 16;
const SAVE_SERIES: i32 = 17;

fn raster_name(year: i32) -> String {
    format!("oosterwold_{year}-epsg25831-50cm.tif")
}

/// Downloads, SHA-256 checks and extracts an asset into the cache shared with the other GeoAI
/// examples; returns the path of its required file. Runs on a worker thread.
fn prepare(asset: &Asset, cancel: &AtomicBool, progress: &Sender<(i32, String)>) -> Result<PathBuf> {
    let base = std::env::var_os("GEOKERNEL_EXAMPLES_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("GeoKernel/Examples/GeoAI")
    });
    let root = base.join(asset.sha);
    fs::create_dir_all(&root)?;
    let archive = root.join("download.zip");
    let hash = |file: &Path| -> Result<String> {
        let mut digest = Sha256::new();
        let mut reader = fs::File::open(file)?;
        let mut buffer = vec![0u8; 1 << 20];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled.".into());
            }
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        Ok(digest.finalize().iter().map(|b| format!("{b:02x}")).collect())
    };
    if !archive.is_file() || hash(&archive)? != asset.sha {
        let partial = root.join(format!("{}.part", std::process::id()));
        let _ = progress.send((0, format!("Downloading {}…", asset.url.rsplit('/').next().unwrap_or(""))));
        let status = Command::new("curl.exe").args(["--fail", "--location", "--silent", "--show-error", "--output"]).arg(&partial).arg(asset.url).status()?;
        if !status.success() || hash(&partial)? != asset.sha {
            let _ = fs::remove_file(&partial);
            return Err("Download failed or SHA-256 mismatch. Please retry.".into());
        }
        fs::rename(&partial, &archive)?;
    }
    let content = root.join("content");
    if !content.is_dir() {
        let staging = root.join(format!("extract-{}", std::process::id()));
        fs::create_dir_all(&staging)?;
        let _ = progress.send((0, "Extracting verified asset…".into()));
        if !Command::new("tar.exe").arg("-xf").arg(&archive).arg("-C").arg(&staging).status()?.success() {
            return Err("Could not extract the asset.".into());
        }
        fs::rename(&staging, &content)?;
    }
    let found = content.join(asset.required);
    if !found.is_file() {
        return Err(format!("{} is missing from the asset.", asset.required).into());
    }
    Ok(found)
}

struct App {
    tasks: ChangeTasks,
    task: Option<ChangeTask>,
    result: Option<Value>,
    series: Option<PathBuf>,
    preparing: Option<Receiver<std::result::Result<PathBuf, String>>>,
    progress: (Sender<(i32, String)>, Receiver<(i32, String)>),
    cancel: Arc<AtomicBool>,
    workspace: PathBuf,
    revision: usize,
    shown: usize,
    rows: Vec<Value>,
    selected: usize,
    prompt: String,
    score: f64,
    dates: String,
    area: String,
    provider: String,
    reviewer: String,
    playing: Option<Instant>,
    busy: bool,
    smoke: bool,
    exit_at: Option<Instant>,
}

impl App {
    fn report(&self, window: &mut ViewerWindow, percent: f64, message: &str) -> Result<()> {
        window.set_control_value(PROGRESS, percent.clamp(0.0, 100.0), "")?;
        window.set_status_text(message)?;
        Ok(())
    }

    fn refresh(&self, window: &mut ViewerWindow) -> Result<()> {
        let viewer = window;
        for id in [PROMPT, SCORE, DATES, AREA, PROVIDER] {
            viewer.set_control_enabled(id, !self.busy)?;
        }
        viewer.set_control_enabled(RUN, !self.busy && self.series.is_some())?;
        viewer.set_control_enabled(CANCEL, self.busy)?;
        let ready = !self.busy && self.result.is_some();
        for id in [TIMELINE, PLAY, SAVE_SERIES] {
            viewer.set_control_enabled(id, ready)?;
        }
        let reviewing = ready && self.shown > 0;
        for id in [APPROVE, REJECT, APPROVE_PENDING] {
            viewer.set_control_enabled(id, reviewing && !self.reviewer.trim().is_empty())?;
        }
        viewer.set_control_enabled(SAVE_ACCEPTED, reviewing)?;
        viewer.set_control_enabled(CHANGES, reviewing)?;
        Ok(())
    }

    fn start_preparing(&mut self, asset: &'static Asset) {
        let (sender, receiver) = mpsc::channel();
        let (cancel, progress) = (self.cancel.clone(), self.progress.0.clone());
        std::thread::spawn(move || {
            let _ = sender.send(prepare(asset, &cancel, &progress).map_err(|e| e.to_string()));
        });
        self.preparing = Some(receiver);
    }

    fn find_changes(&mut self, window: &mut ViewerWindow) -> Result<()> {
        if self.busy || self.series.is_none() || self.prompt.trim().is_empty() {
            return Ok(());
        }
        self.playing = None;
        self.busy = true;
        self.cancel = Arc::new(AtomicBool::new(false));
        match std::env::var_os("GEOKERNEL_SAM3_TEXT_PACKAGE") {
            Some(package) => self.start_task(window, PathBuf::from(package))?,
            None => self.start_preparing(&SAM3),
        }
        self.refresh(window)
    }

    fn start_task(&mut self, window: &mut ViewerWindow, model: PathBuf) -> Result<()> {
        let series = self.series.clone().unwrap();
        let years: Vec<i32> = if self.dates == DATE_OPTIONS[0] { YEARS.to_vec() } else { vec![YEARS[0], YEARS[8]] };
        let mut request = json!({
            "prompt": self.prompt.trim(), "modelPackagePath": model, "scoreThreshold": self.score, "minimumArea": 20,
            "maximumGap": 1, "provider": self.provider.to_lowercase(),
            "intraOpThreads": std::thread::available_parallelism().map_or(4, |n| (n.get() / 2).max(2)),
            "dates": years.iter().map(|year| json!({"date": format!("{year}-07-01"), "rasterPath": series.join(raster_name(*year))})).collect::<Vec<_>>(),
        });
        if self.area == AREA_OPTIONS[0] {
            request["extent"] = json!(DEMO_AREA);
        }
        if let Some(old) = self.task.take() {
            old.close()?;
        }
        self.task = Some(self.tasks.start(&request)?);
        window.append_log(&format!("Looking for \"{}\" on {} dates…", self.prompt.trim(), years.len()))?;
        Ok(())
    }

    fn poll(&mut self, window: &mut ViewerWindow) -> Result<()> {
        while let Ok((percent, message)) = self.progress.1.try_recv() {
            self.report(window, percent as f64, &message)?;
        }
        if let Some(receiver) = &self.preparing {
            if let Ok(outcome) = receiver.try_recv() {
                self.preparing = None;
                match outcome {
                    Ok(file) if self.series.is_none() => self.series_ready(window, file.parent().unwrap().to_path_buf())?,
                    Ok(file) => self.start_task(window, file.parent().unwrap().to_path_buf())?,
                    Err(error) => {
                        self.busy = false;
                        self.report(window, 0.0, &error)?;
                        self.refresh(window)?;
                    }
                }
            }
        }
        if self.busy && self.preparing.is_none() {
            if let Some(task) = &self.task {
                let status = task.status()?;
                // SAM 3 needs seconds per tile on CPU: the elapsed time shows that work goes on.
                let message = format!("{} ({} s)", status["message"].as_str().unwrap_or(""), status["elapsedSeconds"].as_i64().unwrap_or(0));
                self.report(window, status["percent"].as_f64().unwrap_or(0.0), &message)?;
                match status["state"].as_str() {
                    Some("completed") => {
                        self.busy = false;
                        let result = task.result()?;
                        self.apply(window, result)?;
                    }
                    Some("failed") | Some("cancelled") => {
                        self.busy = false;
                        let error = status["error"].as_str().filter(|e| !e.is_empty()).unwrap_or("Cancelled. The previous result is kept.").to_owned();
                        self.report(window, 0.0, &error)?;
                        self.refresh(window)?;
                    }
                    _ => {}
                }
            }
        }
        if let (Some(since), Some(result)) = (self.playing, &self.result) {
            if since.elapsed() >= Duration::from_millis(1100) {
                let count = result["dates"].as_array().map_or(0, Vec::len);
                if self.shown + 1 >= count {
                    self.playing = None;
                } else {
                    self.playing = Some(Instant::now());
                    let next = self.shown + 1;
                    window.set_control_value(TIMELINE, next as f64, "")?;
                    self.show_date(window, next)?;
                }
            }
        }
        Ok(())
    }

    fn series_ready(&mut self, window: &mut ViewerWindow, directory: PathBuf) -> Result<()> {
        self.series = Some(directory);
        self.show_raster(window, YEARS[0])?;
        window.viewer().set_view_extent(DEMO_VIEW)?;
        self.report(window, 100.0, "Series ready (2016 shown). Enter what to look for and click Find changes.")?;
        self.refresh(window)?;
        if self.smoke {
            self.find_changes(window)?;
        }
        Ok(())
    }

    fn apply(&mut self, window: &mut ViewerWindow, result: Value) -> Result<()> {
        let count = result["dates"].as_array().map_or(0, Vec::len);
        let appeared: i64 = result["steps"].as_array().into_iter().flatten().filter_map(|s| s["appeared"].as_i64()).sum();
        let summary = format!(
            "{} objects tracked over {count} dates in {:.0} s; {appeared} appeared after {}. Move the timeline or play it.",
            result["objects"]["features"].as_array().map_or(0, Vec::len),
            result["seconds"].as_f64().unwrap_or(0.0),
            &result["dates"][0].as_str().unwrap_or("")[..4]
        );
        self.result = Some(result);
        self.shown = usize::MAX;
        self.show_date(window, count - 1)?;
        window.set_control_value(TIMELINE, (count - 1) as f64, "")?;
        self.report(window, 100.0, &summary)?;
        window.append_log(&summary)?;
        self.refresh(window)?;
        if self.smoke {
            let date = 3.min(count - 1);
            window.set_control_value(TIMELINE, date as f64, "")?;
            self.show_date(window, date)?;
            println!("smoke | {summary}");
            self.exit_at = Some(Instant::now() + Duration::from_secs(1));
        }
        Ok(())
    }

    fn show_raster(&mut self, window: &mut ViewerWindow, year: i32) -> Result<()> {
        let path = self.series.as_ref().unwrap().join(raster_name(year));
        let mut viewer = window.viewer();
        viewer.clear_layers()?;
        viewer.set_coordinate_system_preset("wgs84")?;
        viewer.add_layer_file(path.to_str().ok_or("Invalid path")?)?;
        viewer.set_coordinate_system_preset("wgs84")?;
        viewer.set_layer_name(0, &format!("Orthophoto {year}"))?;
        Ok(())
    }

    fn show_date(&mut self, window: &mut ViewerWindow, index: usize) -> Result<()> {
        let Some(result) = self.result.clone() else { return Ok(()) };
        let dates = result["dates"].as_array().cloned().unwrap_or_default();
        if index >= dates.len() || index == self.shown {
            return Ok(());
        }
        self.shown = index;
        let date = dates[index].as_str().unwrap_or("")[..10].to_owned();
        self.show_raster(window, date[..4].parse()?)?;
        let present = present_on(&result, &date);
        let is_new = |f: &Value| index > 0 && f["properties"]["appeared"].as_str().unwrap_or("").starts_with(&date);
        let fresh: Vec<Value> = present.iter().filter(|f| is_new(f)).map(|f| (*f).clone()).collect();
        let old: Vec<Value> = present.iter().filter(|f| !is_new(f)).map(|f| (*f).clone()).collect();
        let step = if index > 0 { Some(&result["steps"][index - 1]) } else { None };
        let gone: Vec<Value> = step
            .and_then(|s| s["features"]["features"].as_array())
            .map(|all| all.iter().filter(|f| f["properties"]["change"] == "disappeared").cloned().collect())
            .unwrap_or_default();
        for (features, (name, style)) in [old, fresh, gone].into_iter().zip(STYLES) {
            if features.is_empty() {
                continue;
            }
            self.revision += 1;
            let file = self.workspace.join(format!("layer-{}.geojson", self.revision));
            fs::write(&file, json!({"type": "FeatureCollection", "features": features}).to_string())?;
            let mut viewer = window.viewer();
            if viewer.add_layer_file(file.to_str().ok_or("Invalid path")?)? {
                viewer.set_layer_name(0, name)?;
                viewer.set_layer_style_json(0, style)?;
            }
        }
        window.viewer().refresh_layers()?;
        let label = match step {
            Some(s) => format!("{} · {} present · {} new · {} gone since {}", &date[..4], present.len(), s["appeared"], s["disappeared"],
                               &dates[index - 1].as_str().unwrap_or("")[..4]),
            None => format!("{} · {} present (first date)", &date[..4], present.len()),
        };
        window.append_log(&label)?;
        self.fill_changes(window)
    }

    fn fill_changes(&mut self, window: &mut ViewerWindow) -> Result<()> {
        let result = self.result.as_ref().unwrap();
        let step = if self.shown > 0 { Some(&result["steps"][self.shown - 1]) } else { None };
        self.rows = step.and_then(|s| s["features"]["features"].as_array().cloned()).unwrap_or_default();
        let states = step.map(|s| s["review"]["states"].clone()).unwrap_or(Value::Null);
        let labels: Vec<String> = self.rows.iter().enumerate().map(|(i, f)| {
            let p = &f["properties"];
            let id = p["qa_id"].as_str().unwrap_or("");
            let state = states[id]["state"].as_str().or(p["qa_state"].as_str()).unwrap_or("");
            let who = states[id]["reviewer"].as_str().filter(|r| !r.is_empty()).map(|r| format!(" ({r})")).unwrap_or_default();
            format!("{}. {} · score {:.3} · {:.0} m² · {state}{who}", i + 1, p["change"].as_str().unwrap_or(""),
                    p["score"].as_f64().unwrap_or(0.0), p["area_m2"].as_f64().unwrap_or(0.0))
        }).collect();
        let options = if labels.is_empty() { vec!["(no changes on the first date)".to_owned()] } else { labels };
        window.set_control_options_json(CHANGES, &json!(options).to_string())?;
        self.selected = 0;
        self.refresh(window)
    }

    fn locate(&self, window: &mut ViewerWindow, index: usize) -> Result<()> {
        let Some(feature) = self.rows.get(index) else { return Ok(()) };
        let mut points = Vec::new();
        fn walk(node: &Value, points: &mut Vec<(f64, f64)>) {
            match node.as_array() {
                Some(pair) if pair.len() >= 2 && pair[0].is_number() => points.push((pair[0].as_f64().unwrap(), pair[1].as_f64().unwrap())),
                Some(children) => children.iter().for_each(|child| walk(child, points)),
                None => {}
            }
        }
        walk(&feature["geometry"]["coordinates"], &mut points);
        let pad = 0.0006;
        let fold = |f: fn(f64, f64) -> f64, start: f64, pick: fn(&(f64, f64)) -> f64| points.iter().map(pick).fold(start, f);
        window.viewer().set_view_extent(Extent {
            x_min: fold(f64::min, f64::MAX, |p| p.0) - pad,
            y_min: fold(f64::min, f64::MAX, |p| p.1) - pad,
            x_max: fold(f64::max, f64::MIN, |p| p.0) + pad,
            y_max: fold(f64::max, f64::MIN, |p| p.1) + pad,
        })?;
        Ok(())
    }

    fn decide(&mut self, window: &mut ViewerWindow, action: &str) -> Result<()> {
        let feature = self.rows.get(self.selected).map(|f| f["properties"]["qa_id"].as_str().unwrap_or("").to_owned());
        if action != "approvePending" && feature.is_none() {
            return Ok(());
        }
        let review = self.task.as_ref().unwrap().review(self.shown - 1, action, self.reviewer.trim(), feature.as_deref(), "")?;
        self.result.as_mut().unwrap()["steps"][self.shown - 1]["review"] = review;
        self.fill_changes(window)
    }

    fn save(&mut self, window: &mut ViewerWindow, accepted: bool) -> Result<()> {
        let folder = std::env::current_dir()?.join("outputs");
        fs::create_dir_all(&folder)?;
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
        let task = self.task.as_ref().unwrap();
        let message = if accepted {
            let year = &self.result.as_ref().unwrap()["dates"][self.shown].as_str().unwrap_or("")[..4];
            let path = folder.join(format!("accepted-changes-{year}-{stamp}.gpkg"));
            let exported = task.export(path.to_str().ok_or("Invalid path")?, "accepted", Some(self.shown - 1))?;
            format!("{} accepted changes written to {} (EPSG:25831, with QA state and reviewer).", exported["written"], path.display())
        } else {
            let path = folder.join(format!("change-series-{stamp}.gpkg"));
            task.export(path.to_str().ok_or("Invalid path")?, "series", None)?;
            format!("Series written to {}: layer 'objects' (lifespans) and 'changes' (every interval).", path.display())
        };
        window.append_log(&message)?;
        Ok(())
    }
}

fn run(runtime: &Runtime, window: &mut ViewerWindow, smoke: bool) -> Result<()> {
    let events = window.add_control_panel(&json!({"title":"Change detection","width":420,"controls":[
        {"id":PROMPT,"type":"text","label":"What to look for","value":"building"},
        {"id":SCORE,"type":"number","label":"Score threshold","minimum":0.05,"maximum":0.95,"step":0.05,"decimals":2,"value":0.5},
        {"id":DATES,"type":"combo","label":"Dates","options":DATE_OPTIONS,"value":DATE_OPTIONS[0]},
        {"id":AREA,"type":"combo","label":"Area","options":AREA_OPTIONS,"value":AREA_OPTIONS[0]},
        {"id":PROVIDER,"type":"combo","label":"Provider","options":["CPU","CUDA","Auto"],"value":"CPU"},
        {"id":RUN,"type":"button","text":"Find changes"},
        {"id":CANCEL,"type":"button","text":"Cancel"},
        {"id":PROGRESS,"type":"progress","label":"Progress","value":0},
        {"id":TIMELINE,"type":"slider","label":"Timeline","minimum":0,"maximum":YEARS.len() - 1,"value":0},
        {"id":PLAY,"type":"button","text":"▶ Play / ■ Stop"},
        {"id":CHANGES,"type":"combo","label":"Changes","options":["(run first)"],"value":""},
        {"id":REVIEWER,"type":"text","label":"Reviewer","value":"","placeholder":"Your name (recorded in the audit trail)"},
        {"id":APPROVE,"type":"button","text":"Approve"},
        {"id":REJECT,"type":"button","text":"Reject"},
        {"id":APPROVE_PENDING,"type":"button","text":"Approve all pending"},
        {"id":SAVE_ACCEPTED,"type":"button","text":"Save accepted changes (outputs/)"},
        {"id":SAVE_SERIES,"type":"button","text":"Save series GeoPackage (outputs/)"}
    ]}).to_string())?;
    window.add_log_panel("Timeline and review")?;
    window.set_attribution_text("Imagery: Beeldmateriaal Nederland / PDOK, CC BY 4.0 · Model: Meta SAM 3 (SAM License)")?;
    window.show()?;
    window.process_events();
    let workspace = std::env::temp_dir().join(format!("geokernel-change-{}", std::process::id()));
    fs::create_dir_all(&workspace)?;
    let mut app = App {
        tasks: runtime.change_tasks()?, // fails early when the runtime has no change API
        task: None,
        result: None,
        series: None,
        preparing: None,
        progress: mpsc::channel(),
        cancel: Arc::new(AtomicBool::new(false)),
        workspace: workspace.clone(),
        revision: 0,
        shown: 0,
        rows: Vec::new(),
        selected: 0,
        prompt: "building".into(),
        score: 0.5,
        dates: DATE_OPTIONS[0].into(),
        area: AREA_OPTIONS[0].into(),
        provider: "CPU".into(),
        reviewer: String::new(),
        playing: None,
        busy: false,
        smoke,
        exit_at: None,
    };
    window.append_log("Yellow: present · red: new on this date · blue outline: gone since the previous date. Changes scoring 0.75 or more are accepted automatically, 0.5–0.75 wait for review. The SAM 3 text package (3.1 GB) is downloaded on the first run.")?;
    app.report(window, 0.0, "Preparing the 9-date Oosterwold series…")?;
    app.start_preparing(&SERIES);
    app.refresh(window)?;

    while window.is_visible()? {
        window.process_events();
        while let Ok(event) = events.try_recv() {
            match event.id {
                PROMPT => app.prompt = event.text.clone(),
                SCORE => app.score = event.number,
                DATES => app.dates = event.text.clone(),
                AREA => app.area = event.text.clone(),
                PROVIDER => app.provider = event.text.clone(),
                REVIEWER => { app.reviewer = event.text.clone(); app.refresh(window)?; }
                RUN => app.find_changes(window)?,
                CANCEL => {
                    app.cancel.store(true, Ordering::Relaxed);
                    if let Some(task) = &app.task { let _ = task.cancel(); }
                }
                TIMELINE => { app.show_date(window, event.number.round() as usize)?; }
                PLAY if app.result.is_some() => {
                    if app.playing.take().is_none() {
                        window.set_control_value(TIMELINE, 0.0, "")?;
                        app.show_date(window, 0)?;
                        app.playing = Some(Instant::now());
                    }
                }
                CHANGES => {
                    if let Some(index) = event.text.split('.').next().and_then(|n| n.trim().parse::<usize>().ok()) {
                        app.selected = index - 1;
                        app.locate(window, index - 1)?;
                    }
                }
                APPROVE => app.decide(window, "approve")?,
                REJECT => app.decide(window, "reject")?,
                APPROVE_PENDING => app.decide(window, "approvePending")?,
                SAVE_ACCEPTED => app.save(window, true)?,
                SAVE_SERIES => app.save(window, false)?,
                _ => {}
            }
        }
        app.poll(window)?;
        if app.exit_at.is_some_and(|at| Instant::now() >= at) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if let Some(task) = app.task.take() {
        task.close()?; // joins the current date if it is still running
    }
    let _ = fs::remove_dir_all(workspace);
    Ok(())
}

fn main() -> Result<()> {
    let smoke = std::env::args().any(|argument| argument == "--smoke");
    // This process owns the main thread and loads the trusted runtime named by GEOKERNEL_BIN.
    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| {
        let mut window = unsafe { ViewerWindow::new(&runtime, TITLE, 1440, 900)? };
        run(&runtime, &mut window, smoke)
    })();
    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;
    Ok(())
}
