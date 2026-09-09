mod bootstrap;
use geokernel::{Extent, ViewerWindow};
use runtime::Result;
use serde_json::{json};

fn calculate(window: &mut ViewerWindow) -> Result<()>
{
    window.viewer().clear_shapes()?;
    let inputs=[vec![[-5.0,-1.6], [-2.0,-1.6], [-2.0,1.4], [-5.0,1.4], [-5.0,-1.6]],vec![[0.0,-1.6], [3.3,1.4], [0.0,1.4], [3.3,-1.6], [0.0,-1.6]]];
    let mut results=Vec::new();
    for (i,ring) in inputs.iter().enumerate() {
        let valid=window.viewer().check_polygon_ring(ring)?;
        if valid!=(i==0) { return Err("Unexpected validity result".into()); }
        let color=if valid {"#2A9D8F"} else {"#D95D39"};
        window.viewer().add_polygon_shape(ring,&json!({"fillColor":color,"fillOpacity":125,"lineColor":color,"lineWidth":4}).to_string())?;
        results.push(json!({"polygon":i,"valid":valid}));
    }
    window.clear_log()?;
    window.append_log(&format!("<pre>{}</pre>",serde_json::to_string_pretty(&results)?))?;
    runtime::refresh(window)?;
    Ok(())
}

fn run(window: &mut ViewerWindow, _path: &str) -> Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Result coordinates")?;
    let events = window.add_control_panel(&json!({"title":"TopologyCheck","controls":[
        {"id":1,"type":"button","text":"Recalculate"},
        {"id":2,"type":"button","text":"Full Extent"}
    ]}).to_string())?;
    calculate(window)?;
    runtime::show(window)?;
    runtime::pump(window, 100);
    window.viewer().set_view_extent(Extent { x_min:-6.0, y_min:-4.0, x_max:6.0, y_max:4.0 })?;

    while window.is_visible()? {
        window.process_events();
        while let Ok(event) = events.try_recv() {
            match event.id {
                1 => calculate(window)?,
                2 => window.viewer().set_view_extent(Extent { x_min:-6.0, y_min:-4.0, x_max:6.0, y_max:4.0 })?,
                _ => {}
            }
        }

    }
    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("TopologyCheck", "", run)
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
