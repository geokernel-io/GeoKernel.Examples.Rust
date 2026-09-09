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
    let mut ai=sdk.ai()?;
    let target=output.join("class-mask.tif");
    let preview=output.join("color-preview.tif");
    let colors=[[10,0,100,0],[20,255,187,34],[30,255,255,76],[40,240,150,255],[50,250,0,0],[60,180,180,180],[70,240,240,240],[80,0,100,200],[90,0,150,160],[95,0,207,117],[100,250,230,160]];
    let palette:Vec<_>=colors.iter().map(|c|json!({"code":c[0],"red":c[1],"green":c[2],"blue":c[3]})).collect();
    let request=json!({"rasterPath":path,"modelPackagePath":model,"provider":provider,"bands":[1,2,3,4],"outputMode":"classMask","outputPath":target,"applyManifestClassCodes":true,"previewOutputPath":preview,"classPalette":palette});
    let result=ai.run_raster_inference(&request.to_string())?;
    let value:Value=serde_json::from_str(&result)?;
    if value["error"].is_string() {return Err(result.into());}
    if !target.is_file() || !preview.is_file() || value["classCodesApplied"]!=true {return Err(format!("No classified mask/color preview produced: {result}").into());}
    std::fs::write(output.join("diagnostics.json"),&result)?;
    Ok((preview.to_string_lossy().into_owned(),serde_json::to_string_pretty(&value)?))
}

fn show_details(window:&mut ViewerWindow,text:&str)->Result<()>
{
    let text=text.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('\n',"<br>");
    window.clear_log()?;
    window.append_log(&format!("{text}<br><br><b>ESA WorldCover classes</b><br><font color='#006400'>●</font> Tree cover &nbsp; <font color='#ffbb22'>●</font> Shrubland &nbsp; <font color='#ffff4c'>●</font> Grassland<br><font color='#f096ff'>●</font> Cropland &nbsp; <font color='#fa0000'>●</font> Built-up &nbsp; <font color='#b4b4b4'>●</font> Bare / sparse<br><font color='#0064c8'>●</font> Permanent water"))?;
    Ok(())
}

fn diagnostics(report:&str)->Result<String>
{
    let value:Value=serde_json::from_str(report)?;
    let model=&value["provenance"]["model"];
    Ok(format!("GeoKernel AI land-cover inference\n\nModel: {} {}\nProvider: {}\nRaster: {} x {}\nTiles: {}\nElapsed: {} ms\n\nClass mask:\n{}\n\nColor preview:\n{}",
        model["id"].as_str().unwrap_or(""),model["version"].as_str().unwrap_or(""),value["provider"].as_str().unwrap_or(""),
        value["width"],value["height"],value["processedTiles"],value["elapsedMilliseconds"],
        value["outputPath"].as_str().unwrap_or(""),value["previewOutputPath"].as_str().unwrap_or("")))
}

fn open_input(window:&mut ViewerWindow,path:&str)->Result<()>
{
    window.viewer().clear_layers()?;
    if !window.viewer().add_layer_file(path)? {return Err("Input raster could not be loaded".into());}
    window.viewer().full_extent()?;
    Ok(())
}

fn apply_prediction_opacity(window:&mut ViewerWindow,opacity:f64)->Result<()>
{
    let extent=window.viewer().view_extent()?;
    if !window.viewer().set_layer_opacity(0,opacity/100.0)? {return Err("Prediction opacity could not be applied".into());}
    // The published opacity setter invalidates the cache but does not schedule a frame.
    window.viewer().set_view_extent(extent)?;
    Ok(())
}

fn show_prediction(window:&mut ViewerWindow,path:&str,opacity:f64)->Result<()>
{
    let extent=window.viewer().view_extent()?;
    if window.viewer().get_layer_count()?>1 {window.viewer().remove_layer(0)?;}
    if !window.viewer().add_layer_file(path)? {return Err("Inference output could not be loaded".into());}
    if !window.viewer().set_layer_opacity(0,opacity/100.0)? {return Err("Prediction opacity could not be applied".into());}
    window.viewer().set_view_extent(extent)?;
    Ok(())
}

