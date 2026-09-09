mod bootstrap;
use geokernel::ViewerWindow;
use runtime::Result;

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    point_renderers::run(window, path, false)
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("RuleBasedRenderer", "usa_cities/usa_cities.shp", run)
}

mod point_renderers
{
    use crate::runtime::{self, Result};
    use geokernel::{GraduatedOptions, ViewerTool, ViewerWindow};
    use serde_json::{json, Value};
    pub const LABELS: [&str; 6] = [
        "Less than 50,000",
        "50,000 to 100,000",
        "100,000 to 250,000",
        "250,000 to 500,000",
        "500,000 to 1,000,000",
        "1,000,000 to 5,000,000",
    ];

    pub fn run(window: &mut ViewerWindow, path: &str, size: bool) -> Result<()>
    {
        let name = if size { "GraduatedRendererSize" } else { "RuleBasedRenderer" };
        runtime::show(window)?;
        window.viewer().use_tool(ViewerTool::Pan);
        window.viewer().add_open_street_map_layer(true)?;
        let base = if size {
            json!({"pointColor":"#48D95F35","pointSize":3.0,"lineColor":"#AF8A3A24","lineWidth":0.9})
        } else {
            json!({"pointColor":"#917B8794","pointSize":4.0,"lineColor":"#D24B5563","lineWidth":0.9})
        };
        runtime::load(
            &mut window.viewer(),
            path,
            if size {
                "Cities - graduated size by POP_CLASS"
            } else {
                "Cities - rule based by POP_CLASS"
            },
            &base,
        )?;
        window.add_legend_panel("POP_CLASS")?;
        if size {
            if !window.viewer().add_layer_attribute_definition(0, "POP_CLASS_SIZE", 2, 12, 2)? || !window.viewer().begin_edit_layer(0)? {
                return Err("Could not prepare size field".into());
            }
            let count = window.viewer().get_layer_feature_count(0)?;
            for row in 0..count {
                let attributes: Value = serde_json::from_str(&window.viewer().get_layer_feature_attributes_json(0, row)?)?;
                let label = attributes["POP_CLASS"].as_str().unwrap_or("");
                let value = LABELS.iter().position(|s| s.eq_ignore_ascii_case(label.trim())).map_or(0, |i| i + 1);
                if !window
                    .viewer()
                    .set_feature_attributes_in_edit_layer_json(0, 0, row + 1, &json!({"POP_CLASS_SIZE":value}).to_string())?
                {
                    return Err("Could not set size attribute".into());
                }
            }
            if !window.viewer().apply_graduated_renderer(
                0,
                &GraduatedOptions {
                    field: "POP_CLASS_SIZE",
                    method: 2,
                    classes: 6,
                    ramp: "Plasma",
                    interval: 0.0,
                    breaks: &[],
                    ramp_mode: 1,
                    reverse: false,
                    target: 1,
                    start_size: 3.0,
                    end_size: 36.0,
                },
            )? {
                return Err("Size renderer failed".into());
            }
            let mut renderer = runtime::renderer(window)?;
            for (i, row) in renderer["ranges"].as_array_mut().ok_or("Missing ranges")?.iter_mut().enumerate() {
                row["label"] = json!(LABELS.get(i).unwrap_or(&"Other"));
                for (key, alpha) in [("pointColor", "48"), ("lineColor", "af")] {
                    let color = row["style"][key].as_str().unwrap_or("#D95F35");
                    let rgb = if color.len() >= 7 { &color[color.len() - 6..] } else { "D95F35" };
                    row["style"][key] = json!(format!("#{alpha}{rgb}"));
                }
                let point_size = row["style"]["pointSize"].as_f64().unwrap_or(3.0);
                row["style"]["lineWidth"] = json!((point_size * 0.07).clamp(0.9, 2.2));
            }
            renderer["defaultStyle"] = base.clone();
            if !window.viewer().set_layer_symbol_renderer_json(0, &renderer.to_string())? {
                return Err("Could not update size styles".into());
            }
        } else {
            let fills = ["#917B8794", "#914FA3C4", "#9155B889", "#91F2B84B", "#91E56B5D", "#91A9423A"];
            let outlines = ["#D24B5563", "#D21D6D83", "#D22E7D59", "#D29B6B18", "#D29A3E32", "#D261201C"];
            let sizes: [f64; 6] = [4.0, 5.5, 7.5, 10.0, 14.0, 19.0];
            let rules:Vec<Value>=(0..6).map(|i|json!({"field":"POP_CLASS","operator":"equals","value":LABELS[i],"label":LABELS[i],"enabled":true,"style":{"pointColor":fills[i],"lineColor":outlines[i],"pointSize":sizes[i],"lineWidth":(sizes[i]*0.06).clamp(0.8,1.5)}})).collect();
            if !window
                .viewer()
                .set_layer_symbol_renderer_json(0, &json!({"type":"ruleBased","defaultStyle":base,"rules":rules}).to_string())?
            {
                return Err("Rule renderer failed".into());
            }
        }
        runtime::refresh(window)?;
        let _legend_count = runtime::legend(window, true)?;
        // Establish the final viewport after the legend dock is laid out.
        window.process_events();
        let mut initial_extent = window.viewer().layer_projected_extent(0)?;
        // Match the Qt examples: 12% padding, with a minimum of 500 km per side.
        let padding_x = ((initial_extent.x_max - initial_extent.x_min) * 0.12).max(500000.0);
        let padding_y = ((initial_extent.y_max - initial_extent.y_min) * 0.12).max(500000.0);
        initial_extent.x_min -= padding_x;
        initial_extent.x_max += padding_x;
        initial_extent.y_min -= padding_y;
        initial_extent.y_max += padding_y;
        window.viewer().set_view_extent(initial_extent)?;
        window.set_status_text(&format!("{name} applied: POP_CLASS"))?;
        while window.is_visible()? {
            window.process_events();

        }

        Ok(())
    }
}

mod runtime
{
    use geokernel::{Runtime, Viewer, ViewerWindow};
    use serde_json::{json, Value};
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

    pub fn load(viewer: &mut Viewer<'_>, path: &str, name: &str, style: &Value) -> Result<()>
    {
        if !viewer.add_layer_file(path)? || !viewer.set_layer_name(0, name)? || !viewer.set_layer_style_json(0, &style.to_string())? {
            return Err(format!("Could not load {name}").into());
        }
        Ok(())
    }

    pub fn refresh(window: &mut ViewerWindow) -> Result<()>
    {
        window.viewer().invalidate_render_cache(true, true)?;
        window.viewer().refresh_layers()?;
        Ok(())
    }

    pub fn renderer(window: &mut ViewerWindow) -> Result<Value>
    {
        let text = window.viewer().get_layer_symbol_renderer_json(0)?;
        Ok(if text.trim().is_empty() { json!({}) } else { serde_json::from_str(&text)? })
    }

    pub fn legend(window: &mut ViewerWindow, point: bool) -> Result<usize>
    {
        let value = renderer(window)?;
        let mut items = Vec::new();
        for key in ["categories", "ranges", "rules"] {
            if let Some(rows) = value[key].as_array() {
                for row in rows {
                    let mut item = row.clone();
                    item["shape"] = json!(if point { "point" } else { "polygon" });
                    if item["label"].as_str().is_none_or(str::is_empty) {
                        item["label"] = json!(row["value"].to_string());
                    }
                    items.push(item);
                }
            }
        }
        if !window.set_legend_items_json(&json!(items).to_string())? {
            return Err("Could not update legend".into());
        }
        Ok(items.len())
    }
}
