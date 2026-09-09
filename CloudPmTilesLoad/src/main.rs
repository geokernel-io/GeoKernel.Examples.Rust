mod bootstrap;
use geokernel::{Runtime,ViewerWindow};
use runtime::Result;
use serde_json::{json,Value};
use std::{thread,time::Duration};

const DEFAULT_URL:&str="https://pmtiles.io/protomaps(vector)ODbL_firenze.pmtiles";

fn resolve(url:&str)->Result<(Vec<String>,String)>
{
    // Each worker owns its native handles; only plain data crosses threads.
    let mut sdk=unsafe {Runtime::from_env()?};
    sdk.set_text_capacity(16*1024*1024)?;
    let mut cloud=sdk.cloud()?;
    let cache=std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/cloud_cache/CloudPmTilesLoad");
    std::fs::create_dir_all(&cache)?;
    let mut client=cloud.create_client(&json!({"directory":cache,"memoryEnabled":true,"diskEnabled":true}).to_string())?;
    client.set_timeout(30000)?;
    let report=client.pm_tiles_inspect_json(url,"{}")?;
    let path=client.pm_tiles_gdal_virtual_path(url)?;
    Ok((vec![path],report))
}

fn load(window:&mut ViewerWindow,url:&str)->Result<()>
{
    window.set_status_text("Reading remote metadata...")?;
    let url=url.to_owned();
    let worker=thread::spawn(move||resolve(&url).map_err(|e|e.to_string()));
    while !worker.is_finished()
    {
        window.process_events();
        thread::sleep(Duration::from_millis(10));
    }
    let (paths,report)=worker.join().map_err(|_|"Cloud worker panicked")??;
    window.clear_log()?;
    window.append_log(&format!("<pre>{}</pre>",report.replace('&',"&amp;").replace('<',"&lt;")))?;
    window.viewer().clear_layers()?;
    for path in paths
    {
        let sources:Value=serde_json::from_str(&window.viewer().pm_tiles_source_layers_json(&path)?)?;
        let sources=sources.as_array().ok_or("Invalid PMTiles source-layer list")?;
        if sources.is_empty() {return Err("PMTiles has no vector source layers".into());}
        for source in sources
        {
            let index=source["index"].as_i64().ok_or("Missing source-layer index")?;
            let options=json!({"sourceLayerIndex":index});
            if !window.viewer().add_layer_file_with_options_json(&path,&options.to_string())? {return Err(format!("Could not open PMTiles source layer {index}").into());}
            window.append_log(&format!("Loaded source layer: {}",source["name"]))?;
            window.process_events();
        }
        window.process_events();
    }
    runtime::pump(window,100);
    window.viewer().full_extent()?;
    runtime::refresh(window)?;
    window.set_status_text("Remote layer loaded")?;
    Ok(())
}

fn run(window:&mut ViewerWindow,_path:&str)->Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Cloud metadata")?;
    let controls=window.add_control_panel(&json!({"title":"CloudPmTilesLoad","controls":[
        {"id":1,"type":"text","label":"Source URL","value":DEFAULT_URL},
        {"id":2,"type":"button","text":"Load remote data"},
        {"id":3,"type":"button","text":"Full extent"}
    ]}).to_string())?;
    let mut url=DEFAULT_URL.to_owned();
    runtime::show(window)?;

    while window.is_visible()?
    {
        window.process_events();
        while let Ok(event)=controls.try_recv()
        {
            match event.id
            {
                1=>url=event.text,
                2=>if let Err(error)=load(window,&url) {window.set_status_text(&error.to_string())?;},
                3=>{window.viewer().full_extent()?;},
                _=>{}
            }
        }

    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("CloudPmTilesLoad","",run)
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
