mod bootstrap;
use geokernel::ViewerWindow;
use runtime::Result;
use serde_json::{json,Value};

fn resolve(window:&mut ViewerWindow,authority:&str,code:i32)->Result<()>
{
    let definition=format!("{authority}:{code}");
    let result:Value=serde_json::from_str(&window.viewer().get_coordinate_system_info_json(&definition)?)?;
    if let Some(error)=result["error"].as_str() { return Err(error.to_owned().into()); }
    if result["name"].as_str().is_none_or(str::is_empty) { return Err("CRS name missing".into()); }
    window.clear_log()?;
    let text=serde_json::to_string_pretty(&result)?.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;");
    window.append_log(&format!("<pre>{text}</pre>"))?;
    window.set_status_text(&definition)?;
    Ok(())
}

fn run(window:&mut ViewerWindow,_path:&str)->Result<()>
{
    window.add_log_panel("CRS details — GDAL/PROJ")?;
    let controls=window.add_control_panel(&json!({"title":"CrsByAuthority","controls":[
        {"id":1,"type":"text","label":"Authority (EPSG, ESRI, IGNF)","value":"EPSG"},
        {"id":2,"type":"number","label":"Code","minimum":1,"maximum":999999,"value":32635},
        {"id":3,"type":"button","text":"Resolve"}
    ]}).to_string())?;
    let mut authority=String::from("EPSG");
    let mut code=32635;
    resolve(window,&authority,code)?;
    runtime::show(window)?;

    while window.is_visible()? {
        window.process_events();
        while let Ok(event)=controls.try_recv() {
            match event.id {
                1=>authority=event.text.trim().to_uppercase(),
                2=>code=event.number as i32,
                3=>{ if let Err(error)=resolve(window,&authority,code) { window.set_status_text(&error.to_string())?; } }
                _=>{}
            }
        }

    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("CrsByAuthority","",run)
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
