//! What is in this area? Pick an area of an aerial photo; Florence-2 describes it and marks objects
//! named in plain language on this computer, and a remote vision model answers questions about it
//! when GEOKERNEL_LLM_PROVIDER / GEOKERNEL_LLM_MODEL and the provider's key are set (the area's
//! pixels are then sent to that provider). geokernel `VlmAssistant`, C ABI `GeoKernelVlmAssistant_*`.
//! Rust port of the Qt example.
//!
//! Needs geokernel with the VLM assistant API (newer than 1.5.40): `ViewerWindow::vlm_assistant`.
//! `vlm-image-qa [--smoke]` (--smoke: describe the houses, mark cars (and ask), print, quit).
//! GEOKERNEL_FLORENCE2_PACKAGE (florence2.geokernel.json of an extracted package) skips the model download.
use geokernel::{Extent, Runtime, ViewerWindow, VlmAssistant};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const TITLE: &str = "What is in this area?";
struct Asset {
    url: &'static str,
    sha: &'static str,
    required: &'static str,
    label: &'static str,
}
const PHOTO: Asset = Asset {
    url: "https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/almere_oosterwold_series_2016_2025_v1.zip",
    sha: "080742d758d9c471ee23cebd379821f61c2e5b28a28947f20ac6e20f5cc44f4d",
    required: "oosterwold_2025-epsg25831-50cm.tif",
    label: "aerial photo (56 MB)",
};
const MODEL: Asset = Asset {
    url: "https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/microsoft-florence-2-base-ft-fp32-geokernel-onnx-v1.zip",
    sha: "d848b2a7cbb2b1b2a68c6387abd6c5d0c79575d5b89a93f3373053d950b9e20a",
    required: "florence2.geokernel.json",
    label: "Florence-2 model (630 MB)",
};
// Example areas in EPSG:25831 (the CRS of the photo and of the map), 512 x 512 pixels each.
const AREAS: [(&str, [f64; 4]); 3] = [
    ("houses", [657883.0, 5800850.0, 658139.0, 5801106.0]),
    ("road crossing", [657683.0, 5800550.0, 657939.0, 5800806.0]),
    ("round garden", [658233.0, 5800450.0, 658489.0, 5800706.0]),
];
const CREDIT: &str = "Aerial photo: Beeldmateriaal Nederland / PDOK (CC BY 4.0) · Florence-2: Microsoft (MIT)";
const HOUSES: i32 = 1;
const ROAD: i32 = 2;
const GARDEN: i32 = 3;
const PICK: i32 = 4;
const DESCRIBE: i32 = 5;
const DETAIL: i32 = 6;
const QUESTION: i32 = 7;
const ASK: i32 = 8;
const PROMPT: i32 = 9;
const MARK: i32 = 10;
const PROGRESS: i32 = 11;

fn sha256(file: &Path) -> Result<String> {
    let mut digest = Sha256::new();
    let mut reader = fs::File::open(file)?;
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Downloads, SHA-256 checks and extracts one asset into the cache shared with the other GeoAI
/// examples; returns the path of its required file. Runs on a worker thread.
fn prepare(asset: &Asset, progress: &Sender<String>) -> Result<PathBuf> {
    let base = std::env::var_os("GEOKERNEL_EXAMPLES_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("GeoKernel/Examples/GeoAI")
    });
    let root = base.join(asset.sha);
    fs::create_dir_all(&root)?;
    let archive = root.join("download.zip");
    if !archive.is_file() || sha256(&archive)? != asset.sha {
        let partial = root.join(format!("{}.part", std::process::id()));
        let _ = progress.send(format!("Downloading the {}…", asset.label));
        let status = Command::new("curl.exe").args(["--fail", "--location", "--silent", "--show-error", "--output"]).arg(&partial).arg(asset.url).status()?;
        if !status.success() || sha256(&partial)? != asset.sha {
            let _ = fs::remove_file(&partial);
            return Err(format!("Download of the {} failed or its SHA-256 differs. Please retry.", asset.label).into());
        }
        fs::rename(&partial, &archive)?;
    }
    let content = root.join("content");
    if !content.is_dir() {
        let staging = root.join(format!("extract-{}", std::process::id()));
        fs::create_dir_all(&staging)?;
        let _ = progress.send(format!("Extracting the verified {}…", asset.label));
        if !Command::new("tar.exe").arg("-xf").arg(&archive).arg("-C").arg(&staging).status()?.success() {
            return Err(format!("Could not extract the {}.", asset.label).into());
        }
        fs::rename(&staging, &content)?;
    }
    let found = content.join(asset.required);
    if !found.is_file() {
        return Err(format!("{} is missing from the download.", asset.required).into());
    }
    Ok(found)
}

