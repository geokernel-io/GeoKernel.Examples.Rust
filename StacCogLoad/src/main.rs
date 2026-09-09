mod bootstrap;
use geokernel::{Extent,Runtime,ViewerWindow};
use runtime::Result;
use serde_json::{json,Value};
use std::{thread,time::Duration};

const DEFAULT_URL:&str="https://earth-search.aws.element84.com/v1";
const BBOX:[f64;4]=[18.00,59.25,18.20,59.40];

fn zoom_to_search_area(window:&mut ViewerWindow)->Result<()>
{
    let first_layer=window.viewer().get_layer_count()?-1;
    let metadata:Value=serde_json::from_str(&window.viewer().get_raster_world_transform_json(first_layer,0.0,0.0)?)?;
    let epsg=metadata["epsgCode"].as_i64().filter(|code|*code>0).ok_or("Raster CRS is unavailable")? as i32;
    let mut extent=Extent{x_min:f64::INFINITY,y_min:f64::INFINITY,x_max:f64::NEG_INFINITY,y_max:f64::NEG_INFINITY};
    for (x,y) in [(BBOX[0],BBOX[1]),(BBOX[0],BBOX[3]),(BBOX[2],BBOX[1]),(BBOX[2],BBOX[3])] {
        let (x,y)=window.viewer().transform_point(4326,epsg,x,y)?;
        extent.x_min=extent.x_min.min(x);extent.y_min=extent.y_min.min(y);
        extent.x_max=extent.x_max.max(x);extent.y_max=extent.y_max.max(y);
    }
    let dx=(extent.x_max-extent.x_min)*0.04;
    let dy=(extent.y_max-extent.y_min)*0.04;
    extent.x_min-=dx;extent.x_max+=dx;extent.y_min-=dy;extent.y_max+=dy;
    window.viewer().set_view_extent(extent)?;
    Ok(())
}

fn resolve(url:&str)->Result<(Vec<String>,String)>
{
    // Each worker owns its native handles; only plain data crosses threads.
    let mut sdk=unsafe {Runtime::from_env()?};
    sdk.set_text_capacity(16*1024*1024)?;
    let mut cloud=sdk.cloud()?;
    let cache=std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/cloud_cache/StacCogLoad");
    std::fs::create_dir_all(&cache)?;
    let mut client=cloud.create_client(&json!({"directory":cache,"memoryEnabled":true,"diskEnabled":true}).to_string())?;
    client.set_timeout(10000)?;
    let request=json!({"collections":["sentinel-2-l2a"],"bbox":BBOX,"datetime":"2024-01-01T00:00:00Z/..","limit":100,"query":{"eo:cloud_cover":{"lt":20}}});
    let result:Value=serde_json::from_str(&client.stac_search_json(url,&request.to_string())?)?;
    let mut tiles=std::collections::HashSet::new();
    let mut paths=Vec::new();
    let mut items=Vec::new();
    for item in result["items"].as_array().ok_or("STAC response has no items")?
    {
        let p=&item["properties"];
        let key=format!("{}:{}:{}",p["mgrs:utm_zone"],p["mgrs:latitude_band"],p["mgrs:grid_square"]);
        if tiles.contains(&key) {continue;}
        if let Some(href)=item["assets"]["visual"]["href"].as_str()
        {
            let probe:Value=serde_json::from_str(&client.cog_probe_json(href,"{}")?)?;
            paths.push(client.cog_gdal_virtual_path(href)?);
            items.push(json!({"id":item["id"],"url":href,"probe":probe}));
            tiles.insert(key);
        }
        if paths.len()>=16 {break;}
    }
    if paths.is_empty() {return Err("No visual COG assets matched the search".into());}
    Ok((paths,serde_json::to_string_pretty(&json!({"request":request,"tiles":items}))?))
}

fn load(window:&mut ViewerWindow,url:&str)->Result<()>
{
    window.set_status_text("Reading remote metadata...")?;
    let url=url.to_owned();
    let worker=thread::spawn(move||resolve(&url).map_err(|e|e.to_string()));
    while !worker.is_finished()
    {
        window.process_events();
        if !window.is_visible()? {return Ok(());}
        thread::sleep(Duration::from_millis(10));
    }
    let (paths,report)=worker.join().map_err(|_|"Cloud worker panicked")??;
    if !window.is_visible()? {return Ok(());}
    window.clear_log()?;
    window.append_log(&format!("<pre>{}</pre>",report.replace('&',"&amp;").replace('<',"&lt;")))?;
    window.viewer().clear_layers()?;
    for path in paths
    {
        if !window.is_visible()? {return Ok(());}
        let options=json!({"prepareRasterOverviews":false});
        if !window.viewer().add_layer_file_with_options_json(&path,&options.to_string())? {return Err(format!("GDAL could not open {path}").into());}
        window.process_events();
    }
    zoom_to_search_area(window)?;

    window.set_status_text("Remote layers opened")?;
    Ok(())
}

fn run(window:&mut ViewerWindow,_path:&str)->Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Cloud metadata")?;
    let controls=window.add_control_panel(&json!({"title":"StacCogLoad","controls":[
        {"id":1,"type":"text","label":"Source URL","value":DEFAULT_URL},
        {"id":2,"type":"button","text":"Load remote data"},
        {"id":3,"type":"button","text":"Zoom to search area"}
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
                2=>{
                    window.set_control_enabled(2,false)?;
                    let result=load(window,&url);
                    if window.is_visible()? {
                        window.set_control_enabled(2,true)?;
                        if let Err(error)=result {window.set_status_text(&error.to_string())?;}
                    }
                },
                3=>{zoom_to_search_area(window)?;},
                _=>{}
            }
        }

    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("StacCogLoad","",run)
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