fn run(window:&mut ViewerWindow,path:&str)->Result<()>
{
    let default=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/landcover-bilbao-model");
    let mut model=find_manifest(&default)?.to_string_lossy().into_owned();
    let mut path=path.to_owned();
    let providers=["Auto","CPU","CUDA","DirectML"];
    let mut provider=String::from("auto");
    let mut opacity=50.0;
    let mut has_prediction=false;
    window.add_navigation_toolbar();
    let controls=window.add_control_panel(&json!({"title":"GeoKernel AI","area":"right","width":420,"controls":[
        {"id":2,"type":"text","label":"Model package","value":model},
        {"id":1,"type":"text","label":"Input raster","value":path},
        {"id":3,"type":"combo","label":"Execution provider","options":providers,"value":"Auto"},
        {"id":7,"type":"number","label":"Prediction opacity (%)","minimum":0,"maximum":100,"value":50,"enabled":false},
        {"id":4,"type":"button","text":"Run land-cover inference"},
        {"id":8,"type":"progress","value":0,"format":"%p%"},
        {"id":5,"type":"button","text":"Clear result","enabled":false}
    ]}).to_string())?;
    window.add_log_panel("Inference diagnostics")?;
    window.set_status_text("Map ready.")?;
    runtime::show(window)?;
    open_input(window,&path)?;
    show_details(window,"Bilbao input raster is open. Run land-cover inference to add the prediction layer.")?;
    let mut start=false;
    while window.is_visible()?
    {
        window.process_events();
        while let Ok(event)=controls.try_recv()
        {
            match event.id
            {
                1=>{
                    path=event.text;
                    if Path::new(&path).is_file() {
                        open_input(window,&path)?;has_prediction=false;
                        window.set_control_enabled(7,false)?;window.set_control_enabled(5,false)?;
                    }
                },
                2=>model=event.text,
                3=>provider=event.text.to_lowercase(),
                4=>start=true,
                5=>{
                    if has_prediction {window.viewer().remove_layer(0)?;has_prediction=false;}
                    window.set_control_enabled(7,false)?;window.set_control_enabled(5,false)?;
                    window.set_control_value(8,0.0,"%p%")?;
                    show_details(window,"Prediction cleared. The input raster remains open.")?;
                },
                7=>{
                    opacity=event.number.clamp(0.0,100.0);
                    if has_prediction {apply_prediction_opacity(window,opacity)?;}
                },
                _=>{}
            }
        }
        if start && window.is_visible()?
        {
            start=false;
            if !Path::new(&path).is_file() || !Path::new(&model).is_dir() {
                show_details(window,"Select an existing input raster and model package.")?;
                continue;
            }
            let output=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("outputs").join(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis().to_string());
            std::fs::create_dir_all(&output)?;
            for id in [1,2,3,4,5,7] {window.set_control_enabled(id,false)?;}
            window.set_control_value(8,0.0,"Running inference...")?;
            show_details(window,"Validating the model package and preparing tiled inference...")?;
            let input=path.clone();let package=model.clone();let selected=provider.clone();
            let worker=thread::spawn(move||inference(&input,&package,&selected,&output).map_err(|e|e.to_string()));
            let started=std::time::Instant::now();
            while !worker.is_finished()
            {
                window.process_events();
                if window.is_visible()? {window.set_status_text(&format!("Inference running: {} s",started.elapsed().as_secs()))?;}
                thread::sleep(Duration::from_millis(30));
            }
            let result=worker.join().map_err(|_|"Inference worker panicked")?;
            if !window.is_visible()? {break;}
            for id in [1,2,3,4] {window.set_control_enabled(id,true)?;}
            match result
            {
                Ok((preview,report))=>{
                    show_prediction(window,&preview,opacity)?;
                    has_prediction=true;
                    show_details(window,&diagnostics(&report)?)?;
                    window.set_control_value(8,100.0,"%p% - Inference complete")?;
                    window.set_status_text("Land-cover inference completed.")?;
                },
                Err(error)=>{
                    show_details(window,&format!("Inference failed:\n{error}"))?;
                    window.set_control_value(8,0.0,"Inference failed")?;
                    window.set_status_text("Inference failed")?;
                }
            }
            window.set_control_enabled(7,has_prediction)?;
            window.set_control_enabled(5,has_prediction)?;
        }
    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("LandCoverInference","bilbao_s2_rgbnir_2021/bilbao_s2_rgbnir_2021.tif",run)
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
