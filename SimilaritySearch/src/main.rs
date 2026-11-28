//! Find similar: scan Flevoland for places like a solar park, a lake, a town or a field. Every
//! 80 m cell of the AlphaEarth 2024 embedding grid (64 values per cell, GeoKernel.SampleData) is
//! ranked by cosine similarity to the query (one cell, or the mean of a box); the viewer shows a
//! heat map of the most similar cells and outlines the best separated matches (geokernel
//! `SimilaritySearch`, C ABI `GeoKernelSimilarity_*`). Rust port of the Qt example.
//!
//! Needs geokernel with the find-similar API (newer than 1.5.40): `ViewerWindow::similarity_search`.
//! `similarity-search [--smoke]` (--smoke: search the solar park, print the result, quit)
use geokernel::{Extent, Runtime, SimilaritySearch, ViewerWindow};
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

const TITLE: &str = "Find similar";
const DATA_URL: &str = "https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/flevoland_alphaearth_2024_80m_v1.zip";
const DATA_SHA: &str = "c0a9f05f8680be32f46b9db4f43fa22345f0753a9bfdd9e9ff7fad83bffc4d88";
const GRID: &str = "flevoland_alphaearth_2024_80m.tif";
// Example places in EPSG:32631 (the CRS of the grid and of the map): a box or a point.
const PRESETS: [(&str, &[f64]); 4] = [
    ("Solar park", &[652172.0, 5809772.0, 652868.0, 5810511.0]),
    ("Open water", &[652727.0, 5819032.0]),
    ("Town centre", &[650788.0, 5804723.0]),
    ("Farmland", &[683392.0, 5816743.0]),
];
const ATTRIBUTION: &str = "The AlphaEarth Foundations Satellite Embedding dataset is produced by Google and Google DeepMind (CC BY 4.0) · 2024 layer, 80 m, Source Cooperative mirror, resampled by GeoKernel";
const SOLAR: i32 = 1;
const WATER: i32 = 2;
const TOWN: i32 = 3;
const FARM: i32 = 4;
const INTERACTIVE: i32 = 5;
const THRESHOLD: i32 = 6;
const MATCHES: i32 = 7;
const PROGRESS: i32 = 8;

/// Downloads, SHA-256 checks and extracts the grid into the cache shared with the other GeoAI
/// examples; returns its path. Runs on a worker thread.
fn prepare(progress: &Sender<String>) -> Result<PathBuf> {
    let base = std::env::var_os("GEOKERNEL_EXAMPLES_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("GeoKernel/Examples/GeoAI")
    });
    let root = base.join(DATA_SHA);
    fs::create_dir_all(&root)?;
    let archive = root.join("download.zip");
    let hash = |file: &Path| -> Result<String> {
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
    };
    if !archive.is_file() || hash(&archive)? != DATA_SHA {
        let partial = root.join(format!("{}.part", std::process::id()));
        let _ = progress.send("Downloading flevoland_alphaearth_2024_80m_v1.zip (31 MB)…".into());
        let status = Command::new("curl.exe").args(["--fail", "--location", "--silent", "--show-error", "--output"]).arg(&partial).arg(DATA_URL).status()?;
        if !status.success() || hash(&partial)? != DATA_SHA {
            let _ = fs::remove_file(&partial);
            return Err("Download failed or SHA-256 mismatch. Please retry.".into());
        }
        fs::rename(&partial, &archive)?;
    }
    let content = root.join("content");
    if !content.is_dir() {
        let staging = root.join(format!("extract-{}", std::process::id()));
        fs::create_dir_all(&staging)?;
        let _ = progress.send("Extracting the verified grid…".into());
        if !Command::new("tar.exe").arg("-xf").arg(&archive).arg("-C").arg(&staging).status()?.success() {
            return Err("Could not extract the grid.".into());
        }
        fs::rename(&staging, &content)?;
    }
    let found = content.join(GRID);
    if !found.is_file() {
        return Err(format!("{GRID} is missing from the download.").into());
    }
    Ok(found)
}

fn summary(result: &Value) -> String {
    let matches = result["matches"].as_array().map_or(0, Vec::len);
    let best = result["matches"][0]["similarity"].as_f64().unwrap_or(0.0);
    format!(
        "{matches} matches among {} cells in {:.0} ms (best similarity {best:.3}).",
        result["validCells"].as_f64().unwrap_or(0.0),
        result["elapsedMs"].as_f64().unwrap_or(0.0)
    )
}

fn show(window: &mut ViewerWindow, result: &Value) -> Result<()> {
    let lines: Vec<String> = result["matches"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, m)| format!("{}. {:.3} ({}, {})", i + 1, m["similarity"].as_f64().unwrap_or(0.0), m["column"], m["row"]))
        .collect();
    window.set_control_options_json(MATCHES, &serde_json::to_string(&if lines.is_empty() { vec!["(no match)".to_string()] } else { lines })?)?;
    window.set_status_text(&summary(result))?;
    Ok(())
}

