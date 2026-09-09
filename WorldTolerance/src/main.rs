mod bootstrap;
use geokernel::{Extent, ViewerTool, ViewerWindow};
use runtime::Result;
use serde_json::{json, Value};

fn report(window: &mut ViewerWindow, text: &str) -> Result<()>
{
    let value: Value = serde_json::from_str(text)?;
    let escaped = serde_json::to_string_pretty(&value)?.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    window.clear_log()?;
    window.append_log(&format!("<pre>{escaped}</pre>"))?;
    Ok(())
}

fn click(window: &mut ViewerWindow, x: f64, y: f64, tolerance: f64) -> Result<()>
{
    let (wx, wy) = window.viewer().screen_to_world(x, y)?;
    let text = window.viewer().hit_test_features_json(wx, wy, tolerance)?;
    let hits: Vec<Value> = serde_json::from_str(&text)?;
    window.viewer().clear_selected_features()?;
    if let Some(hit) = hits.first() {
        let id = |name: &str| -> Result<i32> {
            Ok(i32::try_from(hit[name].as_i64().ok_or_else(|| format!("Missing hit field: {name}"))?)?)
        };
        if !window.viewer().select_feature_hit(wx, wy, tolerance, id("layerIndex")?, id("shapeId")?, id("featureId")?)? {
            return Err("Could not select the hit feature".into());
        }
        window.set_status_text(&format!("{} hit(s) | Selected: {}", hits.len(), hit["layerName"].as_str().unwrap_or("")))?;
    } else {
        window.set_status_text("No feature inside world tolerance")?;
    }
    report(window, &text)?;
    Ok(())
}

fn extra_path(default: &str) -> String
{
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../data").join(default).to_string_lossy().into_owned()
}

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("WorldTolerance")?;
    let controls = window.add_control_panel(&json!({"title":"WorldTolerance","controls":[
        {"id":1,"type":"button","text":"WorldTolerance"},
        {"id":2,"type":"button","text":"Pan"},
        {"id":3,"type":"button","text":"Clear Selection"},
        {"id":4,"type":"button","text":"Full Extent"},{"id":5,"type":"number","label":"World tolerance (degrees)","minimum":0,"maximum":5,"step":0.05,"value":0.25}
    ]}).to_string())?;
    let events = window.subscribe_events();
    let mut tolerance=0.25;
    {
        let mut viewer = window.viewer();
        viewer.set_coordinate_system_preset("EPSG:4326")?;
        for (file, title, color) in [
            (path.to_owned(), "World", "#D8E5E1"),
            (extra_path("usa_states/usa_states.shp"), "States", "#C7DEE7"),
            (extra_path("usa_cities/usa_cities.shp"), "Cities", "#D95D39")
        ] {
            if !viewer.add_layer_file(&file)? { return Err(format!("Could not load {file}").into()); }
            viewer.set_layer_name(0, title)?;
            viewer.set_layer_style_json(0, &json!({"fillColor":color,"fillOpacity":160,"pointColor":color,"pointSize":8,"lineColor":"#708984","lineWidth":0.8,"selectedLineColor":"#F59E0B","selectedLineWidth":4}).to_string())?;
        }
        viewer.use_tool(ViewerTool::Info);
    }
    runtime::show(window)?;
    runtime::pump(window, 100);
    window.viewer().set_view_extent(Extent { x_min:-130.0, y_min:22.0, x_max:-65.0, y_max:55.0 })?;
    runtime::pump(window, 100);

    while window.is_visible()? {
        window.process_events();
        while let Ok(control) = controls.try_recv() {
            match control.id {
                1 => window.viewer().use_tool(ViewerTool::Info),
                2 => window.viewer().use_tool(ViewerTool::Pan),
                3 => { window.viewer().clear_selected_features()?; report(window,"[]")?; }
                4 => window.viewer().set_view_extent(Extent { x_min:-130.0, y_min:22.0, x_max:-65.0, y_max:55.0 })?,

                5 => tolerance=control.number,
                _ => {}
            }
        }
        while let Ok(event) = events.try_recv() {
            if event.data.event_type == 20 && event.data.int_value == 2 && event.data.int_value2 == 1 {
                click(window,event.data.screen_rectangle.left as f64,event.data.screen_rectangle.top as f64,tolerance)?;
            }

        }

    }
    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("WorldTolerance","world_4326/world_4326.shp",run)
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
