mod bootstrap;
use geokernel::{Extent,ViewerWindow};
use runtime::Result;
use serde_json::{json,Value};

fn calculate(window:&mut ViewerWindow,tolerance:f64)->Result<()>
{
    let line=[[-4.5,0.0],[4.5,0.0]];
    let value:Value=serde_json::from_str(&window.viewer().get_line_point_tolerance_info_json(&line,0.0,0.35,tolerance)?)?;
    window.viewer().clear_shapes()?;
    window.viewer().add_polyline_shape(&line,&json!({"lineColor":"#2F80C2","lineWidth":3}).to_string())?;
    if tolerance>0.0 { window.viewer().add_point_buffer_shape(0.0,0.35,tolerance,18,&json!({"fillColor":"#F9C74F","fillOpacity":80,"lineColor":"#D95D39"}).to_string())?; }
    window.viewer().add_point_shape(0.0,0.35,&json!({"pointColor":"#D95D39","pointSize":12}).to_string())?;
    if value.as_object().is_none_or(serde_json::Map::is_empty) { return Err("Tolerance diagnostics missing".into()); }
    window.clear_log()?;
    window.append_log(&format!("<pre>{}</pre>",serde_json::to_string_pretty(&value)?))?;
    runtime::refresh(window)?;
    Ok(())
}

fn run(window:&mut ViewerWindow,_path:&str)->Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("ToleranceConfig result")?;
    let events=window.add_control_panel(&json!({"title":"ToleranceConfig","controls":[
        {"id":1,"type":"button","text":"Recalculate"},
        {"id":2,"type":"button","text":"Full Extent"}, {"id":3,"type":"number","label":"Tolerance","minimum":0,"maximum":2,"step":0.1,"value":0.4}
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
    runtime::main("ToleranceConfig","",run)
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
