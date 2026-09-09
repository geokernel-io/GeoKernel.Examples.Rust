mod bootstrap;
use geokernel::{Runtime,ViewerWindow};
use runtime::Result;
use serde_json::{json,Value};

fn analysis_request(path:&str,class:&str,limit:i64)->String
{
    json!({"operation":"SpatialFilter","backend":"Auto","inputKind":"GeoParquet","source":path,"hasAttributeFilter":true,"hasSpatialFilter":true,"projectionRequired":true,
        "options":{"columns":["id","class","geometry"],"predicateSql":"class = ?","predicateParameters":[class],"extent":[18.04,59.30,18.10,59.35],"limit":limit}}).to_string()
}

fn report(class:&str,limit:i64,result:&Value,materialization:&Value)->String
{
    let plan=&result["plan"];
    let yes_no=|value:&Value|if value.as_bool()==Some(true) {"yes"} else {"no"};
    let mut text=format!("Building class: {class}\nMaximum results: {limit}\nBBOX: 18.04, 59.30, 18.10, 59.35\n\nANALYSIS PLAN\nRequested backend: Auto\nSelected backend: {}\nPredicate pushdown: {}\nProjection pushdown: {}\n\n{}\n\nEXECUTION ATTEMPTS\n",
        result["backend"].as_str().unwrap_or("Unknown"),yes_no(&plan["usesPredicatePushdown"]),yes_no(&plan["usesProjectionPushdown"]),plan["explanation"].as_str().unwrap_or(""));
    if let Some(attempts)=result["attempts"].as_array() {
        for attempt in attempts {
            text.push_str(&format!("{}: {} ({} ms) {}\n",attempt["backend"].as_str().unwrap_or("Unknown"),
                if attempt["succeeded"].as_bool()==Some(true) {"success"} else {"failed"},
                attempt["elapsedMilliseconds"],attempt["message"].as_str().unwrap_or("")));
        }
    }
    if !materialization.is_null() {
        text.push_str(&format!("\nMATERIALIZATION\nSource rows: {}\nLayer features: {}\nSkipped: {}\n",
            materialization["sourceRowCount"],materialization["materializedCount"],materialization["skippedCount"]));
        if let Some(warnings)=materialization["warnings"].as_array() {
            for warning in warnings {text.push_str(&format!("{}\n",warning.as_str().unwrap_or("")));}
        }
    }
    text
}

