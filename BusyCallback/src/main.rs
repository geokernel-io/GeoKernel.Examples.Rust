mod bootstrap;
use geokernel::ViewerWindow;
use runtime::Result;
use serde_json::json;

fn run(window:&mut ViewerWindow,path:&str)->Result<()>
{
    window.add_log_panel("Busy and load events")?;
    if !window.add_cancellable_layer_load_toolbar(&json!({"filePath":path,"layerName":"One Million Points"}).to_string())? { return Err("Could not create load controls".into()); }
    let events=window.subscribe_events();
    runtime::show(window)?;

    while window.is_visible()? {
        window.process_events();
        for event in events.try_iter() {
            if matches!(event.data.event_type,3|15|23|25|26|27|29) {
                let text=event.text.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;");
                window.append_log(&format!("Event {}: value={}, value2={}, progress={} {text}",event.data.event_type,event.data.int_value,event.data.int_value2,event.data.double_value))?;
            }
        }

    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("BusyCallback","output_1m_points/output_1m_points.shp",run)
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
