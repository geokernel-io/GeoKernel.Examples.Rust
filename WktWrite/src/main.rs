mod bootstrap;
use geokernel::{Extent,ViewerTool,ViewerWindow};
use runtime::Result;
use serde_json::json;

fn prepare(window:&mut ViewerWindow,mode:i32)->Result<()>
{
    window.viewer().clear_layers()?;
    let kind=[1,3,5][mode as usize];
    if window.viewer().add_empty_vector_layer("Draw geometry",kind,&json!({"pointColor":"#D95D39","pointSize":12,"fillColor":"#F9C74F","fillOpacity":140,"lineColor":"#D95D39","lineWidth":3}).to_string())? < 0 { return Err("Could not create layer".into()); }
    if !window.viewer().begin_edit_layer(0)? { return Err("Could not begin edit session".into()); }
    window.viewer().set_active_edit_layer_index(0)?;
    window.viewer().use_tool([ViewerTool::AddPoint,ViewerTool::AddPolyline,ViewerTool::AddPolygon][mode as usize]);
    Ok(())
}

fn output(window:&mut ViewerWindow)->Result<String>
{
    Ok(window.viewer().write_layer_last_shape_wkt(0)?)
}

fn run(window:&mut ViewerWindow,_path:&str)->Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("WKT output")?;
    let controls=window.add_control_panel(&json!({"title":"WktWrite","controls":[
        {"id":1,"type":"combo","label":"Geometry","options":["Point","Polyline","Polygon"],"value":0},
        {"id":2,"type":"button","text":"Clear"},
        {"id":3,"type":"button","text":"Write WKT"}
    ]}).to_string())?;
    let mut mode=0;
    prepare(window,mode)?;
    runtime::show(window)?;
    runtime::pump(window,100);
    window.viewer().set_view_extent(Extent{x_min:-124.0,y_min:36.0,x_max:-119.0,y_max:40.0})?;

    let mut last=String::new();
    let mut tick=std::time::Instant::now();
    while window.is_visible()? {
        window.process_events();
        while let Ok(event)=controls.try_recv() {
            match event.id {
                1=>{ mode=event.number as i32; prepare(window,mode)?; last.clear(); window.clear_log()?; },
                2=>{ prepare(window,mode)?; last.clear(); window.clear_log()?; },
                3=>{ last.clear(); tick=std::time::Instant::now()-std::time::Duration::from_secs(1); },
                _=>{}
            }
        }
        if tick.elapsed()>=std::time::Duration::from_millis(200) && window.viewer().get_layer_feature_count(0)?>0 {
            let value=output(window)?;
            if value!=last {
                window.clear_log()?;
                let escaped=value.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;");
                window.append_log(&format!("<pre>{escaped}</pre>"))?;
                last=value;
            }
            tick=std::time::Instant::now();
        }

    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("WktWrite","",run)
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