fn line(result: &Value) -> String {
    if let Some(error) = result["error"].as_str() {
        return error.to_string();
    }
    let text = if result["kind"] == "find" {
        format!("{} objects marked as \"{}\" (red boxes)", result["objects"].as_array().map_or(0, Vec::len), result["prompt"].as_str().unwrap_or(""))
    } else {
        result["text"].as_str().unwrap_or("").to_string()
    };
    let question = result["question"].as_str().map(|q| format!("{q} → ")).unwrap_or_default();
    format!(
        "{question}{text}  [{}, {}, {:.1} s]",
        result["model"].as_str().unwrap_or(""),
        if result["remote"] == true { "remote" } else { "local" },
        result["elapsedMs"].as_f64().unwrap_or(0.0) / 1000.0
    )
}

fn started(window: &mut ViewerWindow, outcome: geokernel::Result<()>) -> Result<()> {
    match outcome {
        Ok(()) => window.set_status_text("Working… (Florence-2 on this computer, or the remote model for questions)")?,
        Err(error) => window.set_status_text(&error.to_string())?,
    }
    Ok(())
}

fn pick(window: &mut ViewerWindow, vlm: &VlmAssistant, index: usize) -> Result<()> {
    let [x0, y0, x1, y1] = AREAS[index].1;
    if let Err(error) = vlm.set_area(x0, y0, x1, y1).and_then(|_| vlm.zoom_to_area()) {
        window.set_status_text(&error.to_string())?;
    }
    Ok(())
}

