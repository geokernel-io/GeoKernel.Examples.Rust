mod bootstrap;
use geokernel::{Extent,ViewerTool,ViewerWindow};
use runtime::Result;
use serde_json::{json,Value};

fn coordinates(value:&Value)->Result<Vec<[f64;2]>>
{
    value.as_array().ok_or("Expected coordinates")?.iter().map(|point| {
        Ok([point["x"].as_f64().ok_or("Missing x")?,point["y"].as_f64().ok_or("Missing y")?])
    }).collect()
}

fn setup(window:&mut ViewerWindow)->Result<()>
{
    window.viewer().set_coordinate_system_preset("EPSG:4326")?;
    let polygon:Value=serde_json::from_str(&window.viewer().read_wkt_polygon_json("POLYGON((-123.25 37.15,-122.15 36.95,-121.55 37.65,-122.05 38.35,-123.05 38.15,-123.25 37.15))",false)?)?;
    let line:Value=serde_json::from_str(&window.viewer().read_wkt_line_string_json("LINESTRING(-123.0 37.1,-122.5 37.8,-121.9 37.3,-121.2 38.0)")?)?;
    let (x,y)=window.viewer().read_wkt_point("POINT(-122.4194 37.7749)")?;
    for (name,kind,style) in [
        ("WKT Polygons",5,json!({"fillColor":"#D18A80","fillOpacity":136,"lineColor":"#1F7A4D","lineWidth":2.2})),
        ("WKT Lines",3,json!({"lineColor":"#E4572E","lineWidth":3})),
        ("WKT Points",1,json!({"pointColor":"#D95D39","lineColor":"#8C321D","pointSize":12}))
    ] {
        let index=window.viewer().add_empty_vector_layer(name,kind,&style.to_string())?;
        if index<0 || !window.viewer().begin_edit_layer(index)? { return Err("Could not create WKT layer".into()); }
        let added=match kind {
            5=>window.viewer().add_polygon_to_edit_layer(index,&coordinates(&polygon[0])?)?,
            3=>window.viewer().add_polyline_to_edit_layer(index,&coordinates(&line)?)?,
            _=>window.viewer().add_point_to_edit_layer(index,x,y)?
        };
        if !added || !window.viewer().commit_edit_layer(index)? { return Err("Could not populate WKT layer".into()); }
    }
    window.viewer().use_tool(ViewerTool::Pan);
    window.add_log_panel("WktOverlay")?;
    window.append_log("WktOverlay sample<br><br>API<br>read_wkt_point / read_wkt_line_string_json / read_wkt_polygon_json<br>add_empty_vector_layer<br><br>Three WKT strings are parsed and displayed as overlay layers.")?;
    window.set_status_text("WktOverlay ready.")?;
    Ok(())
}

fn run(window:&mut ViewerWindow,_path:&str)->Result<()>
{
    setup(window)?;
    runtime::show(window)?;
    window.viewer().set_view_extent(Extent{x_min:-124.0,y_min:36.4,x_max:-120.3,y_max:38.7})?;
    while window.is_visible()? { window.process_events(); }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("WktOverlay","",run)
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


}
