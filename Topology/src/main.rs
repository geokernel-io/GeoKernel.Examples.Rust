mod bootstrap;
use geokernel::{Extent,ViewerWindow};
use runtime::Result;
use serde_json::{json,Value};

const LEFT:&str="POLYGON((-4 -1.4,0.7 -1.4,0.7 2,-4 2,-4 -1.4))";
const RIGHT:&str="POLYGON((-1 -2.1,3.9 -2.1,3.9 1.3,-1 1.3,-1 -2.1))";

fn evaluate(window:&mut ViewerWindow,left:&str,right:&str)->Result<()>
{
    let result:Value=serde_json::from_str(&window.viewer().evaluate_spatial_relation_json(left,right)?)?;
    if let Some(error)=result["error"].as_str() { return Err(error.to_owned().into()); }
    if result["matrix"].as_str().is_none_or(|m|m.len()!=9) { return Err("Invalid DE-9IM matrix".into()); }
    window.viewer().clear_shapes()?;
    window.viewer().add_wkt_shape(left)?;
    window.viewer().add_wkt_shape(right)?;
    window.clear_log()?;
    window.append_log(&format!("<pre>{}</pre>",serde_json::to_string_pretty(&result)?))?;
    runtime::refresh(window)?;
    Ok(())
}

fn boolean_operations(window:&mut ViewerWindow)->Result<()>
{
    let a=[[-4.0,-1.4],[0.7,-1.4],[0.7,2.0],[-4.0,2.0],[-4.0,-1.4]];
    let b=[[-1.0,-2.1],[3.9,-2.1],[3.9,1.3],[-1.0,1.3],[-1.0,-2.1]];
    for (name,result) in [("Union",window.viewer().union_polygons_json(&a,&b)?),("Intersection",window.viewer().intersection_polygons_json(&a,&b)?),("Difference",window.viewer().difference_polygons_json(&a,&b)?)] {
        let value:Value=serde_json::from_str(&result)?;
        if value.as_array().is_none_or(Vec::is_empty) { return Err("Empty topology result".into()); }
        window.append_log(&format!("<b>{name}</b><pre>{result}</pre>"))?;
    }
    Ok(())
}

fn run(window:&mut ViewerWindow,_path:&str)->Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Spatial relation and predicates")?;
    let controls=window.add_control_panel(&json!({"title":"Topology","controls":[
        {"id":1,"type":"text","label":"Left WKT","value":LEFT},
        {"id":2,"type":"text","label":"Right WKT","value":RIGHT},
        {"id":3,"type":"button","text":"Evaluate"},{"id":4,"type":"button","text":"Union / Intersection / Difference"}
    ]}).to_string())?;
    let (mut left,mut right)=(LEFT.to_owned(),RIGHT.to_owned());
    evaluate(window,&left,&right)?;
    runtime::show(window)?;
    runtime::pump(window,100);
    window.viewer().set_view_extent(Extent{x_min:-6.0,y_min:-4.0,x_max:6.0,y_max:4.0})?;

    while window.is_visible()? {
        window.process_events();
        while let Ok(event)=controls.try_recv() {
            match event.id {
                1=>left=event.text,
                2=>right=event.text,
                3=>{ if let Err(error)=evaluate(window,&left,&right) { window.set_status_text(&error.to_string())?; } }
                4=>boolean_operations(window)?,
                _=>{}
            }
        }

    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("Topology","",run)
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

    pub fn refresh(window: &mut ViewerWindow) -> Result<()>
    {
        window.viewer().invalidate_render_cache(true, true)?;
        window.viewer().refresh_layers()?;
        Ok(())
    }
}
