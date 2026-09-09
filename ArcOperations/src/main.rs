mod bootstrap;
use geokernel::{Extent,ViewerWindow};
use runtime::Result;
use serde_json::{json,Value};

fn run(window:&mut ViewerWindow,_path:&str)->Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Arc operations")?;
    let query=[[-5.2,2.2],[-3.2,2.2]];
    let candidates=vec![vec![[-1.8,2.7],[0.4,2.7]],query.to_vec()];
    let base=[[-5.2,0.2],[-3.6,0.2],[-2.6,0.8]];
    let continuation=vec![vec![[-2.6,0.8],[-1.1,0.1],[0.4,0.4]]];
    let arc=[[-5.2,-2.0],[-1.0,-2.0]];
    let cutters=vec![vec![[-3.1,-3.0],[-3.1,-1.0]]];
    let found=window.viewer().find_matching_arc_index(&query,&candidates)?;
    if found!=1 { return Err("ArcFind returned the wrong candidate".into()); }
    for line in [&query[..],&candidates[0],&base[..],&continuation[0],&arc[..],&cutters[0]] {
        window.viewer().add_polyline_shape(line,&json!({"lineColor":"#6C757D","lineWidth":2}).to_string())?;
    }
    window.append_log(&format!("ArcFind: candidate {found}"))?;
    for (operation,value) in [("ArcMakeConnected",window.viewer().arc_make_connected_json(&base,&continuation)?),("ArcSplitOnCross",window.viewer().arc_split_on_cross_json(&arc,&cutters)?)] {
        let result:Value=serde_json::from_str(&value)?;
        let parts=result.as_array().ok_or("Expected polyline parts")?;
        if parts.is_empty() { return Err("Arc operation returned no geometry".into()); }
        for part in parts {
            let points:Vec<[f64;2]>=part.as_array().ok_or("Expected points")?.iter().map(|p|Ok([p["x"].as_f64().ok_or("Missing x")?,p["y"].as_f64().ok_or("Missing y")?])).collect::<Result<_>>()?;
            window.viewer().add_polyline_shape(&points,&json!({"lineColor":"#D95D39","lineWidth":4}).to_string())?;
        }
        window.append_log(&format!("<b>{operation}</b><pre>{value}</pre>"))?;
    }
    runtime::show(window)?;
    runtime::pump(window,100);
    window.viewer().set_view_extent(Extent{x_min:-7.0,y_min:-4.0,x_max:2.0,y_max:4.0})?;

    while window.is_visible()? { window.process_events();  }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("ArcOperations","",run)
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
