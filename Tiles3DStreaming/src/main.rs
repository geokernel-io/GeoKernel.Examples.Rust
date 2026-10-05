mod bootstrap;
mod terrain;

use geokernel::{Runtime, ViewerWindow};
use serde_json::{json, Value};
use std::{path::PathBuf, thread, time::{Duration, Instant}};
use terrain::Terrain;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn display(window: &mut ViewerWindow, state: &Value) -> Result<()> {
    let phase = if state["active"] != true { "Stopped" }
        else if state["loading"] == true { "Updating (previous tiles visible)" } else { "Active" };
    let text = |key: &str| state[key].as_str().unwrap_or("0");
    let cache = text("cacheBytes").parse::<u64>()? as f64 / (1024.0 * 1024.0);
    let error = state["error"].as_str().unwrap_or("").trim()
        .replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('\n', "<br>");
    window.clear_log()?;
    window.append_log(&format!(
        "State: {phase}<br>Loaded tiles: {}<br>Deferred tiles: {}<br>Detail budget limited: {}<br>CPU cache: {cache:.1} MiB<br>Cache hits: {}<br>Drawn vertices: {}<br>GPU upload pending: {}<br>{error}",
        text("tiles"), text("deferredTiles"), if state["selectionLimited"] == true { "Yes" } else { "No" },
        text("cacheHits"), text("drawnVertices"), if state["uploadPending"] == true { "Yes" } else { "No" }))?;
    Ok(())
}

fn controls_state(window: &mut ViewerWindow, ready: bool, loading: bool, visible: bool, state: &Value) -> Result<()> {
    window.set_control_enabled(1, !loading)?;
    window.set_control_enabled(2, loading)?;
    for id in [3, 4, 5, 7, 9] { window.set_control_enabled(id, ready && !loading)?; }
    window.set_control_enabled(6, ready && !loading && state["active"] == true)?;
    window.set_control_enabled(8, ready && !loading && visible && state["attached"] == true)?;
    Ok(())
}

fn run(runtime: &Runtime) -> Result<()> {
    let mut window = unsafe { ViewerWindow::new(runtime, "Tiles3DStreaming — GeoKernel", 1400, 850)? };
    let controls = window.add_control_panel(&json!({
        "title":"3D Tiles streaming",
        "controls":[
            {"id":1,"type":"button","text":"Reload sample"},
            {"id":2,"type":"button","text":"Cancel loading"},
            {"id":3,"type":"combo","label":"Orthophoto","options":["Visible","Hidden"],"value":"Visible"},
            {"id":4,"type":"combo","label":"CPU geometry batch budget (MiB)","options":["64","96","128"],"value":"96"},
            {"id":5,"type":"button","text":"Start / apply budget"},
            {"id":6,"type":"button","text":"Stop streaming (keep current tiles)"},
            {"id":7,"type":"combo","label":"Buildings","options":["Visible","Hidden"],"value":"Visible"},
            {"id":8,"type":"button","text":"Focus loaded tiles"},
            {"id":9,"type":"button","text":"Reset view"}
        ]
    }).to_string())?;
    window.add_log_panel("Streaming diagnostics")?;
    window.append_log("Camera movement selects detail from the downloaded local archive.<br>Vertical exaggeration: 1×. DEM uses EGM08D595; alignment is approximate.<br>Hiding clears tiles; showing restarts streaming. Stop retains tiles and may wait for in-flight work.<br>Cache counters are not total RAM/VRAM.")?;
    window.show()?;
    window.process_events();
    let parent = window.viewer().get_native_handle()?;
    let bin = PathBuf::from(std::env::var_os("GEOKERNEL_BIN").ok_or("SDK is not configured")?);
    let library = bin.join("GeoKernel.Viewer3D.dll");
    // Drops before the parent window and Qt runtime.
    let mut terrain = unsafe { Terrain::new(parent, &library)? };
    terrain.resize()?;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data");
    let dem = root.join("sagrada_familia_terrain/sagrada_familia_terrain.tif").canonicalize()?;
    let image = root.join("sagrada_familia_ortophoto/sagrada_familia_ortophoto.tif").canonicalize()?;
    let tiles = root.join("sagrada_familia_3d_tiles/tileset.json").canonicalize()?;
    let geoid = root.join("egm08d595/EGM08D595/cat80000.gr").canonicalize()?;
    let mut ready = false;
    let mut loading = true;
    let mut visible = true;
    let mut resume = false;
    let mut imagery = 1;
    let mut budget = 96;
    let mut snapshot = Value::Null;
    let mut next_poll = Instant::now();
    terrain.load(&dem, &image, &geoid, &tiles)?;
    window.set_status_text("Loading corrected terrain…")?;
    controls_state(&mut window, ready, loading, visible, &snapshot)?;
    while window.is_visible()? {
        window.process_events();
        if !window.is_visible()? { break; }
        terrain.resize()?;
        while let Ok(event) = controls.try_recv() {
            let result: Result<()> = (|| {
                match event.id {
                    1 if !loading => {
                        resume = ready && terrain.state()?["active"] == true;
                        if ready { terrain.stop(false)?; }
                        if let Err(error) = terrain.load(&dem, &image, &geoid, &tiles) {
                            if resume { terrain.start(&tiles, budget)?; }
                            return Err(error);
                        }
                        loading = true;
                        window.set_status_text("Loading corrected terrain…")?;
                    }
                    2 if loading => terrain.cancel(),
                    _ if !ready || loading => {}
                    3 => { imagery = i32::from(event.text != "Hidden"); terrain.imagery(imagery)?; }
                    4 => budget = event.text.parse()?,
                    5 => {
                        terrain.start(&tiles, budget)?;
                        visible = true;
                        window.set_control_value(7, 0.0, "Visible")?;
                    }
                    6 => terrain.stop(false)?,
                    7 => {
                        let show = event.text != "Hidden";
                        if show != visible {
                            if show { terrain.start(&tiles, budget)?; } else { terrain.stop(true)?; }
                            visible = show;
                        }
                    }
                    8 => terrain.focus()?,
                    9 => terrain.reset()?,
                    _ => {}
                }
                Ok(())
            })();
            if let Err(error) = result { window.set_status_text(&error.to_string())?; }
        }
        if loading {
            match terrain.poll() {
                Ok(0) => {}
                Ok(1) => {
                    loading = false;
                    ready = true;
                    snapshot = Value::Null;
                    terrain.imagery(imagery)?;
                    resume = visible;
                    window.set_status_text("Terrain loaded — camera movement controls tile detail.")?;
                }
                Ok(-2) => { loading = false; window.set_status_text("Loading cancelled.")?; }
                Ok(_) => { loading = false; window.set_status_text("Load ended without a scene.")?; }
                Err(error) => { loading = false; window.set_status_text(&error.to_string())?; }
            }
            if !loading && ready && resume {
                if let Err(error) = terrain.start(&tiles, budget) { window.set_status_text(&error.to_string())?; }
            }
        }
        if ready && !loading && Instant::now() >= next_poll {
            next_poll = Instant::now() + Duration::from_millis(500);
            let current = terrain.state()?;
            if current != snapshot { display(&mut window, &current)?; snapshot = current; }
        }
        controls_state(&mut window, ready, loading, visible, &snapshot)?;
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
