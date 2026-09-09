mod bootstrap;
use geokernel::{Extent, ViewerTool, ViewerWindow};
use runtime::Result;
use serde_json::json;

fn apply(window: &mut ViewerWindow, preset: &str) -> Result<()>
{
    if !window.viewer().set_coordinate_system_preset(preset)? { return Err(format!("Unknown CRS: {preset}").into()); }
    if preset == "EPSG:4326" {
        window.viewer().set_view_extent(Extent { x_min:-180.0,y_min:-85.0,x_max:180.0,y_max:85.0 })?;
    } else if preset == "EPSG:3857" {
        window.viewer().set_view_extent(Extent { x_min:-20037508.34,y_min:-20037508.34,x_max:20037508.34,y_max:20037508.34 })?;
    } else { window.viewer().zoom_to_layer(0)?; }
    window.set_status_text(&format!("Viewer CRS: {preset}; source layer remains EPSG:4326"))?;
    Ok(())
}

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    window.add_navigation_toolbar();
    let controls = window.add_control_panel(&json!({"title":"Wgs84Setup","controls":[
        {"id":1,"type":"button","text":"Full Extent"}
    ]}).to_string())?;
    let events = window.subscribe_events();
    if !window.viewer().add_layer_file(path)? { return Err("Could not load world".into()); }
    window.viewer().set_layer_coordinate_system_preset(0,"EPSG:4326")?;
    window.viewer().use_tool(ViewerTool::Pan);
    runtime::show(window)?;
    runtime::pump(window,100);
    let preset = "EPSG:4326";
    apply(window,preset)?;

    while window.is_visible()? {
        window.process_events();
        while let Ok(control)=controls.try_recv() {
            if control.id == 1 { apply(window,preset)?; }
        }
        while let Ok(event)=events.try_recv() {
            if event.data.event_type==19 {
                let e=event.data.extent;
                window.set_status_text(&format!("{preset}: {:.6}, {:.6}",e.x_min,e.y_min))?;
            }
        }

    }
    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("Wgs84Setup","world_4326/world_4326.shp",run)
}

mod runtime
{
    use geokernel::{Runtime, ViewerWindow};

    use std::{
        error::Error,
        path::PathBuf,
        time::{Duration, Instant},
    };
    pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

    pub fn main(name: &str, default_data: &str, run: fn(&mut ViewerWindow, &str) -> Result<()>) -> Result<()>
    {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data").join(default_data);

        let path = if default_data.is_empty() {
            String::new()
        } else {
            path.canonicalize()?.to_str().ok_or("Invalid data path")?.to_owned()
        };
        // This standalone process owns the main thread and loads the trusted matching SDK.
        let runtime = unsafe { Runtime::from_env()? };
        let outcome = (|| {
            let mut window = unsafe {
                if matches!(name, "MultiWindowSync" | "LabelCollisionOff") {
                    ViewerWindow::new_dual(&runtime, name, 1280, 760)?
                } else {
                    ViewerWindow::new(&runtime, name, 1200, 800)?
                }
            };
            run(&mut window, &path)
        })();
        let shutdown = unsafe { runtime.shutdown_viewer_application() };
        outcome?;
        shutdown?;
        Ok(())
    }

    pub fn show(window: &mut ViewerWindow) -> Result<()>
    {
        window.show()?;
        window.process_events();
        Ok(())
    }

    pub fn pump(window: &mut ViewerWindow, millis: u64)
    {
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(millis) {
            window.process_events();
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
