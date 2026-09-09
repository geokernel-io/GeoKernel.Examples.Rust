mod bootstrap;
use geokernel::ViewerWindow;
use runtime::Result;
use serde_json::{json,Value};

fn load(window:&mut ViewerWindow,path:&str,mode:i32)->Result<()>
{
    // Overviews are created on a fresh working copy, never on the shared sample.
    let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
    let directory=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("outputs").join(stamp.to_string());
    std::fs::create_dir_all(&directory)?;
    let copy=directory.join("working.tif");
    std::fs::copy(path,&copy)?;
    window.viewer().clear_layers()?;
    let options=json!({"prepareRasterOverviews":true,"rasterOverviewMinimumPixels":0,"rasterTileCacheEnabled":mode!=0,"rasterTileCachePixelBudget":([0,131072,4194304][mode as usize]),"rasterTileCacheMaximumItemPixels":([0,131072,262144][mode as usize])});
    if !window.viewer().add_layer_file_with_options_json(&copy.to_string_lossy(),&options.to_string())? { return Err("Raster load failed".into()); }
    window.viewer().zoom_to_layer(0)?;
    Ok(())
}

fn diagnostics(window:&mut ViewerWindow,benchmark:bool,clear:bool)->Result<()>
{
    let _=clear;
    let value:Value=serde_json::from_str(&window.viewer().get_raster_tile_cache_diagnostics_json(0,benchmark,clear)?)?;
    if value.as_object().is_none_or(serde_json::Map::is_empty) { return Err("Raster diagnostics missing".into()); }
    window.clear_log()?;
    let text=serde_json::to_string_pretty(&value)?.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;");
    window.append_log(&format!("<pre>{text}</pre>"))?;
    Ok(())
}

fn run(window:&mut ViewerWindow,path:&str)->Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Raster diagnostics and benchmark")?;
    let controls=window.add_control_panel(&json!({"title":"RasterTileCache","controls":[
        {"id":1,"type":"combo","label":"Mode","options":["Disabled","Small cache","Large cache"],"value":0},
        {"id":2,"type":"button","text":"Load / Reset working copy"},
        {"id":3,"type":"button","text":"Run Benchmark"},
        {"id":4,"type":"button","text":"Refresh / Clear Cache"}
    ]}).to_string())?;
    let mut mode=0;
    load(window,path,mode)?;
    runtime::show(window)?;
    runtime::pump(window,100);
    window.viewer().zoom_to_layer(0)?;
    diagnostics(window,false,false)?;

    while window.is_visible()? {
        window.process_events();
        while let Ok(event)=controls.try_recv() {
            match event.id {
                1=>mode=event.number as i32,
                2=>{ load(window,path,mode)?; diagnostics(window,false,false)?; },
                3=>diagnostics(window,true,false)?,
                4=>diagnostics(window,false,true)?,
                _=>{}
            }
        }

    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("RasterTileCache","world_8km_tif/world_8km.tif",run)
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
