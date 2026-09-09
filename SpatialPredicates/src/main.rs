mod bootstrap;
use geokernel::{Extent,ViewerWindow};
use runtime::Result;
use serde_json::{json,Value};

fn run(window:&mut ViewerWindow,_path:&str)->Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Spatial predicate cases")?;
    for (name,left,right) in [("contains","POLYGON((-8.2 3.5,-4.8 3.5,-4.8 6.3,-8.2 6.3,-8.2 3.5))","POLYGON((-7.4 4.1,-5.7 4.1,-5.7 5.5,-7.4 5.5,-7.4 4.1))"),("within","POLYGON((-2.8 4.1,-1.1 4.1,-1.1 5.5,-2.8 5.5,-2.8 4.1))","POLYGON((-3.6 3.5,-0.2 3.5,-0.2 6.3,-3.6 6.3,-3.6 3.5))"),("touch","POLYGON((1.2 3.6,3.4 3.6,3.4 6.1,1.2 6.1,1.2 3.6))","POLYGON((3.4 3.6,5.6 3.6,5.6 6.1,3.4 6.1,3.4 3.6))"),("overlap","POLYGON((-8.2 -2,-5 -2,-5 0.8,-8.2 0.8,-8.2 -2))","POLYGON((-6.3 -0.8,-3.1 -0.8,-3.1 2,-6.3 2,-6.3 -0.8))"),("cross","LINESTRING(-2.9 -1.7,0.4 1.6)","LINESTRING(-2.9 1.6,0.4 -1.7)"),("disjoint","POLYGON((1.4 -2,3 -2,3 -0.2,1.4 -0.2,1.4 -2))","POLYGON((4.2 0.2,5.8 0.2,5.8 2,4.2 2,4.2 0.2))")] {
        let result:Value=serde_json::from_str(&window.viewer().evaluate_spatial_relation_json(left,right)?)?;
        if result[name]!=true { return Err(format!("Expected {name} to be true: {result}").into()); }
        window.viewer().add_wkt_shape(left)?;
        window.viewer().add_wkt_shape(right)?;
        window.append_log(&format!("<pre>{}</pre>",json!({"predicate":name,"result":result[name],"matrix":result["matrix"]})))?;
    }
    runtime::show(window)?;
    runtime::pump(window,100);
    window.viewer().set_view_extent(Extent{x_min:-10.0,y_min:-4.0,x_max:8.0,y_max:8.0})?;

    while window.is_visible()? { window.process_events();  }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("SpatialPredicates","",run)
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
