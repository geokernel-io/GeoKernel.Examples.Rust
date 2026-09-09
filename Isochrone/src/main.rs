mod bootstrap;
use graph::Graph;
use geokernel::{ViewerTool,ViewerWindow};
use runtime::Result;
use serde_json::json;
use std::collections::HashMap;

fn projected(_window:&mut ViewerWindow,points:&[[f64;2]])->Result<Vec<[f64;2]>>
{
    points.iter().map(|[x,y]| {
        if !x.is_finite() || !y.is_finite() || y.abs()>85.05112878 {return Err("Invalid Web Mercator coordinate".into());}
        let radius=6378137.0;
        Ok([radius*x.to_radians(),radius*(std::f64::consts::FRAC_PI_4+y.to_radians()/2.0).tan().ln()])
    }).collect()
}

fn markers(window:&mut ViewerWindow,graph:&Graph,stops:&[usize])->Result<()>
{
    for (index,&node) in stops.iter().enumerate() {
        let p=projected(window,&[graph.nodes[node]])?[0];
        let (fill,outline)=if index==0 {("#22C55E","#14532D")}else{("#EF4444","#7F1D1D")};
        window.viewer().add_point_shape(p[0],p[1],&json!({"pointColor":fill,"lineColor":outline,"lineWidth":2,"pointSize":16}).to_string())?;
    }
    Ok(())
}

fn calculate(window:&mut ViewerWindow,graph:&Graph,stops:&[usize])->Result<Vec<[f64;2]>>
{
    window.viewer().clear_shapes()?;window.clear_log()?;
    if stops.is_empty(){return Err("Select an origin".into());}
    let (cost,_)=graph.distances(stops[0],true,&HashMap::new());let mut count=0;
    for edge in &graph.edges {
        let time=cost[edge.from].max(cost[edge.to]);
        if time<=900.0 && edge.geometry.len()>1 {
            let color=if time<=300.0 {"#2A9D8F"}else if time<=600.0 {"#F9C74F"}else {"#E63946"};
            let points=projected(window,&edge.geometry)?;
            window.viewer().add_polyline_shape(&points,&json!({"lineColor":color,"lineWidth":2}).to_string())?;
            count+=1;
        }
    }
    markers(window,graph,stops)?;
    if count==0{return Err("No reachable roads".into());}
    window.append_log(&format!("Reachable roads: {count}; green ≤5 min, yellow ≤10 min, red ≤15 min"))?;
    window.set_status_text(&format!("Isochrone calculated: {count} reachable road segments."))?;
    Ok(Vec::new())
}

