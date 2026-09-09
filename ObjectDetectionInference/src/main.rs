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
    Err("Model manifest not found in the model package".into())
}

fn inference(path:&str,model:&str,provider:&str,output:&Path)->Result<(String,String)>
{
    // Native service ownership stays on this worker thread.
    let mut sdk=unsafe {Runtime::from_env()?};
    sdk.set_text_capacity(16*1024*1024)?;
    let mut analysis=sdk.analysis()?;
    let request=json!({"modelPackagePath":model,"rasterPath":path,"provider":provider,"windowSize":512,"overlap":256,"nmsThreshold":0.3,"layerName":"detections"});
    let mut layer=match analysis.run_ai_object_detection(&request.to_string())
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

fn draw(window:&mut ViewerWindow,text:&str)->Result<()>
{
    let data:Value=serde_json::from_str(text)?;
    let hexes=data["geometryWkbHex"].as_array().ok_or("Missing inference geometry")?;
    let style=json!({"fillColor":"#33AA66","fillOpacity":80,"lineColor":"#FFCC22","lineWidth":2}).to_string();
    for value in hexes
    {
        let hex=value.as_str().ok_or("Invalid WKB")?;
        if !hex.len().is_multiple_of(2) {return Err("Invalid WKB hex length".into());}
        let bytes=(0..hex.len()).step_by(2).map(|i|u8::from_str_radix(&hex[i..i+2],16)).collect::<std::result::Result<Vec<_>,_>>()?;
        let geometry:Value=serde_json::from_str(&window.viewer().read_wkb_geometry_json(&bytes)?)?;
        let parts=geometry["parts"].as_array().ok_or("Missing parts")?.iter().map(|ring|
            ring.as_array().ok_or("Invalid ring")?.iter().map(|p|Ok([p["x"].as_f64().ok_or("Missing x")?,p["y"].as_f64().ok_or("Missing y")?])).collect::<Result<Vec<_>>>()
        ).collect::<Result<Vec<_>>>()?;
        window.viewer().add_polygon_parts_shape(&parts,&style)?;
    }
    println!("Materialized {} inferred objects",hexes.len());
    Ok(())
}

fn sample_images(directory:&Path)->Result<Vec<PathBuf>>
{
    let mut images=Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let path=entry?.path();
        if path.is_file() && path.extension().is_some_and(|e|matches!(e.to_string_lossy().to_lowercase().as_str(),"jpg"|"jpeg"|"png"|"tif"|"tiff")) {
            images.push(path.canonicalize()?);
        }
    }
    images.sort_by_key(|p|(p.file_stem().and_then(|s|s.to_str()).and_then(|s|s.parse::<u32>().ok()).unwrap_or(u32::MAX),p.file_name().unwrap().to_owned()));
    if images.is_empty() {return Err("No supported sample images found".into());}
    Ok(images)
}

fn show_details(window:&mut ViewerWindow,text:&str)->Result<()>
{
    window.clear_log()?;
    window.append_log(&text.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('\n',"<br>"))?;
    Ok(())
}

fn select_image(window:&mut ViewerWindow,images:&[PathBuf],index:usize)->Result<String>
{
    let image=images.get(index).ok_or("Invalid image index")?;
    let path=image.to_str().ok_or("Invalid image path")?.to_owned();
    window.viewer().clear_shapes()?;
    window.viewer().clear_layers()?;
    if !window.viewer().add_layer_file(&path)? {return Err("Input image could not be loaded".into());}
    window.viewer().full_extent()?;
    window.set_control_value(1,0.0,&path)?;
    window.set_control_value(9,0.0,&image.file_name().unwrap().to_string_lossy())?;
    window.set_control_enabled(7,index>0)?;
    window.set_control_enabled(8,index+1<images.len())?;
    window.set_control_value(10,0.0,"%p%")?;
    let position=format!("{} / {} - {}",index+1,images.len(),image.file_name().unwrap().to_string_lossy());
    show_details(window,&format!("{position}\n\nImage selected. Run inference to create its detection overlay."))?;
    window.set_status_text(&position)?;
    Ok(path)
}

