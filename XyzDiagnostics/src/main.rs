mod bootstrap;
use geokernel::{Extent, ViewerWindow};
use runtime::Result;
use serde_json::{json, Value};

struct Settings
{
    url: String,
    min: i32,
    max: i32,
    cache: bool,
}

fn apply(window: &mut ViewerWindow, settings: &Settings) -> Result<()>
{
    if settings.min > settings.max { return Err("Minimum zoom must not exceed maximum zoom".into()); }
    let cache = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/xyz_cache/XyzDiagnostics");
    std::fs::create_dir_all(&cache)?;
    window.viewer().clear_layers()?;
    if window.viewer().add_xyz_layer("XyzDiagnostics",&settings.url,settings.min,settings.max,256,"",settings.cache,&cache.to_string_lossy())? < 0 { return Err("Could not add XYZ layer".into()); }
    window.viewer().set_coordinate_system_preset("EPSG:3857")?;
    window.set_attribution_text("© OpenStreetMap contributors (default source); other sources retain their own attribution")?;
    Ok(())
}

fn diagnostics(window: &mut ViewerWindow) -> Result<()>
{
    let value: Value=serde_json::from_str(&window.viewer().get_xyz_layer_diagnostics_json(0)?)?;
    if value.as_object().is_none_or(serde_json::Map::is_empty) { return Err("XYZ diagnostics unavailable".into()); }
    window.clear_log()?;
    let text=serde_json::to_string_pretty(&value)?.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;");
    window.append_log(&format!("<pre>{text}</pre>"))?;
    Ok(())
}

fn run(window: &mut ViewerWindow, _path: &str) -> Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("XYZ diagnostics")?;
    let controls=window.add_control_panel(&json!({"title":"XyzDiagnostics","controls":[

        {"id":5,"type":"button","text":"Apply"},
        {"id":6,"type":"button","text":"Refresh Stats"},
        {"id":7,"type":"button","text":"Full Extent"}
    ]}).to_string())?;
    let mut settings=Settings {url:"https://tile.openstreetmap.org/{z}/{x}/{y}.png".into(),min:0,max:19,cache:true};
    apply(window,&settings)?;
    runtime::show(window)?;
    runtime::pump(window,100);
    window.viewer().set_view_extent(Extent {x_min:-1400000.0,y_min:4100000.0,x_max:4200000.0,y_max:7800000.0})?;

    while window.is_visible()? {
        window.process_events();
        while let Ok(event)=controls.try_recv() {
            match event.id {
                1 => settings.url=event.text,
                2 => settings.min=event.number as i32,
                3 => settings.max=event.number as i32,
                4 => settings.cache=event.number!=0.0,
                5 => { if let Err(error)=apply(window,&settings) { window.set_status_text(&error.to_string())?; } },
                6 => diagnostics(window)?,
                7 => window.viewer().set_view_extent(Extent {x_min:-1400000.0,y_min:4100000.0,x_max:4200000.0,y_max:7800000.0})?,

                _ => {}
            }
        }

    }
    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("XyzDiagnostics","",run)
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
