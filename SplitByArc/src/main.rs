mod bootstrap;
use geokernel::{Extent,ViewerWindow};
use runtime::Result;
use serde_json::{json,Value};

const LEFT: &[[f64;2]] = &[[-4.0, -2.0], [3.8, -2.0], [4.5, 0.5], [2.5, 2.4], [-1.5, 2.1], [-4.4, 0.6], [-4.0, -2.0]];
const RIGHT: &[[f64;2]] = &[[-5.2, 1.4], [-1.8, 0.7], [0.2, -0.2], [2.0, -0.6], [5.1, -1.0]];

fn points(value:&Value)->Result<Vec<[f64;2]>>
{
    value.as_array().ok_or("Expected coordinates")?.iter().map(|p|Ok([p["x"].as_f64().ok_or("Missing x")?,p["y"].as_f64().ok_or("Missing y")?])).collect()
}

fn calculate(window:&mut ViewerWindow,tolerance:f64)->Result<()>
{
    let _=tolerance;
    window.viewer().clear_shapes()?;
    let style=json!({"fillColor":"#BFD7EA","fillOpacity":100,"lineColor":"#2F80C2","lineWidth":2}).to_string();
    window.viewer().add_polygon_shape(LEFT,&style)?;
    window.viewer().add_polyline_shape(RIGHT,&json!({"lineColor":"#D95D39","lineWidth":3}).to_string())?;
    let value:Value=serde_json::from_str(&window.viewer().split_polygon_by_arc_json(LEFT,RIGHT)?)?;
    let rings=value.as_array().ok_or("Expected split polygons")?;
    if rings.len()<2 { return Err("Polygon was not split".into()); }
    for (i,ring) in rings.iter().enumerate() {
        window.viewer().add_polygon_shape(&points(ring)?,&json!({"fillColor":(["#F9C74F","#CDE7D8","#A7D8F0"][i%3]),"fillOpacity":155,"lineColor":"#D95D39","lineWidth":3}).to_string())?;
    }
    window.clear_log()?;
    window.append_log(&format!("<pre>{}</pre>",serde_json::to_string_pretty(&value)?))?;
    runtime::refresh(window)?;
    Ok(())
}

fn run(window:&mut ViewerWindow,_path:&str)->Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("SplitByArc result")?;
    let events=window.add_control_panel(&json!({"title":"SplitByArc","controls":[
        {"id":1,"type":"button","text":"Recalculate"},
        {"id":2,"type":"button","text":"Full Extent"}
    ]}).to_string())?;
    let mut tolerance=0.4;
    calculate(window,tolerance)?;
    runtime::show(window)?;
    runtime::pump(window,100);
    window.viewer().set_view_extent(Extent{x_min:-8.0,y_min:-4.0,x_max:8.0,y_max:4.0})?;

    while window.is_visible()? {
        window.process_events();
        while let Ok(event)=events.try_recv() {
            match event.id {
                1=>calculate(window,tolerance)?,
                2=>window.viewer().set_view_extent(Extent{x_min:-8.0,y_min:-4.0,x_max:8.0,y_max:4.0})?,
                3=>{ tolerance=event.number; calculate(window,tolerance)?; },
                _=>{}
            }
        }

    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("SplitByArc","",run)
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