fn run(window:&mut ViewerWindow,path:&str)->Result<()>
{
    let default=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/object_detection_model");
    let mut model=find_manifest(&default)?.to_string_lossy().into_owned();
    let images=sample_images(Path::new(path).parent().ok_or("Input has no parent directory")?)?;
    let names:Vec<_>=images.iter().map(|p|p.file_name().unwrap().to_string_lossy().into_owned()).collect();
    let mut current=0;
    let mut provider=String::from("auto");
    window.add_navigation_toolbar();
    let controls=window.add_control_panel(&json!({"title":"GeoKernel AI","area":"right","width":420,"controls":[
        {"id":2,"type":"text","label":"Model package","value":model},
        {"id":1,"type":"text","label":"Input raster","value":path},
        {"id":3,"type":"combo","label":"Execution provider","options":["Auto","CPU","CUDA","DirectML"],"value":"Auto"},
        {"id":9,"type":"combo","label":"Sample image","options":names,"value":names[0]},
        {"id":7,"type":"button","text":"Previous image"},
        {"id":8,"type":"button","text":"Next image"},
        {"id":4,"type":"button","text":"Run object detection inference"},
        {"id":10,"type":"progress","value":0,"format":"%p%"},
        {"id":5,"type":"button","text":"Clear result"}
    ]}).to_string())?;
    window.add_log_panel("Inference diagnostics")?;
    runtime::show(window)?;
    let mut path=select_image(window,&images,current)?;
    let mut start=false;
    while window.is_visible()?
    {
        window.process_events();
        while let Ok(event)=controls.try_recv()
        {
            let next=match event.id {
                7=>current.checked_sub(1),
                8=>if current+1<images.len() {Some(current+1)} else {None},
                9=>names.iter().position(|name|name==&event.text).filter(|index|*index!=current),
                _=>None,
            };
            if let Some(index)=next {current=index;path=select_image(window,&images,current)?;continue;}
            match event.id {
                1=>if event.text!=path && Path::new(&event.text).is_file() {
                    path=event.text;
                    window.viewer().clear_shapes()?;window.viewer().clear_layers()?;
                    if !window.viewer().add_layer_file(&path)? {return Err("Input image could not be loaded".into());}
                    window.viewer().full_extent()?;
                    show_details(window,"Input image changed. Run inference to create its detection overlay.")?;
                },
                2=>model=event.text,
                3=>provider=event.text.to_lowercase(),
                4=>start=true,
                5=>{
                    let extent=window.viewer().view_extent()?;
                    window.viewer().clear_shapes()?;
                    window.viewer().set_view_extent(extent)?;
                    show_details(window,"Detection overlay cleared.")?;
                },
                _=>{}
            }
        }
        if start && window.is_visible()?
        {
            start=false;
            let output=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("outputs").join(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis().to_string());
            std::fs::create_dir_all(&output)?;
            for id in [1,2,3,4,5,7,8,9] {window.set_control_enabled(id,false)?;}
            window.viewer().clear_shapes()?;
            window.set_control_value(10,0.0,"Running inference...")?;
            show_details(window,"Running NWPU-VHR-10 object detection...")?;
            let input=path.clone();let package=model.clone();let selected=provider.clone();
            let worker=thread::spawn(move||inference(&input,&package,&selected,&output).map_err(|e|e.to_string()));
            let started=std::time::Instant::now();
            while !worker.is_finished() {
                window.process_events();
                if window.is_visible()? {window.set_status_text(&format!("Inference running: {} s",started.elapsed().as_secs()))?;}
                thread::sleep(Duration::from_millis(30));
            }
            let outcome=worker.join().map_err(|_|"Inference worker panicked")?;
            if !window.is_visible()? {break;}
            for id in [1,2,3,4,5,9] {window.set_control_enabled(id,true)?;}
            window.set_control_enabled(7,current>0)?;window.set_control_enabled(8,current+1<images.len())?;
            match outcome {
                Ok((result,report))=>{
                    let extent=window.viewer().view_extent()?;
                    draw(window,&result)?;
                    window.viewer().set_view_extent(extent)?;
                    let d:Value=serde_json::from_str(&report)?;
                    show_details(window,&format!("GeoKernel AI object detection\n\nImage: {}\nRequested provider: {provider}\n\nDetections: {}\nSkipped: {}\nTiles: {}\nElapsed (including materialization): {} ms\n\nVector output: in-memory overlay\n\nWarnings: {}",
                        Path::new(&path).file_name().unwrap().to_string_lossy(),d["materializedCount"],d["skippedCount"],d["tilesProcessed"],started.elapsed().as_millis(),d["warnings"]))?;
                    window.set_control_value(10,100.0,"%p% - Inference complete")?;
                    window.set_status_text(&format!("{} detections - {}",d["materializedCount"],Path::new(&path).file_name().unwrap().to_string_lossy()))?;
                },
                Err(error)=>{show_details(window,&format!("Inference failed:\n{error}"))?;window.set_control_value(10,0.0,"Inference failed")?;}
            }
        }
    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("ObjectDetectionInference","object_detection_images/1.jpg",run)
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
                    ViewerWindow::new(&runtime, name, 1280, 820)?
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
