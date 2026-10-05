mod bootstrap;
mod point_cloud;

use geokernel::{Runtime, ViewerWindow};
use point_cloud::PointCloud;
use serde_json::{json, Value};
use std::{path::PathBuf, thread, time::{Duration, Instant}};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn display(window: &mut ViewerWindow, state: &Value) -> Result<()> {
    let total = state["total"].as_str().unwrap_or("0").parse::<u64>()?;
    let sample = state["sample"].as_str().unwrap_or("0").parse::<u64>()?;
    let low = state["minimumZ"].as_f64().unwrap_or(0.0);
    let high = state["maximumZ"].as_f64().unwrap_or(0.0);
    let error = state["error"].as_str().unwrap_or("").trim()
        .replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    window.clear_log()?;
    window.append_log(&format!(
        "Source: {total} points<br>Display sample: {sample} points<br>Source Z: {low:.2} – {high:.2} m<br>RGB available: {}<br>{error}<br><br>Local normalized preview. Source coordinates remain EPSG:32630; heights are provisional. Source points are preserved. Point size: 1 pixel (SDK fixed).",
        if state["hasRgb"] == true { "Yes" } else { "No" }))?;
    Ok(())
}

fn controls_state(window: &mut ViewerWindow, ready: bool, loading: bool) -> Result<()> {
    for id in [1, 4] { window.set_control_enabled(id, !loading)?; }
    window.set_control_enabled(2, loading)?;
    for id in [3, 5] { window.set_control_enabled(id, ready && !loading)?; }
    Ok(())
}

fn run(runtime: &Runtime) -> Result<()> {
    let mut window = unsafe { ViewerWindow::new(runtime, "PointCloudViewer — GeoKernel", 1200, 800)? };
    let controls = window.add_control_panel(&json!({
        "title":"EuroSDR P4 — 40 × 40 m crop",
        "controls":[
            {"id":1,"type":"button","text":"Load / apply sample limit"},
            {"id":2,"type":"button","text":"Cancel loading"},
            {"id":3,"type":"combo","label":"Colour","options":["RGB","Height"],"value":"RGB"},
            {"id":4,"type":"combo","label":"Sample limit","options":["50000","100000","250000"],"value":"250000"},
            {"id":5,"type":"button","text":"Reset view"}
        ]
    }).to_string())?;
    window.add_log_panel("Point cloud")?;
    window.append_log("Reading the downloaded LAZ sample…<br>Left drag: pan | Wheel/right drag: zoom | Middle/Ctrl+left drag: orbit | Shift+left drag: look")?;
    window.show()?;
    window.process_events();
    let parent = window.viewer().get_native_handle()?;
    let bin = PathBuf::from(std::env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    let library = bin.join("GeoKernel.Viewer3D.dll");
    // The adapter is destroyed before its parent window and the Qt runtime.
    let mut cloud = unsafe { PointCloud::new(parent, &library)? };
    cloud.resize()?;
    let prepared_path = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("pointcloud-path.txt"))?;
    let source = PathBuf::from(prepared_path.trim().trim_start_matches('\u{feff}')).canonicalize()?;
    let mut ready = false;
    let mut loading = true;
    let mut limit = 250000;
    let mut height = false;
    let mut snapshot = Value::Null;
    let mut next_poll = Instant::now();
    cloud.load(&source, limit)?;
    window.set_status_text("Reading and sampling LAZ…")?;
    controls_state(&mut window, ready, loading)?;
    while window.is_visible()? {
        window.process_events();
        if !window.is_visible()? { break; }
        cloud.resize()?;
        while let Ok(event) = controls.try_recv() {
            let result: Result<()> = (|| {
                match event.id {
                    1 if !loading => {
                        cloud.load(&source, limit)?;
                        loading = true;
                        window.set_status_text("Reading and sampling LAZ…")?;
                    }
                    2 if loading => cloud.cancel(),
                    4 if !loading => {
                        let requested = event.text.parse::<i32>()?;
                        if ![50000, 100000, 250000].contains(&requested) {
                            return Err("Invalid sample limit".into());
                        }
                        limit = requested;
                    }
                    3 if ready && !loading => {
                        let requested = event.text == "Height";
                        cloud.style(requested)?;
                        height = requested;
                    }
                    5 if ready && !loading => cloud.reset()?,
                    _ => {}
                }
                Ok(())
            })();
            if let Err(error) = result { window.set_status_text(&error.to_string())?; }
        }
        if loading {
            match cloud.poll() {
                Ok(0) => {}
                Ok(1) => {
                    loading = false;
                    ready = true;
                    snapshot = Value::Null;
                    if let Err(error) = cloud.style(height) {
                        window.set_status_text(&error.to_string())?;
                    } else {
                        window.set_status_text("Point cloud loaded.")?;
                    }
                }
                Ok(-2) => {
                    loading = false;
                    window.set_status_text(if ready { "Cancelled; previous cloud retained." } else { "Loading cancelled." })?;
                }
                Ok(_) => { loading = false; window.set_status_text("Load ended without a point cloud.")?; }
                Err(error) => { loading = false; window.set_status_text(&error.to_string())?; }
            }
        }
        if ready && !loading && Instant::now() >= next_poll {
            next_poll = Instant::now() + Duration::from_secs(1);
            match cloud.state() {
                Ok(current) => {
                    if current != snapshot { display(&mut window, &current)?; snapshot = current; }
                }
                Err(error) => window.set_status_text(&error.to_string())?,
            }
        }
        controls_state(&mut window, ready, loading)?;
        thread::sleep(Duration::from_millis(16));
    }
    Ok(())
}

fn main() -> Result<()> {
    bootstrap::prepare()?;
    let runtime = unsafe { Runtime::from_env()? };
    let result = run(&runtime);
    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    result?;
    shutdown?;
    Ok(())
}
