mod bootstrap;
use geokernel::{Extent, ViewerTool, ViewerWindow};
use runtime::Result;
use serde_json::{json, Value};

fn report(window: &mut ViewerWindow, signals: &[String]) -> Result<()>
{
    let hits: Vec<Value> = serde_json::from_str(&window.viewer().get_selected_features_json()?)?;
    let mut text = format!("Selected: {}\nClick: add | Ctrl+Click: toggle\n\nSelection set\n", hits.len());
    if hits.is_empty() { text.push_str("No selected features.\n"); }
    for (index, hit) in hits.iter().enumerate() {
        let name = ["NAME", "Name", "CITY_NAME", "STATE", "STATE_NAME", "ADMIN"]
            .iter().filter_map(|key| hit["attributes"][key].as_str())
            .find(|value| !value.trim().is_empty()).unwrap_or("-");
        text.push_str(&format!("{}. {} | Feature {} | {}\n", index + 1,
            hit["layerName"].as_str().unwrap_or("-"), hit["featureId"], name));
    }
    text.push_str("\nselectionChanged(int count) log\nElapsed time since opening\n");
    for signal in signals { text.push_str(signal); text.push('\n'); }
    let escaped = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    window.clear_log()?;
    window.append_log(&format!("<pre>{escaped}</pre>"))?;
    window.set_status_text(&format!("Selected: {} | Signal: selectionChanged(int count)", hits.len()))?;
    Ok(())
}

fn click(window: &mut ViewerWindow, x: f64, y: f64, modifiers: i32) -> Result<()>
{
    let found = if modifiers & 0x04000000 != 0 {
        window.viewer().toggle_top_feature_selection_at(x, y, 8)?
    } else {
        window.viewer().add_top_feature_to_selection_at(x, y, 8)?
    };
    if !found { window.set_status_text("No feature hit")?; }
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
    window.add_log_panel("SelectionSignal")?;
    let controls = window.add_control_panel(&json!({"title":"SelectionSignal","controls":[
        {"id":1,"type":"button","text":"Select"},
        {"id":2,"type":"button","text":"Pan"},
        {"id":3,"type":"button","text":"Clear Selection"},
        {"id":4,"type":"button","text":"Full Extent"}
    ]}).to_string())?;
    let events = window.subscribe_events();
    let started = std::time::Instant::now();
    let mut signals = Vec::new();
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
    window.viewer().set_view_extent(Extent { x_min:-130.0, y_min:22.0, x_max:-65.0, y_max:55.0 })?;

    report(window, &signals)?;
    while window.is_visible()? {
        window.process_events();
        while let Ok(control) = controls.try_recv() {
            match control.id {
                1 => window.viewer().use_tool(ViewerTool::Info),
                2 => window.viewer().use_tool(ViewerTool::Pan),
                3 => window.viewer().clear_selected_features()?,
                4 => window.viewer().set_view_extent(Extent { x_min:-130.0, y_min:22.0, x_max:-65.0, y_max:55.0 })?,

                _ => {}
            }
        }
        while let Ok(event) = events.try_recv() {
            if event.data.event_type == 20 && event.data.int_value == 2 && event.data.int_value2 == 1 {
                click(window, event.data.screen_rectangle.left as f64, event.data.screen_rectangle.top as f64, event.data.double_value as i32)?;
            }
            if event.data.event_type == 11 {
                signals.push(format!("{:.3}s  selectionChanged({})", started.elapsed().as_secs_f64(), event.data.int_value));
                report(window, &signals)?;
            }

        }

    }
    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("SelectionSignal","world_4326/world_4326.shp",run)
}

mod runtime
{
    use geokernel::{Runtime, ViewerWindow};

    use std::{
        error::Error,
        path::PathBuf,
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

}