fn run(runtime: &Runtime, window: &mut ViewerWindow, smoke: bool) -> Result<()> {
    let remote = std::env::var_os("GEOKERNEL_LLM_PROVIDER").is_some();
    let mut question = "How many houses are in this area?".to_string();
    let mut prompt = "car".to_string();
    let events = window.add_control_panel(&json!({"title": TITLE, "width": 400, "controls": [
        {"id": HOUSES, "type": "button", "text": "Example: houses"},
        {"id": ROAD, "type": "button", "text": "Example: road crossing"},
        {"id": GARDEN, "type": "button", "text": "Example: round garden"},
        {"id": PICK, "type": "button", "text": "Pick an area on the map: on / off"},
        {"id": DESCRIBE, "type": "button", "text": "Describe (Florence-2, local)"},
        {"id": DETAIL, "type": "button", "text": "Describe in detail"},
        {"id": QUESTION, "type": "text", "label": "Question", "value": if remote { question.as_str() } else { "" },
         "placeholder": if remote { "" } else { "Set GEOKERNEL_LLM_PROVIDER to ask" }},
        {"id": ASK, "type": "button", "text": if remote { "Ask (remote model)" } else { "Ask (needs GEOKERNEL_LLM_PROVIDER)" }},
        {"id": PROMPT, "type": "text", "label": "Mark", "value": prompt},
        {"id": MARK, "type": "button", "text": "Mark objects (Florence-2, local)"},
        {"id": PROGRESS, "type": "progress", "label": "Progress", "value": 0}
    ]}).to_string())?;
    window.set_attribution_text(CREDIT)?;
    window.show()?;
    window.process_events();
    window.set_status_text("Preparing the aerial photo (56 MB) and the Florence-2 model (630 MB) on the first run…")?;
    let (progress, messages) = mpsc::channel::<String>();
    let (sender, preparing): (Sender<std::result::Result<(PathBuf, PathBuf), String>>, Receiver<_>) = mpsc::channel();
    std::thread::spawn(move || {
        let outcome = (|| -> Result<(PathBuf, PathBuf)> {
            let photo = prepare(&PHOTO, &progress)?;
            let model = match std::env::var_os("GEOKERNEL_FLORENCE2_PACKAGE") {
                Some(local) => PathBuf::from(local),
                None => prepare(&MODEL, &progress)?,
            };
            Ok((photo, model))
        })();
        let _ = sender.send(outcome.map_err(|e| e.to_string()));
    });
    let mut vlm: Option<VlmAssistant> = None;
    let (mut revision, mut picking, mut exit_at) = (0, false, None::<Instant>);

    while window.is_visible()? {
        window.process_events();
        while let Ok(message) = messages.try_recv() {
            window.set_status_text(&message)?;
        }
        if vlm.is_none() && exit_at.is_none() {
            if let Ok(prepared) = preparing.try_recv() {
                match prepared {
                    Err(error) => {
                        window.set_status_text(&error)?;
                        if smoke {
                            println!("smoke | error: {error}");
                            exit_at = Some(Instant::now());
                        }
                    }
                    Ok((photo, model)) => {
                        let photo = photo.to_str().ok_or("Invalid path")?.to_string();
                        window.viewer().add_layer_file(&photo)?;
                        window.set_status_text("Loading Florence-2…")?;
                        window.process_events();
                        let options = json!({"localModel": model.to_str().ok_or("Invalid path")?, "remote": remote}).to_string();
                        let opened = window.vlm_assistant(runtime, &photo, Some(&options))?;
                        // The native panel shows the answers in full, next to the controls.
                        opened.add_panel(Some(r#"{"title":"Answer","area":"right"}"#))?;
                        window.viewer().set_view_extent(Extent { x_min: 657533.0, y_min: 5800382.0, x_max: 658557.0, y_max: 5801406.0 })?;
                        window.set_control_value(PROGRESS, 100.0, "")?;
                        window.set_status_text(if remote {
                            "Ready: Florence-2 on this computer, questions go to the remote model."
                        } else {
                            "Ready: Florence-2 on this computer. Set GEOKERNEL_LLM_PROVIDER to ask questions."
                        })?;
                        if smoke {
                            pick(window, &opened, 0)?;
                            let outcome = opened.start_describe("caption");
                            started(window, outcome)?;
                        }
                        revision = opened.revision();
                        vlm = Some(opened);
                    }
                }
            }
        }
        while let Ok(event) = events.try_recv() {
            let Some(active) = vlm.as_ref() else { continue };
            match event.id {
                HOUSES | ROAD | GARDEN => pick(window, active, (event.id - HOUSES) as usize)?,
                PICK => {
                    picking = !picking;
                    active.set_interactive(picking)?;
                    window.set_status_text(if picking { "Drag a box (or click) on the map to pick the area." } else { "Map picking off." })?;
                }
                DESCRIBE => started(window, active.start_describe("caption"))?,
                DETAIL => started(window, active.start_describe("paragraph"))?,
                QUESTION if !event.text.is_empty() => question = event.text.clone(),
                ASK => started(window, active.start_ask(&question))?,
                PROMPT if !event.text.is_empty() => prompt = event.text.clone(),
                MARK => started(window, active.start_find(&prompt))?,
                _ => {}
            }
        }
        // Runs finish inside the viewer (also those started in the panel or on the map).
        if let Some(active) = vlm.as_ref() {
            if active.revision() != revision {
                revision = active.revision();
                let result = active.result()?;
                window.set_status_text(&line(&result))?;
                if smoke && exit_at.is_none() {
                    let kind = result["kind"].as_str().unwrap_or("");
                    if kind == "describe" {
                        println!("smoke | describe: {} ({} ms)", result["text"].as_str().unwrap_or(""), result["elapsedMs"]);
                        let outcome = active.start_find("car");
                        started(window, outcome)?;
                    } else if kind == "find" {
                        println!("smoke | find car: {} objects ({} ms)", result["objects"].as_array().map_or(0, Vec::len), result["elapsedMs"]);
                        if remote {
                            let outcome = active.start_ask(&question);
                            started(window, outcome)?;
                        } else {
                            exit_at = Some(Instant::now() + Duration::from_millis(500));
                        }
                    } else {
                        println!("smoke | {kind}: {} ({} ms)", result["text"].as_str().or(result["error"].as_str()).unwrap_or(""), result["elapsedMs"]);
                        exit_at = Some(Instant::now() + Duration::from_millis(500));
                    }
                }
            }
        }
        if exit_at.is_some_and(|at| Instant::now() >= at) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if let Some(active) = vlm.take() {
        active.wait(Duration::from_secs(600));
    }
    Ok(())
}

fn main() -> Result<()> {
    let smoke = std::env::args().any(|argument| argument == "--smoke");
    // This process owns the main thread and loads the trusted runtime named by GEOKERNEL_BIN.
    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| {
        let mut window = unsafe { ViewerWindow::new(&runtime, TITLE, 1400, 900)? };
        run(&runtime, &mut window, smoke)
    })();
    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;
    Ok(())
}
