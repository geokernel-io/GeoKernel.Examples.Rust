mod bootstrap;
use geokernel::{Extent,ViewerWindow};
use runtime::Result;
use serde_json::{json,Value};

fn calculate(window:&mut ViewerWindow)->Result<()>
{
    let a=[-4.4,-1.8,0.8,1.8];
    let b=[-0.8,-0.6,4.2,2.6];
    let value:Value=serde_json::from_str(&window.viewer().get_extent_operations_json(&json!({"a":a,"b":b,"point":[-2.0,0.0],"inflate":[0.9,0.7]}).to_string())?)?;
    if value["intersects"]!=true || value["containsPoint"]!=true { return Err("Extent predicates failed".into()); }
    window.viewer().clear_shapes()?;
    for (label,rect,color) in [("A",json!(a),"#2F80C2"),("B",json!(b),"#D95D39"),("A.expand(B)",value["expanded"].clone(),"#2A9D8F"),("A.inflate(0.9,0.7)",value["inflated"].clone(),"#7048A8")] {
        let x=rect[0].as_f64().ok_or("Missing x")?;
        let y=rect[1].as_f64().ok_or("Missing y")?;
        let xx=rect[2].as_f64().ok_or("Missing xMax")?;
        let yy=rect[3].as_f64().ok_or("Missing yMax")?;
        window.viewer().add_polygon_shape(&[[x,y],[xx,y],[xx,yy],[x,yy],[x,y]],&json!({"fillColor":color,"fillOpacity":35,"lineColor":color,"lineWidth":2}).to_string())?;
        window.viewer().add_text_shape(x,y,label,"{}")?;
    }
    window.clear_log()?;
    window.append_log(&format!("<pre>{}</pre>",serde_json::to_string_pretty(&value)?))?;
    Ok(())
}

fn run(window:&mut ViewerWindow,_path:&str)->Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Extent operations")?;
    calculate(window)?;
    runtime::show(window)?;
    runtime::pump(window,100);
    window.viewer().set_view_extent(Extent{x_min:-6.0,y_min:-4.0,x_max:6.0,y_max:4.0})?;

    while window.is_visible()? { window.process_events();  }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("ExtentOperations","",run)
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
