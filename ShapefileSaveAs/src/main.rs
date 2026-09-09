mod bootstrap;
use geokernel::{ViewerTool, ViewerWindow};
use runtime::Result;
use serde_json::{json, Value};

fn details(window: &mut ViewerWindow) -> Result<()>
{
    window.clear_log()?;
    let info: Value = serde_json::from_str(&window.viewer().get_layer_info_json(0)?)?;
    log_json(window, "Layer metadata", &info)?;
    let schema: Value = serde_json::from_str(&window.viewer().get_layer_attribute_definitions_json(0)?)?;
    log_json(window, "Attribute schema", &schema)?;
    let count = window.viewer().get_layer_feature_count(0)?;
    for row in 0..count.min(12) {
        let attributes: Value = serde_json::from_str(&window.viewer().get_layer_feature_attributes_json(0, row)?)?;
        log_json(window, &format!("Feature {}", row + 1), &attributes)?;
    }
    window.set_status_text(&format!("Loaded {count} features; showing the first 12 attribute rows."))?;
    Ok(())
}

fn log_json(window: &mut ViewerWindow, title: &str, value: &Value) -> Result<()>
{
    let text = serde_json::to_string_pretty(value)?.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    window.append_log(&format!("<b>{title}</b><pre>{text}</pre>"))?;
    Ok(())
}

fn save(window:&mut ViewerWindow)->Result<()>
{
    let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let directory=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("outputs").join(stamp.to_string());
    std::fs::create_dir_all(&directory)?;
    let file=directory.join("world_copy.shp");
    let count=window.viewer().get_layer_feature_count(0)?;
    if !window.viewer().save_layer_as_shapefile(0,&file.to_string_lossy())? { return Err("Save failed".into()); }
    for extension in ["shp","shx","dbf"] { if !file.with_extension(extension).is_file() { return Err("Incomplete saved Shapefile".into()); } }
    if !window.viewer().add_layer_file(&file.to_string_lossy())? { return Err("Saved file could not be reopened".into()); }
    let actual=window.viewer().get_layer_feature_count(0)?;
    window.viewer().remove_layer(0)?;
    if actual!=count { return Err("Saved feature count differs".into()); }
    window.set_status_text(&format!("Saved and reopened: {}",file.display()))?;
    Ok(())
}

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Layer metadata and attributes")?;
    let controls = window.add_control_panel(&json!({"title":"ShapefileSaveAs", "controls":[
        {"id":1,"type":"button","text":"Full Extent"},
        {"id":2,"type":"button","text":"Refresh metadata"},{"id":3,"type":"button","text":"Save As Shapefile"}
    ]}).to_string())?;
    {
        let mut viewer = window.viewer();
        viewer.use_tool(ViewerTool::Pan);
        if !viewer.add_layer_file(path)? { return Err(format!("Could not load {path}").into()); }
        viewer.set_layer_name(0, "ShapefileSaveAs")?;
        if viewer.get_layer_count()? < 1 { return Err("No layer was loaded".into()); }
    }
    details(window)?;
    runtime::show(window)?;
    // Fit after the dock/control panels and the native viewport have their final sizes.
    runtime::pump(window, 100);
    if !window.viewer().zoom_to_layer(0)? { return Err("Could not fit the layer extent".into()); }

    while window.is_visible()? {
        window.process_events();
        while let Ok(event) = controls.try_recv() {
            match event.id {
                1 => { window.viewer().zoom_to_layer(0)?; }
                2 => details(window)?,
                3 => save(window)?,
                _ => {}
            }
        }

    }
    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("ShapefileSaveAs", "world_4326/world_4326.shp", run)
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
