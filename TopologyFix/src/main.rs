mod bootstrap;
use geokernel::{Extent,ViewerWindow};
use runtime::Result;
use serde_json::{json,Value};

fn source()->Vec<Vec<[f64;2]>>
{
    vec![vec![[-5.2,-1.3],[-4.0,-0.2],[-4.0,-0.2],[-2.6,-1.1],[-1.2,0.5],[-1.2,0.5],[0.4,0.1]],vec![[1.5,1.0]],vec![[2.8,-0.8],[2.8,-0.8]],vec![[3.7,-1.1],[4.8,0.3],[5.4,-0.9]]]
}

fn operation_id(name:&str)->Option<i32>
{
    match name { "FixShape"=>Some(0),"FixShapeEx"=>Some(1),"ClearShape"=>Some(2),_=>None }
}

fn summary(parts:&[Vec<[f64;2]>])->String
{
    let vertices:usize=parts.iter().map(Vec::len).sum();
    let mut text=format!("Parts: {}\nVertices: {}\n",parts.len(),vertices);
    if vertices==0 { text.push_str("Extent: (empty)\n"); }
    else {
        let mut e=[f64::INFINITY,f64::INFINITY,f64::NEG_INFINITY,f64::NEG_INFINITY];
        for [x,y] in parts.iter().flatten() { e[0]=e[0].min(*x);e[1]=e[1].min(*y);e[2]=e[2].max(*x);e[3]=e[3].max(*y); }
        text.push_str(&format!("Extent: ({:.2}, {:.2}) - ({:.2}, {:.2})\n",e[0],e[1],e[2],e[3]));
    }
    for (i,part) in parts.iter().enumerate() { text.push_str(&format!("part {}: {} vertices\n",i+1,part.len())); }
    text
}

fn draw(window:&mut ViewerWindow,parts:&[Vec<[f64;2]>],color:&str,width:f64,size:f64)->Result<()>
{
    let style=json!({"lineColor":color,"lineWidth":width,"pointColor":color,"pointSize":size}).to_string();
    for part in parts {
        if part.len()>1 { window.viewer().add_polyline_shape(part,&style)?; }
        if let Some(point)=part.first().filter(|point| part.iter().all(|p| p==*point)) {
            window.viewer().add_point_shape(point[0],point[1],&style)?;
        }
    }
    Ok(())
}

fn calculate(window:&mut ViewerWindow,operation:&str)->Result<()>
{
    let inputs=source();
    window.viewer().clear_shapes()?;
    draw(window,&inputs,"#6C757D",2.0,9.0)?;
    let mut details=format!("Topology fix functions\n\nSource: messy multipart polyline\n- part 1 has duplicate consecutive vertices\n- part 2 has only one vertex\n- part 3 collapses to one vertex after duplicate cleanup\n- part 4 is already valid\n\nSource\n{}",summary(&inputs));
    if let Some(id)=operation_id(operation) {
        let value:Value=serde_json::from_str(&window.viewer().fix_polyline_json(&inputs,id)?)?;
        let parts:Vec<Vec<[f64;2]>>=value.as_array().ok_or("Expected fixed parts")?.iter().map(|part| {
            part.as_array().ok_or("Expected part")?.iter().map(|p| Ok([p["x"].as_f64().ok_or("Missing x")?,p["y"].as_f64().ok_or("Missing y")?])).collect::<Result<_>>()
        }).collect::<Result<_>>()?;
        let (color,note)=match id {
            1=>("#7B2CBF","FixShapeEx(preserveEmptyParts=true) keeps short/empty parts for diagnostics."),
            2=>("#D95D39","ClearShape currently performs the same cleanup path as FixShape."),
            _=>("#2A9D8F","FixShape removes duplicate vertices and drops invalid short parts.")
        };
        draw(window,&parts,color,4.0,12.0)?;
        details.push_str(&format!("\nOperation: {operation}\nResult\n{}\n{note}",summary(&parts)));
        window.set_status_text(&format!("{operation} applied."))?;
    } else {
        details.push_str("\nChoose an operation from the combo box to see the cleaned result.");
        window.set_status_text("Source geometry is shown. Choose a fix operation.")?;
    }
    window.clear_log()?;
    window.append_log(&format!("<pre>{details}</pre>"))?;
    runtime::refresh(window)?;
    Ok(())
}

fn run(window:&mut ViewerWindow,_path:&str)->Result<()>
{
    window.add_log_panel("Topology fix functions")?;
    let events=window.add_control_panel(&json!({"title":"TopologyFix","controls":[
        {"id":1,"type":"combo","label":"Operation","options":["Source only","FixShape","FixShapeEx","ClearShape"],"value":"Source only"},
        {"id":2,"type":"button","text":"Full Extent"}
    ]}).to_string())?;
    calculate(window,"Source only")?;
    runtime::show(window)?;
    window.viewer().set_view_extent(Extent{x_min:-5.9,y_min:-2.4,x_max:5.9,y_max:1.8})?;
    while window.is_visible()? {
        window.process_events();
        while let Ok(event)=events.try_recv() {
            match event.id {
                1=>calculate(window,&event.text)?,
                2=>window.viewer().set_view_extent(Extent{x_min:-5.9,y_min:-2.4,x_max:5.9,y_max:1.8})?,
                _=>{}
            }
        }
    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("TopologyFix","",run)
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
