mod bootstrap;
use graph::Graph;
use geokernel::{ViewerTool,ViewerWindow};
use runtime::Result;
use serde_json::json;
use std::collections::HashMap;

fn projected(window:&mut ViewerWindow,points:&[[f64;2]])->Result<Vec<[f64;2]>>
{
    points.iter().map(|[x,y]| {
        let (x,y)=window.viewer().transform_point(4326,3857,*x,*y)?;
        Ok([x,y])
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

fn calculate(window:&mut ViewerWindow,graph:&Graph,stops:&[usize])->Result<()>
{
    if stops.len()!=2 {return Err("Select start and finish".into());}
    let path=graph.path(stops[0],stops[1],false,&HashMap::new())?;
    let points=projected(window,&graph.geometry(&path))?;
    if points.len()<2 {return Err("Select two different road nodes".into());}
    window.viewer().clear_shapes()?;
    window.viewer().add_polyline_shape(&points,&json!({"lineColor":"#EF4444","lineWidth":4}).to_string())?;
    markers(window,graph,stops)?;
    let distance:f64=path.iter().map(|&i|graph.edges[i].distance).sum();
    let seconds:f64=path.iter().map(|&i|graph.edges[i].seconds).sum();
    let mut steps:Vec<(String,f64)>=Vec::new();
    for &id in &path {
        let edge=&graph.edges[id];
        if let Some(last)=steps.last_mut().filter(|last|last.0.to_lowercase()==edge.name.to_lowercase()) {
            last.1+=edge.distance;
        }else {steps.push((edge.name.clone(),edge.distance));}
    }
    window.clear_log()?;
    window.append_log(&format!("<b>Route directions</b><br>{:.2} km · {:.1} min<br><br><font color='#16A34A'>● Start</font> &nbsp; <font color='#DC2626'>● Finish</font><br>",distance/1000.0,seconds/60.0))?;
    for (index,(name,distance)) in steps.iter().enumerate() {
        let name=name.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;");
        let length=if *distance>=1000.0 {format!("{:.1} km",distance/1000.0)}else{format!("{distance:.0} m")};
        window.append_log(&format!("{}. {name}<br>&nbsp;&nbsp;&nbsp;{length}<br>",index+1))?;
    }
    window.set_status_text(&format!("Route: {:.2} km, {:.1} min",distance/1000.0,seconds/60.0))?;
    Ok(())
}

fn run(window:&mut ViewerWindow,path:&str)->Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Route directions")?;
    let controls=window.add_control_panel(&json!({"title":"ShortestRoute","controls":[
        {"id":1,"type":"button","text":"Select route points"}
    ]}).to_string())?;
    let events=window.subscribe_events();
    if !window.viewer().add_layer_file(path)? {return Err("Could not load Stockholm roads".into());}
    if !window.viewer().set_layer_coordinate_system_preset(0,"EPSG:4326")? {return Err("Could not set road CRS".into());}
    if !window.viewer().set_coordinate_system_preset("EPSG:3857")? {return Err("Could not set map CRS".into());}
    window.viewer().set_layer_style_json(0,&json!({"lineColor":"#708580","lineWidth":1}).to_string())?;
    if !window.viewer().build_routing_graph_for_layer(0,1e-6,true,"maxspeed","name","oneway",50.0)? {return Err("Routing graph build failed".into());}
    let graph=Graph::parse(&window.viewer().routing_graph_json(96*1024*1024)?)?;
    window.viewer().use_tool(ViewerTool::Route);
    runtime::show(window)?;
    window.viewer().zoom_to_layer(0)?;
    window.append_log("Select a start and finish point.<br><font color='#16A34A'>● Start</font> &nbsp; <font color='#DC2626'>● Finish</font>")?;
    window.set_status_text("Click the map to choose the start point.")?;
    let mut stops=Vec::new();
    while window.is_visible()? {
        window.process_events();
        while let Ok(event)=events.try_recv() {
            if event.data.event_type==20 && event.data.int_value==8 && event.data.int_value2==1 {
                let (x,y)=window.viewer().transform_point(3857,4326,event.data.extent.x_min,event.data.extent.y_min)?;
                let node=graph.nearest(x,y);
                if stops.len()>=2 {stops.clear();}
                stops.push(node);
                window.viewer().clear_shapes()?;
                markers(window,&graph,&stops)?;
                window.clear_log()?;
                if stops.len()==2 {
                    if let Err(error)=calculate(window,&graph,&stops) {window.set_status_text(&error.to_string())?;}
                }else {
                    window.append_log("Start selected. Select the finish point.")?;
                    window.set_status_text("Start selected. Click the map to choose the finish point.")?;
                }
            }
        }
        while let Ok(control)=controls.try_recv() {
            if control.id==1 {
                stops.clear();window.viewer().clear_shapes()?;window.clear_log()?;
                window.viewer().use_tool(ViewerTool::Route);
                window.set_status_text("Click the map to choose the start point.")?;
            }
        }
    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("ShortestRoute","stockholm/stockholm.shp",run)
}

mod graph
{
    use crate::runtime::Result;
    use serde_json::Value;
    use std::{collections::{BinaryHeap,HashMap},cmp::Ordering};
    pub struct Edge {pub name:String,pub from:usize,pub to:usize,pub distance:f64,pub seconds:f64,pub geometry:Vec<[f64;2]>}
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
                let name=e["attributes"]["name"].as_str().unwrap_or("").trim();
                let name=if name.is_empty(){"Unnamed road"}else{name}.to_owned();
                edges.push(Edge{name,from,to,distance,seconds:distance/(speed/3.6),geometry});
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
        pub fn path(&self,start:usize,end:usize,time:bool,penalties:&HashMap<usize,f64>)->Result<Vec<usize>>
        {
            let (dist,parent)=self.distances(start,time,penalties);
            if !dist[end].is_finite(){return Err("No connected route; choose another road node".into());}
            let mut result=Vec::new();let mut node=end;
            while node!=start{let edge=parent[node].ok_or("Incomplete route")?;result.push(edge);node=self.edges[edge].from;if result.len()>self.nodes.len(){return Err("Route cycle".into());}}
            result.reverse();Ok(result)
        }
        pub fn geometry(&self,path:&[usize])->Vec<[f64;2]>
        {
            let mut points=Vec::new();
            for &id in path{let e=&self.edges[id];if e.geometry.is_empty(){points.push(self.nodes[e.from]);points.push(self.nodes[e.to]);}else{points.extend_from_slice(&e.geometry);}}
            points
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