/// Runs a preset and frames the query place and its ten best matches.
fn search_preset(window: &mut ViewerWindow, search: &SimilaritySearch, index: usize) -> Result<Value> {
    let place = PRESETS[index].1;
    let result = if place.len() == 4 { search.search_extent(place[0], place[1], place[2], place[3])? } else { search.search_point(place[0], place[1])? };
    show(window, &result)?;
    let (mut x0, mut y0, mut x1, mut y1) = (place[0], place[1], place[place.len() - 2], place[place.len() - 1]);
    for m in result["matches"].as_array().into_iter().flatten().take(10) {
        let e: Vec<f64> = m["extent"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
        x0 = x0.min(e[0]);
        y0 = y0.min(e[1]);
        x1 = x1.max(e[2]);
        y1 = y1.max(e[3]);
    }
    let margin = (x1 - x0).max(y1 - y0) * 0.15 + 500.0;
    window.viewer().set_view_extent(Extent { x_min: x0 - margin, y_min: y0 - margin, x_max: x1 + margin, y_max: y1 + margin })?;
    Ok(result)
}

fn run(runtime: &Runtime, window: &mut ViewerWindow, smoke: bool) -> Result<()> {
    let events = window.add_control_panel(&json!({"title":"Find similar","width":380,"controls":[
        {"id":SOLAR,"type":"button","text":"Solar park (Zonnepark Zuyderzon)"},
        {"id":WATER,"type":"button","text":"Open water (Markermeer)"},
        {"id":TOWN,"type":"button","text":"Town centre (Almere Stad)"},
        {"id":FARM,"type":"button","text":"Farmland (Oostelijk Flevoland)"},
        {"id":INTERACTIVE,"type":"button","text":"Find similar on the map: on / off"},
        {"id":THRESHOLD,"type":"number","label":"Heat map ≥","minimum":0.5,"maximum":0.99,"step":0.01,"decimals":2,"value":0.9},
        {"id":MATCHES,"type":"combo","label":"Matches","options":["(search first)"],"value":""},
        {"id":PROGRESS,"type":"progress","label":"Progress","value":0}
    ]}).to_string())?;
    window.set_attribution_text(ATTRIBUTION)?;
    window.show()?;
    window.process_events();
    window.set_status_text("Preparing the Flevoland AlphaEarth embeddings (31 MB on the first run)…")?;
    let (progress, messages) = mpsc::channel::<String>();
    let (sender, preparing): (Sender<std::result::Result<PathBuf, String>>, Receiver<_>) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(prepare(&progress).map_err(|e| e.to_string()));
    });
    let mut search: Option<SimilaritySearch> = None;
    let (mut revision, mut interactive, mut exit_at) = (0, false, None::<Instant>);

    while window.is_visible()? {
        window.process_events();
        while let Ok(message) = messages.try_recv() {
            window.set_status_text(&message)?;
        }
        if search.is_none() {
            if let Ok(prepared) = preparing.try_recv() {
                let grid = prepared?;
                let path = grid.to_str().ok_or("Invalid path")?;
                {
                    // The embedding grid itself is the background: its first three dimensions as muted false colour.
                    let mut viewer = window.viewer();
                    viewer.add_layer_file(path)?;
                    viewer.set_layer_opacity(0, 0.45)?;
                }
                let opened = window.similarity_search(runtime, path, Some(r#"{"maximumResults":20,"threshold":0.9}"#))?;
                let info = opened.info()?;
                let e: Vec<f64> = info["extent"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
                window.viewer().set_view_extent(Extent { x_min: e[0], y_min: e[1], x_max: e[2], y_max: e[3] })?;
                window.set_control_value(PROGRESS, 100.0, "")?;
                window.set_status_text(&format!(
                    "Ready: {} × {} cells of 80 m, {} dimensions. Pick an example or search on the map.",
                    info["width"], info["height"], info["dimensions"]
                ))?;
                if smoke {
                    let started = Instant::now();
                    let result = search_preset(window, &opened, 0)?;
                    println!("smoke | Solar park: {} ({} ms with the view)", summary(&result), started.elapsed().as_millis());
                    exit_at = Some(Instant::now() + Duration::from_secs(1));
                }
                revision = opened.revision();
                search = Some(opened);
            }
        }
        while let Ok(event) = events.try_recv() {
            let Some(active) = search.as_ref() else { continue };
            match event.id {
                SOLAR | WATER | TOWN | FARM => {
                    if let Err(error) = search_preset(window, active, (event.id - SOLAR) as usize) {
                        window.set_status_text(&error.to_string())?;
                    }
                }
                INTERACTIVE => {
                    interactive = !interactive;
                    active.set_interactive(interactive)?;
                    window.set_status_text(if interactive { "Click a place (its cell) or drag a box (the mean of its cells) on the map." } else { "Map search off." })?;
                }
                THRESHOLD => active.set_heatmap_threshold(Some(event.number))?,
                MATCHES => {
                    if let Some(index) = event.text.split('.').next().and_then(|n| n.trim().parse::<i32>().ok()) {
                        let _ = active.zoom_to_match(index - 1);
                    }
                }
                _ => {}
            }
        }
        // Interactive clicks search inside the viewer; pick up their results.
        if let Some(active) = search.as_ref() {
            if active.revision() != revision {
                revision = active.revision();
                let result = active.result()?;
                show(window, &result)?;
            }
        }
        if exit_at.is_some_and(|at| Instant::now() >= at) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(search);
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