fn show_report(window:&mut ViewerWindow,text:&str)->Result<()>
{
    window.clear_log()?;
    window.append_log(&format!("<pre>{}</pre>",text.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;")))?;
    Ok(())
}

fn draw(window:&mut ViewerWindow,hexes:&[String])->Result<()>
{
    window.viewer().clear_shapes()?;
    let style=json!({"fillColor":"#65B8E8","lineColor":"#176B9C","lineWidth":0.8}).to_string();
    for hex in hexes
    {
        if !hex.len().is_multiple_of(2) {return Err("Invalid WKB hex length".into());}
        let bytes=(0..hex.len()).step_by(2).map(|i|u8::from_str_radix(&hex[i..i+2],16)).collect::<std::result::Result<Vec<_>,_>>()?;
        let value:Value=serde_json::from_str(&window.viewer().read_wkb_geometry_json(&bytes)?)?;
        let parts=value["parts"].as_array().ok_or("Missing geometry parts")?.iter().map(|ring|
            ring.as_array().ok_or("Invalid ring")?.iter().map(|p|Ok([p["x"].as_f64().ok_or("Missing x")?,p["y"].as_f64().ok_or("Missing y")?])).collect::<Result<Vec<_>>>()
        ).collect::<Result<Vec<_>>>()?;
        window.viewer().add_polygon_parts_shape(&parts,&style)?;
    }
    runtime::refresh(window)?;
    window.viewer().set_view_extent(geokernel::Extent{x_min:18.04,y_min:59.30,x_max:18.10,y_max:59.35})?;
    Ok(())
}

fn run(window:&mut ViewerWindow,path:&str)->Result<()>
{
    let mut sdk=unsafe {Runtime::from_env()?};
    sdk.set_text_capacity(16*1024*1024)?;
    let mut analysis=sdk.analysis()?;
    window.add_navigation_toolbar();
    window.add_log_panel("Analysis plan and materialization")?;
    window.viewer().set_coordinate_system_preset("EPSG:4326")?;
    let classes=["apartments","house","commercial","industrial"];
    let controls=window.add_control_panel(&json!({"title":"AnalysisGeoParquetFilter","controls":[
        {"id":1,"type":"combo","label":"Building class","options":classes,"value":"apartments"},
        {"id":2,"type":"number","label":"Maximum results","minimum":1,"maximum":100000,"value":25000},
        {"id":3,"type":"button","text":"Run automatic analysis"},
        {"id":4,"type":"button","text":"Cancel","enabled":false}
    ]}).to_string())?;
    let (mut class,mut limit)=(String::from("apartments"),25000);
    let mut job:Option<geokernel::AnalysisJob>=None;
    let mut start = false;
    runtime::show(window)?;
    loop
    {
        window.process_events();
        while let Ok(event)=controls.try_recv()
        {
            match event.id
            {
                1=>if classes.contains(&event.text.as_str()) {class=event.text;},
                2=>limit=(event.number as i64).clamp(1,100000),
                3=>start=job.is_none(),
                4=>if let Some(active)=job.as_mut() {active.job_cancel()?;},
                _=>{}
            }
        }
        if start && job.is_none()
        {
            start=false;
            let request=analysis_request(path,&class,limit);
            show_report(window,&format!("Running analysis for '{class}' (maximum {limit})..."))?;
            match analysis.start(&request) {
                Ok(active)=>{
                    job=Some(active);
                    for id in [1,2,3] {window.set_control_enabled(id,false)?;}
                    window.set_control_enabled(4,true)?;
                },
                Err(error)=>window.set_status_text(&error.to_string())?,
            }
        }
        if let Some(active)=job.as_mut()
        {
            window.set_status_text(&active.job_progress_json()?)?;
            if active.job_is_finished()?!=0
            {
                let outcome=(||->Result<usize>
                {
                    let mut result=active.job_wait()?;
                    let summary:Value=serde_json::from_str(&result.result_json()?)?;
                    show_report(window,&report(&class,limit,&summary,&Value::Null))?;
                    if summary["cancelled"].as_bool()==Some(true) {return Err("Analysis cancelled".into());}
                    if summary["succeeded"].as_bool()!=Some(true) {
                        return Err(summary["message"].as_str().unwrap_or("Analysis failed").to_owned().into());
                    }
                    let mut layer=result.materialize_layer("{\"name\":\"Filtered buildings\",\"geometryColumn\":\"geometry\",\"buildSpatialIndex\":true}")?;
                    let diagnostics:Value=serde_json::from_str(&layer.layer_diagnostics_json()?)?;
                    show_report(window,&report(&class,limit,&summary,&diagnostics))?;
                    let value:Value=serde_json::from_str(&layer.layer_features_json()?)?;
                    let hexes=value["geometryWkbHex"].as_array().ok_or("Missing geometries")?.iter().map(|v|v.as_str().ok_or("Invalid WKB").map(str::to_owned)).collect::<std::result::Result<Vec<_>,_>>()?;
                    draw(window,&hexes)?;
                    Ok(hexes.len())
                })();
                job=None;
                for id in [1,2,3] {window.set_control_enabled(id,true)?;}
                window.set_control_enabled(4,false)?;
                match outcome
                {
                    Ok(count)=>
                    {
                        window.set_status_text(&format!("{count} filtered buildings ({class})"))?;

                    }
                    Err(error)=>{ window.set_status_text(&format!("{error}: {}",analysis.last_error()?))?;}
                }
            }
        }
        if !window.is_visible()?
        {
            if let Some(mut active)=job.take() {active.job_cancel()?;let _=active.job_wait();}
            break;
        }

    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("AnalysisGeoParquetFilter","stockholm_data/stockholm_buildings.parquet",run)
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
