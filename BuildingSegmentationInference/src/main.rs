mod bootstrap;
use geokernel::{Runtime,ViewerWindow};
use runtime::Result;
use serde_json::{json,Value};
use std::{path::{Path,PathBuf},thread,time::{Duration,SystemTime,UNIX_EPOCH}};

fn find_manifest(path:&Path)->Result<PathBuf>
{
    if path.join("geokernel-model.json").is_file() {return Ok(path.to_owned());}
    for entry in std::fs::read_dir(path)?
    {
        let path=entry?.path();
        if path.is_dir()
        {
            if let Ok(found)=find_manifest(&path) {return Ok(found);}
        }
    }
    Err("Model manifest not found; run run.ps1 to download the model package".into())
}

fn inference(path:&str,model:&str,provider:&str,output:&Path)->Result<(String,String)>
{
    // Native service ownership stays on this worker thread.
    let mut sdk=unsafe {Runtime::from_env()?};
    sdk.set_text_capacity(16*1024*1024)?;
    let mut analysis=sdk.analysis()?;
    let request=json!({"modelPackagePath":model,"rasterPath":path,"provider":provider,"labelRasterPath":output.join("labels.tif"),"targetCrs":"EPSG:4326","validateGeometries":true,"repairInvalidGeometries":true});
    let mut layer=match analysis.run_ai_instance_vectorization(&request.to_string())
    {
        Ok(layer)=>layer,
        Err(error)=>return Err(format!("{error}: {}",analysis.last_error()?).into())
    };
    let report=layer.layer_diagnostics_json()?;
    let features=layer.layer_features_json()?;
    std::fs::write(output.join("diagnostics.json"),&report)?;
    std::fs::write(output.join("features-wkb.json"),&features)?;
    Ok((features,report))
}

fn load_input(window:&mut ViewerWindow,path:&str)->Result<()>
{
    window.viewer().clear_shapes()?;
    window.viewer().clear_layers()?;
    if !window.viewer().add_layer_file(path)? {return Err("Input raster could not be loaded".into());}
    // Clearing layers resets the viewer CRS; the raster then supplies its UTM CRS.
    // Inference polygons use targetCrs EPSG:4326, so restore it after loading.
    if !window.viewer().set_coordinate_system_preset("EPSG:4326")? {return Err("Could not set the result coordinate system".into());}
    window.viewer().zoom_to_layer(0)?;
    Ok(())
}

fn draw(window:&mut ViewerWindow,text:&str)->Result<usize>
{
    let data:Value=serde_json::from_str(text)?;
    let hexes=data["geometryWkbHex"].as_array().ok_or("Missing inference geometry")?;
    let style=json!({"fillColor":"#33AA66","fillOpacity":80,"lineColor":"#FFCC22","lineWidth":2}).to_string();
    let view=window.viewer().view_extent()?;
    let mut visible_count=0;
    for value in hexes
    {
        let hex=value.as_str().ok_or("Invalid WKB")?;
        if !hex.len().is_multiple_of(2) {return Err("Invalid WKB hex length".into());}
        let bytes=(0..hex.len()).step_by(2).map(|i|u8::from_str_radix(&hex[i..i+2],16)).collect::<std::result::Result<Vec<_>,_>>()?;
        let geometry:Value=serde_json::from_str(&window.viewer().read_wkb_geometry_json(&bytes)?)?;
        let parts=geometry["parts"].as_array().ok_or("Missing parts")?.iter().map(|ring|
            ring.as_array().ok_or("Invalid ring")?.iter().map(|p|Ok([p["x"].as_f64().ok_or("Missing x")?,p["y"].as_f64().ok_or("Missing y")?])).collect::<Result<Vec<_>>>()
        ).collect::<Result<Vec<_>>>()?;
        if parts.iter().flatten().any(|p|p[0]>=view.x_min && p[0]<=view.x_max && p[1]>=view.y_min && p[1]<=view.y_max) {visible_count+=1;}
        if !window.viewer().add_polygon_parts_shape(&parts,&style)? {return Err("Could not add an inferred polygon".into());}
    }
    if !hexes.is_empty() && visible_count==0 {return Err("Inference polygons do not overlap the raster view; check coordinate systems".into());}
    println!("Materialized {} inferred objects",hexes.len());
    Ok(hexes.len())
}

fn run(window:&mut ViewerWindow,path:&str)->Result<()>
{
    let default=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/buildings-model");
    let model=find_manifest(&default)?;
    let mut model=model.to_string_lossy().into_owned();
    let mut path=path.to_owned();
    let providers=["auto","cpu","cuda","directml","coreml"];
    let mut provider=0;
    window.add_navigation_toolbar();
    window.add_log_panel("Inference diagnostics")?;
    let controls=window.add_control_panel(&json!({"title":"BuildingSegmentationInference","controls":[
        {"id":1,"type":"text","label":"Input raster","value":path},
        {"id":2,"type":"text","label":"Model package","value":model},
        {"id":3,"type":"combo","label":"Provider","options":providers,"value":0},
        {"id":4,"type":"button","text":"Run inference"},
        {"id":5,"type":"button","text":"Clear result"},
        {"id":6,"type":"button","text":"Full extent"}

    ]}).to_string())?;
    load_input(window,&path)?;
    runtime::show(window)?;
    runtime::pump(window,100);
    window.viewer().zoom_to_layer(0)?;
    let mut start = false;
    loop
    {
        window.process_events();
        while let Ok(event)=controls.try_recv()
        {
            match event.id
            {
                1=>path=event.text,
                2=>model=event.text,
                3=>provider=(event.number as usize).min(providers.len()-1),
                4=>start=true,
                5=>
                {
                    load_input(window,&path)?;
                    runtime::refresh(window)?;
                }
                6=>{window.viewer().full_extent()?;},

                _=>{}
            }
        }
        if start
        {
            start=false;
            let output=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("outputs").join(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis().to_string());
            std::fs::create_dir_all(&output)?;
            load_input(window,&path)?;
            window.set_status_text("Inference running...")?;
            let input=path.clone();let package=model.clone();let selected=providers[provider];
            let worker=thread::spawn(move||inference(&input,&package,selected,&output).map_err(|e|e.to_string()));
            let started=std::time::Instant::now();
            while !worker.is_finished()
            {
                window.process_events();
                window.set_status_text(&format!("Inference running: {} s",started.elapsed().as_secs()))?;
                thread::sleep(Duration::from_millis(30));
            }
            // Join before closing the Qt application, including when its window was closed.
            match worker.join().map_err(|_|"Inference worker panicked")?
            {
                Ok((result,report))=>
                {
                    window.clear_log()?;
                    window.append_log(&format!("<pre>{}</pre>",report.replace('&',"&amp;").replace('<',"&lt;")))?;
                    let count=draw(window,&result)?;
                    runtime::refresh(window)?;
                    window.viewer().zoom_to_layer(0)?;
                    window.set_status_text(&format!("Inference completed: {count} building polygons"))?;

                }
                Err(error)=>{window.set_status_text(&error)?;}
            }
        }
        if !window.is_visible()? {break;}

    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("BuildingSegmentationInference","buildings/buildings.tif",run)
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
