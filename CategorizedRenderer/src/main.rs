mod bootstrap;
use geokernel::ViewerWindow;
use runtime::Result;

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    classification::run(window, path, "CategorizedRenderer")
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("CategorizedRenderer", "usa_states_3857/usa_states_3857.shp", run)
}

mod classification
{
    use crate::runtime::{self, Result};
    use geokernel::{ControlEvent, Extent, GraduatedOptions, ViewerTool, ViewerWindow};
    use serde_json::{json, Value};

    const METHODS: [&str; 11] = [
        "Manual",
        "Defined Interval",
        "Equal Interval",
        "Quantile",
        "Natural Breaks",
        "Quartile",
        "Standard Deviation",
        "Standard Deviation with Central",
        "Geometrical Interval",
        "K-Means",
        "K-Means Spatial",
    ];

    struct Settings {
        categorized: bool,
        field: String,
        method: i32,
        classes: i32,
        ramp: String,
        interval: f64,
        breaks: Vec<f64>,
        ramp_mode: i32,
        reverse: bool,
        target: i32,
    }

    fn apply(window: &mut ViewerWindow, state: &Settings) -> Result<()>
    {
        let success = if state.categorized {
            window
                .viewer()
                .apply_layer_categorized_renderer(0, &state.field, &state.ramp, state.classes, state.reverse, 100000, state.target, 3.0, 36.0)?
        } else {
            window.viewer().apply_graduated_renderer(
                0,
                &GraduatedOptions {
                    field: &state.field,
                    method: state.method,
                    classes: state.classes,
                    ramp: &state.ramp,
                    interval: state.interval,
                    breaks: &state.breaks,
                    ramp_mode: state.ramp_mode,
                    reverse: state.reverse,
                    target: state.target,
                    start_size: 3.0,
                    end_size: 36.0,
                },
            )?
        };
        if !success {
            return Err(format!("Classification failed for {}", state.field).into());
        }
        runtime::refresh(window)?;
        if runtime::legend(window, false)? == 0 {
            return Err("Renderer has no legend entries".into());
        }
        window.set_status_text(&format!(
            "Classification applied: {} / {}",
            state.field,
            if state.categorized { "Categorized" } else { METHODS[state.method as usize] }
        ))?;
        Ok(())
    }

    fn controls(window: &mut ViewerWindow, name: &str, state: &Settings) -> Result<std::sync::mpsc::Receiver<ControlEvent>>
    {
        let ramps: Value = serde_json::from_str(&window.viewer().get_color_ramp_names_json()?)?;
        let mut items = Vec::new();
        if name == "Classification" {
            let definitions: Value = serde_json::from_str(&window.viewer().get_layer_attribute_definitions_json(0)?)?;
            let fields: Vec<Value> = definitions
                .as_array()
                .ok_or("Invalid field definitions")?
                .iter()
                .filter(|field| state.categorized || matches!(field["type"].as_i64(), Some(1 | 2)))
                .filter_map(|field| field["name"].as_str().map(|s| json!(s)))
                .collect();
            items.extend([
                json!({"id":1,"type":"combo","label":"Renderer","options":["Categorized","Graduated"],"value":"Graduated"}),
                json!({"id":2,"type":"combo","label":"Field","options":fields,"value":state.field}),
                json!({"id":4,"type":"number","label":"Classes / Categories","minimum":2,"maximum":64,"decimals":0,"step":1,"value":state.classes}),
                json!({"id":5,"type":"number","label":"Interval / Std dev step","minimum":0.0001,"maximum":1000000000,"decimals":4,"value":state.interval}),
                json!({"id":6,"type":"text","label":"Manual breaks","value":"0, 100000, 500000, 1000000, 5000000, 10000000"}),
                json!({"id":7,"type":"combo","label":"Render by","options":["Color","Size / Width","Outline color","Outline width"],"value":"Color"}),
                json!({"id":9,"type":"combo","label":"Ramp mode","options":["Continuous","Discrete"],"value":"Continuous"}),
                json!({"id":10,"type":"combo","label":"Reverse","options":["No","Yes"],"value":"No"}),
            ]);
        }
        if name == "ClassificationMethods" || name == "Classification" {
            let methods = if name == "ClassificationMethods" {
                json!(["Equal Interval", "Quantile", "Standard Deviation"])
            } else {
                json!(METHODS)
            };
            items.push(json!({"id":3,"type":"combo","label":"Method","options":methods,"value":METHODS[state.method as usize]}));
        }
        if name == "GraduatedRenderer" || name == "Classification" {
            items.push(json!({"id":8,"type":"combo","label":"Color ramp","options":ramps,"value":state.ramp}));
        }
        if name == "ClearRenderer" || name == "Classification" {
            items.push(json!({"id":11,"type":"button","text":if name=="ClearRenderer" { "Apply Categorized Renderer" } else { "Apply" }}));
            items.push(json!({"id":12,"type":"button","text":"Clear Renderer"}));
            items.push(json!({"id":13,"type":"button","text":"Full Extent"}));
        }
        if items.is_empty() {
            items.push(json!({"id":13,"type":"button","text":"Full Extent"}));
        }
        window
            .add_control_panel(&json!({"title":"Classification controls","area":"right","width":270,"controls":items}).to_string())
            .map_err(Into::into)
    }

