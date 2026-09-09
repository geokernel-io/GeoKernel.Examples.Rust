mod bootstrap;
use geokernel::{ViewerTool, ViewerWindow};
use runtime::Result;
use serde_json::{json,Value};

fn update(window:&mut ViewerWindow,x:f64,y:f64)->Result<()>
{
    let value:Value=serde_json::from_str(&window.viewer().get_raster_world_transform_json(0,x,y)?)?;
    let wx=value["worldX"].as_f64().ok_or("Missing world X")?;
    let wy=value["worldY"].as_f64().ok_or("Missing world Y")?;
    let rx=value["reversePixelX"].as_f64().ok_or("Missing reverse X")?;
    let ry=value["reversePixelY"].as_f64().ok_or("Missing reverse Y")?;
    if (rx-x).abs()>1e-6 || (ry-y).abs()>1e-6 { return Err("Pixel/world roundtrip failed".into()); }
    window.viewer().clear_shapes()?;
    window.viewer().add_point_shape(wx,wy,&json!({"pointColor":"#D95D39","pointSize":14}).to_string())?;
    window.clear_log()?;
    window.append_log(&format!("<pre>{}</pre>",serde_json::to_string_pretty(&value)?))?;
    window.set_status_text(&format!("Pixel ({x:.0}, {y:.0}) -> World ({wx:.6}, {wy:.6})"))?;
    runtime::refresh(window)?;
    Ok(())
}

fn run(window:&mut ViewerWindow,path:&str)->Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Pixel / world transform")?;
    if !window.viewer().add_layer_file(path)? { return Err("Raster load failed".into()); }
    let transform:Value=serde_json::from_str(&window.viewer().get_raster_world_transform_json(0,0.0,0.0)?)?;
    let width=transform["width"].as_u64().filter(|n| *n>0).ok_or("Invalid raster width")?;
    let height=transform["height"].as_u64().filter(|n| *n>0).ok_or("Invalid raster height")?;
    let (mut x,mut y)=((width/2) as f64,(height/2) as f64);
    let controls=window.add_control_panel(&json!({"title":"RasterWorldTransform","controls":[
        {"id":1,"type":"number","label":"Pixel X","minimum":0,"maximum":width-1,"decimals":0,"step":1,"value":x},
        {"id":2,"type":"number","label":"Pixel Y","minimum":0,"maximum":height-1,"decimals":0,"step":1,"value":y},
        {"id":3,"type":"button","text":"Pick Pixel"},
        {"id":4,"type":"button","text":"Pan"}
    ]}).to_string())?;
    let events=window.subscribe_events();
    window.viewer().use_tool(ViewerTool::Info);
    update(window,x,y)?;
    runtime::show(window)?;
    window.viewer().zoom_to_layer(0)?;

    while window.is_visible()? {
        window.process_events();
        let mut changed=false;
        while let Ok(event)=controls.try_recv() {
            match event.id {
                1 if x!=event.number => { x=event.number; changed=true; }
                2 if y!=event.number => { y=event.number; changed=true; }
                3 => window.viewer().use_tool(ViewerTool::Info),
                4 => window.viewer().use_tool(ViewerTool::Pan),
                _ => {}
            }
        }
        for event in events.try_iter() {
            let data=event.data;
            if data.event_type==20 && data.int_value==2 && data.int_value2==1 {
                let (wx,wy)=window.viewer().screen_to_world(data.screen_rectangle.left as f64,data.screen_rectangle.top as f64)?;
                let (px,py)=world_to_pixel(&transform,wx,wy)?;
                x=px.round().clamp(0.0,(width-1) as f64);
                y=py.round().clamp(0.0,(height-1) as f64);
                window.set_control_value(1,x,"")?;
                window.set_control_value(2,y,"")?;
                changed=true;
            }
        }
        if changed { update(window,x,y)?; }

    }
    Ok(())
}

fn world_to_pixel(transform:&Value,wx:f64,wy:f64)->Result<(f64,f64)>
{
    let number=|key:&str| transform[key].as_f64().ok_or_else(|| format!("Missing transform field: {key}"));
    let (a,b,c,d)=(number("pixelSizeX")?,number("rotationY")?,number("rotationX")?,number("pixelSizeY")?);
    let determinant=a*d-b*c;
    if !determinant.is_finite() || determinant==0.0 { return Err("Raster transform cannot be inverted".into()); }
    let dx=wx-number("upperLeftCenterX")?;
    let dy=wy-number("upperLeftCenterY")?;
    Ok(((dx*d-b*dy)/determinant,(a*dy-c*dx)/determinant))
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("RasterWorldTransform","world_8km_tif/world_8km.tif",run)
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
