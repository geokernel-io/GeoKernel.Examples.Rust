mod bootstrap;
use geokernel::{Extent,ViewerWindow};
use runtime::Result;
use serde_json::{json,Value};

const PRESETS: &[&str] = &["{\n  \"type\": \"Point\",\n  \"coordinates\": [-122.4194, 37.7749]\n}","{\n  \"type\": \"LineString\",\n  \"coordinates\": [\n    [-122.4194, 37.7749],\n    [-121.8863, 37.3382],\n    [-121.4944, 38.5816],\n    [-120.7401, 37.6391]\n  ]\n}","{\n  \"type\": \"Polygon\",\n  \"coordinates\": [[\n    [-123.25, 37.15],\n    [-122.15, 36.95],\n    [-121.55, 37.65],\n    [-122.05, 38.35],\n    [-123.05, 38.15],\n    [-123.25, 37.15]\n  ]]\n}","{\n  \"type\": \"MultiPolygon\",\n  \"coordinates\": [\n    [[\n      [-123.25, 37.15],\n      [-122.25, 36.95],\n      [-121.85, 37.65],\n      [-122.45, 38.20],\n      [-123.15, 37.95],\n      [-123.25, 37.15]\n    ]],\n    [[\n      [-121.60, 36.75],\n      [-120.70, 36.70],\n      [-120.45, 37.35],\n      [-121.25, 37.65],\n      [-121.60, 36.75]\n    ]]\n  ]\n}"];

fn read(window:&mut ViewerWindow,input:&str)->Result<()>
{
    let value:Value=serde_json::from_str(&window.viewer().read_geo_json_geometry_json(input)?)?;
    let parts:Vec<Vec<[f64;2]>>=value["parts"].as_array().ok_or("Invalid geometry")?.iter().map(|ring| {
        ring.as_array().ok_or("Invalid coordinate array")?.iter().map(|p| {
            Ok([p["x"].as_f64().ok_or("Missing x")?,p["y"].as_f64().ok_or("Missing y")?])
        }).collect::<Result<_>>()
    }).collect::<Result<_>>()?;
    if parts.is_empty() || parts.iter().any(Vec::is_empty) { return Err("Empty geometry".into()); }
    let style=json!({"pointColor":"#D95D39","pointSize":14,"fillColor":"#F9C74F","fillOpacity":140,"lineColor":"#D95D39","lineWidth":3}).to_string();
    if parts.iter().flatten().any(|[x,y]| !x.is_finite() || !y.is_finite() || !(-180.0..=180.0).contains(x) || !(-85.05112878..=85.05112878).contains(y)) {
        return Err("OSM requires longitude between -180 and 180 and latitude between -85.05112878 and 85.05112878".into());
    }
    let projected: Vec<Vec<[f64;2]>>=parts.iter().map(|part| {
        part.iter().map(|[x,y]| {
            let (mx,my)=window.viewer().transform_point(4326,3857,*x,*y)?;
            Ok([mx,my])
        }).collect::<Result<_>>()
    }).collect::<Result<_>>()?;
    let mut extent=Extent{x_min:f64::INFINITY,y_min:f64::INFINITY,x_max:f64::NEG_INFINITY,y_max:f64::NEG_INFINITY};
    for [x,y] in projected.iter().flatten() {
        extent.x_min=extent.x_min.min(*x);
        extent.y_min=extent.y_min.min(*y);
        extent.x_max=extent.x_max.max(*x);
        extent.y_max=extent.y_max.max(*y);
    }
    let padding=((extent.x_max-extent.x_min).max(extent.y_max-extent.y_min)*0.15).max(1000.0);
    extent.x_min-=padding;
    extent.y_min-=padding;
    extent.x_max+=padding;
    extent.y_max+=padding;
    window.viewer().clear_shapes()?;
    match value["shapeClass"].as_str() {
        Some("Point")=>{ window.viewer().add_point_shape(projected[0][0][0],projected[0][0][1],&style)?; }
        Some("Polyline")=>{ for part in &projected { window.viewer().add_polyline_shape(part,&style)?; } }
        Some("Polygon")=>{ window.viewer().add_polygon_parts_shape(&projected,&style)?; }
        _=>return Err("Unsupported geometry".into())
    }
    window.clear_log()?;
    window.append_log(&format!("<pre>{}</pre>",serde_json::to_string_pretty(&value)?))?;
    window.viewer().set_view_extent(extent)?;
    runtime::refresh(window)?;
    Ok(())
}

fn run(window:&mut ViewerWindow,_path:&str)->Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Decoded geometry")?;
    let controls=window.add_control_panel(&json!({"title":"GeoJsonRead","controls":[
        {"id":1,"type":"combo","label":"Preset","options":["Point","LineString","Polygon","MultiPolygon"],"value":"Point"},
        {"id":2,"type":"text","label":"GeoJSON","value":PRESETS[0]},
        {"id":3,"type":"button","text":"Read GeoJSON"}
    ]}).to_string())?;
    window.viewer().set_coordinate_system_preset("EPSG:3857")?;
    if window.viewer().add_open_street_map_layer(true)? < 0 {
        return Err("Could not load OpenStreetMap basemap".into());
    }
    window.set_attribution_text("© OpenStreetMap contributors")?;
    let mut input=PRESETS[0].to_owned();
    runtime::show(window)?;
    read(window,&input)?;

    while window.is_visible()? {
        window.process_events();
        while let Ok(event)=controls.try_recv() {
            match event.id {
                1=>{ if let Some(preset)=["Point","LineString","Polygon","MultiPolygon"].iter().position(|name| *name==event.text).and_then(|index| PRESETS.get(index)) { input=(*preset).into(); window.set_control_value(2,0.0,&input)?; read(window,&input)?; } }
                2=>input=event.text,
                3=>{ if let Err(error)=read(window,&input) { window.set_status_text(&error.to_string())?; } }
                _=>{}
            }
        }

    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("GeoJsonRead","",run)
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