    fn sync_controls(window: &mut ViewerWindow, state: &Settings) -> Result<()>
    {
        window.set_control_enabled(3, !state.categorized)?;
        window.set_control_enabled(4, state.categorized || !matches!(state.method, 0 | 1 | 5 | 6 | 7))?;
        window.set_control_enabled(5, !state.categorized && matches!(state.method, 1 | 6 | 7))?;
        window.set_control_enabled(6, !state.categorized && state.method == 0)?;
        window.set_control_enabled(9, !state.categorized)?;
        Ok(())
    }

    fn change(window: &mut ViewerWindow, state: &mut Settings, event: ControlEvent, name: &str) -> Result<()>
    {
        match event.id {
            1 => {
                state.categorized = event.text == "Categorized";
                state.field = if state.categorized { "STATEFP" } else { "POPULATION" }.into();
                state.ramp = if state.categorized { "Unique" } else { "GreenBlue" }.into();
                let definitions: Value = serde_json::from_str(&window.viewer().get_layer_attribute_definitions_json(0)?)?;
                let fields: Vec<Value> = definitions.as_array().ok_or("Invalid fields")?.iter()
                    .filter(|field| state.categorized || matches!(field["type"].as_i64(), Some(1 | 2)))
                    .map(|field| field["name"].clone()).collect();
                window.set_control_options_json(2, &json!(fields).to_string())?;
                window.set_control_value(2, 0.0, &state.field)?;
                window.set_control_value(8, 0.0, &state.ramp)?;
                window.set_control_enabled(3, !state.categorized)?;
            }
            2 => state.field = event.text,
            3 => {
                state.method = METHODS.iter().position(|m| *m == event.text).ok_or("Invalid method")? as i32;
                if matches!(state.method, 6 | 7) && state.interval > 10.0 {
                    state.interval = 1.0;
                    if name == "Classification" {
                        window.set_control_value(5, 1.0, "")?;
                    }
                }
            }
            4 => state.classes = event.number as i32,
            5 => state.interval = event.number,
            6 => {
                let mut breaks = event
                    .text
                    .split([',', ';', ' '])
                    .filter(|s| !s.is_empty())
                    .map(str::parse::<f64>)
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                if breaks.len() < 2 || !breaks.iter().all(|v| v.is_finite()) {
                    return Err("Manual mode requires at least two finite breaks".into());
                }
                breaks.sort_by(f64::total_cmp);
                state.breaks = breaks;
            }
            7 => {
                state.target = ["Color", "Size / Width", "Outline color", "Outline width"]
                    .iter()
                    .position(|s| *s == event.text)
                    .ok_or("Invalid style target")? as i32
            }
            8 => state.ramp = event.text,
            9 => state.ramp_mode = i32::from(event.text == "Discrete"),
            10 => state.reverse = event.text == "Yes",
            11 => return apply(window, state),
            12 => {
                if !window.viewer().clear_layer_symbol_renderer(0)? {
                    return Err("Could not clear renderer".into());
                }
                runtime::legend(window, false)?;
                runtime::refresh(window)?;
                window.set_status_text("Renderer cleared; default layer style")?;
                return Ok(());
            }
            13 => {
                window.viewer().zoom_to_layer(0)?;
                return Ok(());
            }
            _ => return Err("Unknown classification control".into()),
        }
        if name != "Classification" {
            apply(window, state)?;
        } else {
            sync_controls(window, state)?;
        }
        Ok(())
    }

    pub fn run(window: &mut ViewerWindow, path: &str, name: &str) -> Result<()>
    {
        let categorized = matches!(name, "CategorizedRenderer" | "ClearRenderer");
        let mut state = Settings {
            categorized,
            field: if categorized { "STATE" } else { "POPULATION" }.into(),
            method: if name == "ClassificationMethods" { 2 } else { 4 },
            classes: if categorized {
                64
            } else if name == "Classification" {
                15
            } else {
                5
            },
            ramp: if categorized { "Unique" } else { "GreenBlue" }.into(),
            interval: 100000.0,
            breaks: vec![0.0, 100000.0, 500000.0, 1000000.0, 5000000.0, 10000000.0],
            ramp_mode: 0,
            reverse: false,
            target: 0,
        };
        runtime::show(window)?;
        window.viewer().use_tool(ViewerTool::Pan);
        window.viewer().add_open_street_map_layer(true)?;
        let style = json!({"fillColor":if categorized {"#D8E5E1"} else {"#DCE8E4"},"fillOpacity":if categorized || name=="Classification" {220} else {225},"lineColor":"#536B68","lineWidth":if categorized {0.9} else {0.8}});
        runtime::load(
            &mut window.viewer(),
            path,
            if categorized { "USA States" } else { "California counties" },
            &style,
        )?;
        if !window.add_legend_panel(if categorized { "STATE categories" } else { "POPULATION classes" })? {
            return Err("Legend creation failed".into());
        }
        apply(window, &state)?;
        let rx = controls(window, name, &state)?;
        if name == "Classification" { sync_controls(window, &state)?; }
        // Fit after the dock panels have established the map viewport size.
        window.process_events();
        let initial_extent = Extent {
            x_min: -16831516.0,
            y_min: 1856556.0,
            x_max: -4631023.0,
            y_max: 7472472.0,
        };
        if categorized {
            // Match the Qt examples' continental USA view, excluding overseas territories.
            window.viewer().set_view_extent(initial_extent)?;
        } else {
            window.viewer().zoom_to_layer(0)?;
        }
        while window.is_visible()? {
            window.process_events();
            for event in rx.try_iter() {
                if let Err(error) = change(window, &mut state, event, name) {
                    window.set_status_text(&error.to_string())?;
                }
            }

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
