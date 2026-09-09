mod bootstrap;
use geokernel::{Extent, Runtime, Viewer, ViewerWindow, ViewerTool};
use serde_json::{json, Value};
use std::{error::Error, path::PathBuf};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn route(offset: f64) -> [[f64; 2]; 4]
{
    [[-122.4194 + offset, 37.7749], [-118.2437 + offset, 34.0522], [-112.0740 + offset, 33.4484], [-104.9903 + offset, 39.7392]]
}

fn region(offset: f64) -> [[f64; 2]; 6]
{
    [[-101.0 + offset, 30.0], [-91.0 + offset, 30.0], [-89.0 + offset, 37.0], [-96.0 + offset, 42.0], [-103.0 + offset, 38.0], [-101.0 + offset, 30.0]]
}

fn append(viewer: &mut Viewer<'_>, index: i32, points: &[[f64; 2]]) -> Result<()>
{
    if !viewer.begin_edit_layer(index)? {
        return Err("Could not begin memory layer edit".into());
    }

    let added = match index {
        0 => viewer.add_point_to_edit_layer(index, points[0][0], points[0][1])?,
        1 => viewer.add_polyline_to_edit_layer(index, points)?,
        2 => viewer.add_polygon_to_edit_layer(index, points)?,
        _ => false,
    };

    if !added || !viewer.commit_edit_layer(index)? {
        return Err("Could not append memory geometry".into());
    }

    Ok(())
}

fn reset_memory(viewer: &mut Viewer<'_>) -> Result<()>
{
    for name in ["Memory Cities", "Memory Routes", "Memory Regions"] {
        let info: Value = serde_json::from_str(&viewer.get_layer_info_by_name_json(name)?)?;

        if info["isValid"] == true && !viewer.remove_layer_by_name(name)? {
            return Err(format!("Could not reset {name}").into());
        }
    }

    for (name, shape_type, style) in [
        ("Memory Regions", 5, json!({"fillColor":"#F1D58A", "fillOpacity":150, "lineColor":"#9A7A1F", "lineWidth":1.5})),
        ("Memory Routes", 3, json!({"lineColor":"#266D8F", "lineWidth":2.2})),
        ("Memory Cities", 1, json!({"pointColor":"#D95F35", "pointSize":7.0})),
    ] {
        if viewer.add_empty_vector_layer(name, shape_type, &style.to_string())? < 0 {
            return Err(format!("Could not create {name}").into());
        }
    }

    append(viewer, 2, &region(0.0))?;
    append(viewer, 1, &route(0.0))?;
    append(viewer, 0, &[[-122.4194, 37.7749]])?;
    append(viewer, 0, &[[-118.2437, 34.0522]])?;

    Ok(())
}

fn command(window: &mut ViewerWindow, id: i32, cursors: &mut [i32; 3]) -> Result<()>
{
    {
        let mut viewer = window.viewer();

        match id {
            1 => {
                let column = cursors[0] % 12;
                let row = cursors[0] / 12;
                let point = [-124.0 + f64::from(column) * 4.8 + f64::from(row % 3) * 0.35, 25.0 + f64::from(row) * 3.2 + f64::from(column % 4) * 0.25];
                append(&mut viewer, 0, &[point])?;
                cursors[0] += 1;
            }
            2 => {
                append(&mut viewer, 1, &route(f64::from(cursors[1]) * 2.0))?;
                cursors[1] += 1;
            }
            3 => {
                append(&mut viewer, 2, &region(f64::from(cursors[2]) * 5.0))?;
                cursors[2] += 1;
            }
            4 => {
                reset_memory(&mut viewer)?;
                *cursors = [0, 1, 1];
            }
            5 => viewer.full_extent()?,
            _ => return Err("Unknown command".into()),
        }

        viewer.invalidate_render_cache(false, true)?;
        viewer.refresh_layers()?;
    }

    let counts = [window.viewer().get_layer_feature_count(0)?, window.viewer().get_layer_feature_count(1)?, window.viewer().get_layer_feature_count(2)?];
    window.set_status_text(&format!("Memory features - points: {} | lines: {} | polygons: {}", counts[0], counts[1], counts[2]))?;

    Ok(())
}

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    let commands = window.add_command_toolbar(&json!({"commands":[
        {"id":1,"text":"Add Point","enabled":false}, {"id":2,"text":"Add Line","enabled":false},
        {"id":3,"text":"Add Polygon","enabled":false}, {"id":4,"text":"Clear Memory Layers","separatorBefore":true,"enabled":false},
        {"id":5,"text":"Full Extent","enabled":false}
    ]}).to_string())?;

    window.show()?;
    window.process_events();

    {
        let mut viewer = window.viewer();
        viewer.use_tool(ViewerTool::Pan);
        let style = json!({"fillColor":"#D8E5E1", "fillOpacity":210, "lineColor":"#6F8883", "lineWidth":0.7});

        if !viewer.add_layer_file(path)? || !viewer.set_layer_name(0, "World")? || !viewer.set_layer_style_json(0, &style.to_string())? {
            return Err("Could not load World".into());
        }
    }

    let mut cursors = [0, 1, 1];
    command(window, 4, &mut cursors)?;
    window.viewer().set_view_extent(Extent { x_min: -130.0, y_min: 20.0, x_max: -65.0, y_max: 52.0 })?;

    for id in 1..=5 {
        if !window.set_command_enabled(id, true)? {
            return Err("Could not enable memory command".into());
        }
    }

    while window.is_visible()? {
        window.process_events();

        for id in commands.try_iter() {
            command(window, id, &mut cursors)?;
        }

    }

    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    let data = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/world_4326/world_4326.shp");

    let path = data.canonicalize()?;

    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| {
        let mut window = unsafe { ViewerWindow::new(&runtime, "InMemoryLayers", 1200, 800)? };
        run(&mut window, path.to_str().ok_or("Invalid point path")?)
    })();

    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;

    Ok(())
}
