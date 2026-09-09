mod bootstrap;
use geokernel::{Runtime,ViewerWindow};
use runtime::Result;
use serde_json::{json,Value};
use base64::Engine;

fn draw(window:&mut ViewerWindow,geometries:&[Vec<u8>])->Result<()>
{
    window.viewer().clear_shapes()?;
    let style=json!({"fillColor":"#65B8E8","lineColor":"#176B9C","lineWidth":0.8}).to_string();
    for bytes in geometries
    {
        let value:Value=serde_json::from_str(&window.viewer().read_wkb_geometry_json(bytes)?)?;
        let parts=value["parts"].as_array().ok_or("Missing geometry parts")?.iter().map(|ring|
            ring.as_array().ok_or("Invalid ring")?.iter().map(|p|Ok([p["x"].as_f64().ok_or("Missing x")?,p["y"].as_f64().ok_or("Missing y")?])).collect::<Result<Vec<_>>>()
        ).collect::<Result<Vec<_>>>()?;
        window.viewer().add_polygon_parts_shape(&parts,&style)?;
    }
    runtime::refresh(window)?;
    window.viewer().set_view_extent(geokernel::Extent{x_min:18.04,y_min:59.30,x_max:18.10,y_max:59.35})?;
    Ok(())
}

fn compare(path:&str,class:&str,limit:usize)->Result<(Vec<Vec<u8>>,String)>
{
    let mut sdk=unsafe {Runtime::from_env()?};
    sdk.set_text_capacity(16*1024*1024)?;
    let mut duck=sdk.duckdb()?;
    let mut connection=duck.create_connection(":memory:")?;
    let metadata:Value=serde_json::from_str(&connection.inspect_geo_parquet_json(path)?)?;
    connection.query_json("SELECT count(*) FROM read_parquet(?)",&json!([path]).to_string())?;
    let begin=std::time::Instant::now();
    let (mut offset,mut matched,mut transferred)=(0,0,0);
    // Transfer every row in bounded pages, then apply the predicate in Rust.
    // LIMIT here bounds transport memory, not the number of source rows examined.
    loop
    {
        let text=connection.query_json("SELECT id,class,geometry,bbox.xmin,bbox.ymin,bbox.xmax,bbox.ymax FROM read_parquet(?) LIMIT 4096 OFFSET ?",&json!([path,offset]).to_string())?;
        transferred+=text.len();
        let page:Value=serde_json::from_str(&text)?;
        let rows=page["rows"].as_array().ok_or("Missing rows")?;
        for row in rows
        {
            if matched<limit && row[1].as_str()==Some(class) && row[5].as_f64().unwrap_or(f64::NEG_INFINITY)>=18.04 && row[3].as_f64().unwrap_or(f64::INFINITY)<=18.10 && row[6].as_f64().unwrap_or(f64::NEG_INFINITY)>=59.30 && row[4].as_f64().unwrap_or(f64::INFINITY)<=59.35 {matched+=1;}
        }
        offset+=rows.len();
        if rows.len()<4096 {break;}
    }
    let full_ms=begin.elapsed().as_millis();
    let begin=std::time::Instant::now();
    let request=json!({"columns":["id","class","geometry"],"extent":{"xMin":18.04,"yMin":59.30,"xMax":18.10,"yMax":59.35},"predicateSql":"class = ?","predicateParameters":[class],"limit":limit});
    let text=connection.query_geo_parquet_json(path,&request.to_string())?;
    let pushed_bytes=text.len();
    let value:Value=serde_json::from_str(&text)?;
    let rows=value["rows"].as_array().ok_or("Missing filtered rows")?;
    if rows.len()!=matched {return Err(format!("Filter mismatch: Rust={matched}, DuckDB={}",rows.len()).into());}
    let push_ms=begin.elapsed().as_millis();
    let geometries=rows.iter().map(|row|->Result<Vec<u8>>
    {
        Ok(base64::engine::general_purpose::STANDARD.decode(row[2]["$binary"].as_str().ok_or("Missing WKB binary")?)?)
    }).collect::<Result<Vec<_>>>()?;
    let report=json!({"filter":{"class":class,"limit":limit},"dataset":metadata,"fullTransfer":{"rows":offset,"matched":matched,"jsonBytes":transferred,"elapsedMs":full_ms,"columns":7,"pageSize":4096},"pushdown":{"rows":rows.len(),"jsonBytes":pushed_bytes,"elapsedMs":push_ms,"columns":3},"note":"Timings include JSON transport. Full transfer is paged to bound memory; both paths use the same warmed connection."});
    Ok((geometries,serde_json::to_string_pretty(&report)?))
}

fn run(window:&mut ViewerWindow,path:&str)->Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Full transfer vs predicate / BBOX / projection pushdown")?;
    window.viewer().set_coordinate_system_preset("EPSG:4326")?;
    let classes=["apartments","house","commercial","industrial"];
    let controls=window.add_control_panel(&json!({"title":"DuckDbGeoParquetAnalytics","controls":[
        {"id":1,"type":"combo","label":"Building class","options":classes,"value":"apartments"},
        {"id":2,"type":"number","label":"Maximum results","minimum":1,"maximum":100000,"value":25000},
        {"id":3,"type":"button","text":"Compare both paths"}
    ]}).to_string())?;
    let (mut class,mut limit)=(String::from("apartments"),25000);
    let mut start = false;
    runtime::show(window)?;
    loop
    {
        window.process_events();
        while let Ok(event)=controls.try_recv()
        {
            match event.id {1=>class=event.text,2=>limit=(event.number as usize).clamp(1,100000),3=>start=true,_=>{}}
        }
        if start
        {
            start=false;
            for id in [1,2,3] { window.set_control_enabled(id,false)?; }
            window.set_status_text(&format!("Comparing class '{class}': full dataset transfer and filtered query..."))?;
            let path=path.to_owned();
            let selected=class.clone();
            let worker=std::thread::spawn(move||compare(&path,&selected,limit).map_err(|e|e.to_string()));
            while !worker.is_finished() {window.process_events();std::thread::sleep(std::time::Duration::from_millis(20));}
            for id in [1,2,3] { window.set_control_enabled(id,true)?; }
            match worker.join().map_err(|_|"Analytics worker panicked")?
            {
                Ok((geometries,report))=>
                {
                    window.clear_log()?;
                    window.append_log(&format!("<pre>{report}</pre>"))?;
                    draw(window,&geometries)?;
                    window.set_status_text(&format!("{} buildings ({class}); both filter results agree",geometries.len()))?;

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
    runtime::main("DuckDbGeoParquetAnalytics","stockholm_data/stockholm_buildings.parquet",run)
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
