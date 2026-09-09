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
        let (fill,outline)=if index==0 {("#22C55E","#14532D")}else{("#F59E0B","#92400E")};
        window.viewer().add_point_shape(p[0],p[1],&json!({"pointColor":fill,"lineColor":outline,"lineWidth":2,"pointSize":16}).to_string())?;
    }
    Ok(())
}

fn draw(window:&mut ViewerWindow,graph:&Graph,path:&[usize],color:&str)->Result<Vec<[f64;2]>>
{
    let points=projected(window,&graph.geometry(path))?;
    if points.len()<2{return Err("Route has no geometry".into());}
    window.viewer().add_polyline_shape(&points,&json!({"lineColor":color,"lineWidth":4}).to_string())?;
    let distance:f64=path.iter().map(|&i|graph.edges[i].distance).sum();
    let seconds:f64=path.iter().map(|&i|graph.edges[i].seconds).sum();
    window.append_log(&format!("Route: {:.2} km, {:.1} min; {} segments",distance/1000.0,seconds/60.0,path.len()))?;
    Ok(points)
}

fn calculate(window:&mut ViewerWindow,graph:&Graph,stops:&[usize],vehicles:usize)->Result<Vec<[f64;2]>>
{
    if stops.len()<3{return Err("Select a depot and at least two visits".into());}
    window.viewer().clear_shapes()?;window.clear_log()?;
    markers(window,graph,stops)?;
    if stops.len()>12{return Err("This interactive example supports up to 12 stops".into());}
    let n=stops.len();let mut paths=vec![vec![Vec::new();n];n];let mut costs=vec![vec![0.0;n];n];
    for i in 0..n{for j in 0..n{if i!=j{paths[i][j]=graph.path(stops[i],stops[j],true,&HashMap::new())?;costs[i][j]=paths[i][j].iter().map(|&e|graph.edges[e].seconds).sum();}}}
    let mut order=vec![0];let mut remaining:Vec<usize>=(1..n).collect();
    while !remaining.is_empty(){let last=*order.last().ok_or("No depot")?;let best=(0..remaining.len()).min_by(|&a,&b|costs[last][remaining[a]].total_cmp(&costs[last][remaining[b]])).ok_or("No stop")?;order.push(remaining.remove(best));}
    order.push(0);
    let cost=|o:&[usize]|o.windows(2).map(|p|costs[p[0]][p[1]]).sum::<f64>();
    loop{let mut improved=false;for a in 1..n{for b in a+1..n{let mut candidate=order.clone();candidate[a..=b].reverse();if cost(&candidate)+1e-8<cost(&order){order=candidate;improved=true;}}}if !improved{break;}}
    window.append_log(&format!("Nearest-neighbor + 2-opt visit order: {order:?}"))?;
    let vehicles=vehicles.clamp(1,n-1);
    let colors=["#E63946","#1565C0","#2A9D8F","#9C27B0","#FF9800","#795548"];
    let mut combined=Vec::new();
    let mut served=Vec::new();
    for vehicle in 0..vehicles
    {
        let begin=1+vehicle*(n-1)/vehicles;
        let end=1+(vehicle+1)*(n-1)/vehicles;
        let mut tour=vec![0];
        tour.extend_from_slice(&order[begin..end]);
        served.extend_from_slice(&order[begin..end]);
        tour.push(0);
        window.append_log(&format!("Vehicle {}: {tour:?}",vehicle+1))?;
        let mut route=Vec::new();
        for pair in tour.windows(2) {route.extend_from_slice(&paths[pair[0]][pair[1]]);}
        combined.extend(draw(window,graph,&route,colors[vehicle%colors.len()])?);
    }
    served.sort_unstable();
    if served!=(1..n).collect::<Vec<_>>() {return Err("Every delivery must be served exactly once".into());}
    markers(window,graph,stops)?;
    window.set_status_text(&format!("Optimization completed: {} visits, {} vehicles",n-1,vehicles))?;
    Ok(combined)
}

fn run(window:&mut ViewerWindow,path:&str)->Result<()>
{
    window.add_navigation_toolbar();window.add_log_panel("Routing result")?;
    let controls=window.add_control_panel(&json!({"title":"RouteOptimization","controls":[
        {"id":1,"type":"button","text":"Calculate"},{"id":2,"type":"button","text":"Clear stops"},{"id":3,"type":"button","text":"Full Extent"},
        {"id":4,"type":"number","label":"Vehicles","minimum":1,"maximum":6,"value":1}
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
    window.set_status_text("Click to add the depot, then at least two visits. Green: depot; orange: visits.")?;
    let mut stops=Vec::new();
    let mut vehicles=1;

    while window.is_visible()?{
        window.process_events();
        while let Ok(event)=events.try_recv(){
            if event.data.event_type==20 && event.data.int_value==8 && event.data.int_value2==1{
                let (x,y)=window.viewer().transform_point(3857,4326,event.data.extent.x_min,event.data.extent.y_min)?;
                let node=graph.nearest(x,y);
                if stops.contains(&node) {continue;}
                if stops.len()>=12 {window.set_status_text("Maximum 12 stops including depot.")?;continue;}

                stops.push(node);
                window.viewer().clear_shapes()?;window.clear_log()?;
                markers(window,&graph,&stops)?;
                window.set_status_text(&format!("{} stop(s) selected",stops.len()))?;
            }
        }
        while let Ok(control)=controls.try_recv(){
            match control.id{
                1=>match calculate(window,&graph,&stops,vehicles){Ok(points)=>{let _=points;},Err(error)=>window.set_status_text(&error.to_string())?},
                2=>{stops.clear();window.viewer().clear_shapes()?;window.clear_log()?;window.viewer().use_tool(ViewerTool::Route);window.set_status_text("Click the map to add the depot.")?;},
                3=>{window.viewer().zoom_to_layer(0)?;},
                4=>vehicles=(control.number as usize).clamp(1,6),
                _=>{}
            }
        }

    }
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("RouteOptimization","stockholm/stockholm.shp",run)
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
