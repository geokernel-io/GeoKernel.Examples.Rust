mod bootstrap;
use geokernel::ViewerWindow;
use runtime::Result;

fn run(window: &mut ViewerWindow, _path: &str) -> Result<()>
{
    styles::run(window, true)
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("SelectionStyle", "", run)
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

mod styles
{
    use crate::runtime::{self, Result};
    use geokernel::{ControlEvent, Extent, ViewerTool, ViewerWindow};
    use serde_json::{json, Value};

    fn edit(window: &mut ViewerWindow, index: i32, kind: i32, points: &[[f64; 2]]) -> Result<()>
    {
        let mut viewer = window.viewer();
        if !viewer.begin_edit_layer(index)? {
            return Err("Could not edit memory layer".into());
        }
        let ok = match kind {
            1 => viewer.add_point_to_edit_layer(index, points[0][0], points[0][1])?,
            3 => viewer.add_polyline_to_edit_layer(index, points)?,
            _ => viewer.add_polygon_to_edit_layer(index, points)?,
        };
        if !ok || !viewer.commit_edit_layer(index)? {
            return Err("Could not append geometry".into());
        }
        Ok(())
    }

    fn apply(window: &mut ViewerWindow, selection: bool, values: &Value) -> Result<()>
    {
        for index in 0..3 {
            let patch = if selection {
                json!({"selectedLineColor":values["color"],"selectedLineWidth":values["width"]})
            } else {
                match index {
                    0 => json!({"pointColor":"#D95F35","pointSize":values["size"]}),
                    1 => json!({"lineColor":values["line"],"lineWidth":values["width"]}),
                    _ => json!({"fillColor":values["fill"],"fillOpacity":185,"lineColor":values["line"],"lineWidth":values["width"]}),
                }
            };
            if !window.viewer().set_layer_style_json(index, &patch.to_string())? {
                return Err("Style update failed".into());
            }
        }
        window.viewer().refresh_layers()?;
        Ok(())
    }

    fn handle(window: &mut ViewerWindow, selection: bool, values: &mut Value, event: ControlEvent) -> Result<()>
    {
        match event.id {
            1 => values[if selection { "color" } else { "fill" }] = json!(event.text),
            2 => values["line"] = json!(event.text),
            3 => values["width"] = json!(event.number),
            4 => values["size"] = json!(event.number),
            5 => {
                *values = defaults(selection);
                window.set_control_value(1, 0.0, values[if selection { "color" } else { "fill" }].as_str().unwrap())?;
                window.set_control_value(3, values["width"].as_f64().unwrap(), "")?;
                if !selection {
                    window.set_control_value(2, 0.0, values["line"].as_str().unwrap())?;
                    window.set_control_value(4, 10.0, "")?;
                }
            }
            6 => window.viewer().clear_selected_features()?,
            _ => return Err("Unknown style control".into()),
        }
        apply(window, selection, values)
    }

    fn defaults(selection: bool) -> Value
    {
        if selection {
            json!({"color":"#F59E0B","width":4.0})
        } else {
            json!({"fill":"#F1D58A","line":"#266D8F","width":2.0,"size":10.0})
        }
    }

    pub fn run(window: &mut ViewerWindow, selection: bool) -> Result<()>
    {
        let name = if selection { "SelectionStyle" } else { "SimpleStyle" };
        runtime::show(window)?;
        let mut values = defaults(selection);
        let mut items = vec![
            json!({"id":1,"type":"color","label":if selection {"Selected Line Color"} else {"Fill Color"},"value":values[if selection {"color"} else {"fill"}]}),
            json!({"id":3,"type":"number","label":if selection {"Selected Line Width"} else {"Line Width"},"minimum":if selection {1.0} else {0.5},"maximum":if selection {16.0} else {12.0},"step":0.5,"value":values["width"]}),
        ];
        if !selection {
            items.extend([
                json!({"id":2,"type":"color","label":"Line Color","value":values["line"]}),
                json!({"id":4,"type":"number","label":"Point Size","minimum":2,"maximum":32,"step":0.5,"value":10}),
            ]);
        } else {
            items.push(json!({"id":6,"type":"button","text":"Clear Selection"}));
        }
        items.push(json!({"id":5,"type":"button","text":"Reset Style"}));
        let rx = window.add_control_panel(&json!({"title":name,"controls":items}).to_string())?;
        for (label, kind, style) in [
            (
                "Polygons",
                5,
                json!({"fillColor":"#F1D58A","fillOpacity":180,"lineColor":"#266D8F","lineWidth":1.8}),
            ),
            ("Polyline", 3, json!({"lineColor":"#266D8F","lineWidth":2.2})),
            ("Points", 1, json!({"pointColor":"#D95F35","pointSize":10.0})),
        ] {
            if window.viewer().add_empty_vector_layer(
                &format!("{} {label}", if selection { "Selectable" } else { "Styled" }),
                kind,
                &style.to_string(),
            )? < 0
            {
                return Err("Memory layer creation failed".into());
            }
        }
        if selection {
            edit(
                window,
                2,
                5,
                &[[-11.0, -4.0], [-4.0, -4.0], [-3.0, 2.0], [-8.0, 5.0], [-12.0, 1.0], [-11.0, -4.0]],
            )?;
            edit(window, 2, 5, &[[2.0, -4.0], [10.0, -4.0], [12.0, 2.0], [6.0, 5.0], [1.0, 1.0], [2.0, -4.0]])?;
            edit(window, 1, 3, &[[-12.0, -7.0], [-6.0, -1.0], [0.0, -5.5], [6.0, -0.5], [13.0, -5.0]])?;
            for p in [[-8.0, 8.0], [0.0, 7.0], [8.0, 8.0]] {
                edit(window, 0, 1, &[p])?;
            }
            window.viewer().use_tool(ViewerTool::Select);
            window.viewer().set_view_extent(Extent {
                x_min: -15.0,
                y_min: -9.0,
                x_max: 15.0,
                y_max: 11.0,
            })?;
        } else {
            edit(window, 2, 5, &[[-8.0, -3.0], [1.0, -3.0], [3.0, 4.0], [-6.0, 6.0], [-10.0, 2.0], [-8.0, -3.0]])?;
            edit(window, 1, 3, &[[-12.0, -7.0], [-5.0, -1.0], [1.0, -5.0], [8.0, 2.0], [13.0, -2.0]])?;
            for p in [[-6.0, 9.0], [0.0, 8.0], [7.0, 7.0]] {
                edit(window, 0, 1, &[p])?;
            }
            window.viewer().use_tool(ViewerTool::Pan);
            window.viewer().set_view_extent(Extent {
                x_min: -19.5,
                y_min: -14.2,
                x_max: 20.5,
                y_max: 18.9,
            })?;
        }
        apply(window, selection, &values)?;
        while window.is_visible()? {
            window.process_events();
            for event in rx.try_iter() {
                handle(window, selection, &mut values, event)?;
            }

        }

        Ok(())
    }
}
