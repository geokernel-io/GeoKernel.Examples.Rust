mod bootstrap;
use geokernel::{Extent, ViewerWindow};
use runtime::Result;
use serde_json::{json};
const DEFAULT: &str = "POINT(-122.4194 37.7749)";

fn read(window: &mut ViewerWindow, input: &str) -> Result<()>
{
    let (x,y)=window.viewer().read_wkt_point(input)?;
    if !x.is_finite() || !y.is_finite() || !(-180.0..=180.0).contains(&x) || !(-85.05112878..=85.05112878).contains(&y) {
        return Err("OSM requires longitude between -180 and 180 and latitude between -85.05112878 and 85.05112878".into());
    }
    let (mx,my)=window.viewer().transform_point(4326,3857,x,y)?;
    let output=window.viewer().write_wkt_point(x,y)?;
    window.viewer().clear_shapes()?;
    window.viewer().add_point_shape(mx,my,&json!({"pointColor":"#D95D39","pointSize":14}).to_string())?;
    let escaped = output.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;");
    window.clear_log()?;
    window.append_log(&format!("<b>WKT output</b><pre>{escaped}</pre>"))?;
    window.viewer().set_view_extent(Extent{x_min:mx-25000.0,y_min:my-25000.0,x_max:mx+25000.0,y_max:my+25000.0})?;
    runtime::refresh(window)?;
    Ok(())
}

fn run(window: &mut ViewerWindow, _path: &str) -> Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Parsed geometry")?;
    let controls = window.add_control_panel(&json!({"title":"WktReadPoint","controls":[
        {"id":1,"type":"text","label":"WKT","value":DEFAULT},
        {"id":2,"type":"button","text":"Read WKT"},
        {"id":3,"type":"button","text":"Reset"}
    ]}).to_string())?;
    window.viewer().set_coordinate_system_preset("EPSG:3857")?;
    if window.viewer().add_open_street_map_layer(true)? < 0 {
        return Err("Could not load OpenStreetMap basemap".into());
    }
    window.set_attribution_text("© OpenStreetMap contributors")?;
    let mut input=DEFAULT.to_owned();
    read(window,&input)?;
    runtime::show(window)?;
    read(window,&input)?;

    while window.is_visible()? {
        window.process_events();
        while let Ok(event)=controls.try_recv() {
            match event.id {
                1 => input=event.text,
                2 => { if let Err(error)=read(window,&input) { window.set_status_text(&error.to_string())?; } }
                3 => { input=DEFAULT.to_owned(); window.set_control_value(1,0.0,&input)?; read(window,&input)?; }
                _ => {}
            }
        }

    }
    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("WktReadPoint","",run)
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

    pub fn refresh(window: &mut ViewerWindow) -> Result<()>
    {
        window.viewer().invalidate_render_cache(true, true)?;
        window.viewer().refresh_layers()?;
        Ok(())
    }
}