fn run(window:&mut ViewerWindow,path:&str)->Result<()>
{
    window.add_navigation_toolbar();window.add_log_panel("Routing result")?;
    let controls=window.add_control_panel(&json!({"title":"Isochrone","controls":[
        {"id":1,"type":"button","text":"Select isochrone origin"},{"id":2,"type":"button","text":"Clear origin"},{"id":3,"type":"button","text":"Full Extent"}
    ]}).to_string())?;
    let events=window.subscribe_events();
    if !window.viewer().add_layer_file(path)?{return Err("Could not load Stockholm roads".into());}
    if !window.viewer().set_layer_coordinate_system_preset(0,"EPSG:4326")? {return Err("Could not set road CRS".into());}
    if !window.viewer().set_coordinate_system_preset("EPSG:3857")? {return Err("Could not set map CRS".into());}
    window.viewer().set_layer_style_json(0,&json!({"lineColor":"#708580","lineWidth":1}).to_string())?;
    if !window.viewer().build_routing_graph_for_layer(0,1e-6,true,"maxspeed","name","oneway",50.0)?{return Err("Routing graph build failed".into());}
    let graph=Graph::parse(&window.viewer().routing_graph_json(96*1024*1024)?)?;
    window.viewer().use_tool(ViewerTool::Route);
    runtime::show(window)?;window.viewer().zoom_to_layer(0)?;
    window.set_status_text("Click a road to calculate 5/10/15-minute reachable roads.")?;
    let mut stops=Vec::new();

    while window.is_visible()?{
        window.process_events();
        while let Ok(event)=events.try_recv(){
            if event.data.event_type==20 && event.data.int_value==8 && event.data.int_value2==1{
                let (x,y)=window.viewer().transform_point(3857,4326,event.data.extent.x_min,event.data.extent.y_min)?;
                let node=graph.nearest(x,y);
                stops.clear();
                stops.push(node);
                window.viewer().clear_shapes()?;window.clear_log()?;
                markers(window,&graph,&stops)?;
                window.set_status_text("Calculating 5/10/15-minute reachable roads...")?;
                if let Err(error)=calculate(window,&graph,&stops) {
                    window.set_status_text(&error.to_string())?;
                }
            }
        }
        while let Ok(control)=controls.try_recv(){
            match control.id{
                1=>{stops.clear();window.viewer().clear_shapes()?;window.clear_log()?;window.viewer().use_tool(ViewerTool::Route);window.set_status_text("Click the map to choose an origin.")?;},
                2=>{stops.clear();window.viewer().clear_shapes()?;window.clear_log()?;window.viewer().use_tool(ViewerTool::Route);window.set_status_text("Click the map to choose an origin.")?;},
                3=>{window.viewer().zoom_to_layer(0)?;},

                _=>{}
            }
        }

    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("Isochrone","stockholm/stockholm.shp",run)
}

mod graph
{
    use crate::runtime::Result;
    use serde_json::Value;
    use std::{collections::{BinaryHeap,HashMap},cmp::Ordering};
    pub struct Edge {pub from:usize,pub to:usize,pub distance:f64,pub seconds:f64,pub geometry:Vec<[f64;2]>}
    pub struct Graph {pub nodes:Vec<[f64;2]>,pub edges:Vec<Edge>,adj:Vec<Vec<usize>>,main:Vec<bool>}
    #[derive(Clone,Copy)] struct Entry(f64,usize);
    impl PartialEq for Entry {fn eq(&self,other:&Self)->bool {self.0==other.0 && self.1==other.1}}
    impl Eq for Entry {}
    impl PartialOrd for Entry {fn partial_cmp(&self,other:&Self)->Option<Ordering>{Some(self.cmp(other))}}
    impl Ord for Entry {fn cmp(&self,other:&Self)->Ordering{other.0.total_cmp(&self.0).then_with(||other.1.cmp(&self.1))}}
    impl Graph
    {
        pub fn parse(text:&str)->Result<Self>
        {
            let value:Value=serde_json::from_str(text)?;
            let mut nodes=Vec::new(); let mut ids=HashMap::new();
            for n in value["nodes"].as_array().ok_or("Missing graph nodes")? {
                ids.insert(n["id"].as_i64().ok_or("Missing node id")?,nodes.len());
                nodes.push([n["x"].as_f64().ok_or("Missing node x")?,n["y"].as_f64().ok_or("Missing node y")?]);
            }
            let mut edges=Vec::new();let mut adj=vec![Vec::new();nodes.len()];let mut undirected=adj.clone();
            for e in value["edges"].as_array().ok_or("Missing graph edges")? {
                let from=*ids.get(&e["from"].as_i64().ok_or("Missing from")?).ok_or("Unknown from node")?;
                let to=*ids.get(&e["to"].as_i64().ok_or("Missing to")?).ok_or("Unknown to node")?;
                let distance=e["distance"].as_f64().ok_or("Missing edge length")?;
                let speed=e["speedKmh"].as_f64().filter(|v|*v>0.0).unwrap_or(50.0);
                if !distance.is_finite() || distance<0.0 {return Err("Invalid edge length".into());}
                let geometry=e["geometry"].as_array().ok_or("Missing edge geometry")?.iter().map(|p|Ok([p[0].as_f64().ok_or("Missing x")?,p[1].as_f64().ok_or("Missing y")?])).collect::<Result<_>>()?;
                adj[from].push(edges.len()); undirected[from].push(to);undirected[to].push(from);
                edges.push(Edge{from,to,distance,seconds:distance/(speed/3.6),geometry});
            }
            if edges.is_empty(){return Err("Routing graph has no edges".into());}
            let mut visited=vec![false;nodes.len()];let mut largest=Vec::new();
            for start in 0..nodes.len(){
                if visited[start]{continue;} let mut component=vec![start];visited[start]=true;let mut head=0;
                while head<component.len(){let n=component[head];head+=1;for &next in &undirected[n]{if !visited[next]{visited[next]=true;component.push(next);}}}
                if component.len()>largest.len(){largest=component;}
            }
            let mut main=vec![false;nodes.len()];for n in largest{main[n]=true;}
            Ok(Self{nodes,edges,adj,main})
        }
        pub fn nearest(&self,x:f64,y:f64)->usize
        {
            let scale=y.to_radians().cos();
            self.nodes.iter().enumerate().filter(|(i,_)|self.main[*i]).min_by(|(_,a),(_,b)|{
                let d=|p:&[f64;2]|((p[0]-x)*scale).powi(2)+(p[1]-y).powi(2);
                d(a).total_cmp(&d(b))
            }).map(|(i,_)|i).unwrap_or(0)
        }
        pub fn distances(&self,start:usize,time:bool,penalties:&HashMap<usize,f64>)->(Vec<f64>,Vec<Option<usize>>)
        {
            let mut dist=vec![f64::INFINITY;self.nodes.len()];let mut parent=vec![None;self.nodes.len()];let mut queue=BinaryHeap::new();
            dist[start]=0.0;queue.push(Entry(0.0,start));
            while let Some(Entry(cost,node))=queue.pop(){
                if cost>dist[node]{continue;}
                for &id in &self.adj[node]{let e=&self.edges[id];let weight=if time{e.seconds}else{e.distance};let next=cost+weight*penalties.get(&id).copied().unwrap_or(1.0);
                    if next<dist[e.to]{dist[e.to]=next;parent[e.to]=Some(id);queue.push(Entry(next,e.to));}
                }
            }
            (dist,parent)
        }



    }
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
        let mut runtime = unsafe { Runtime::from_env()? };
        runtime.set_text_capacity(16 * 1024 * 1024)?;
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
