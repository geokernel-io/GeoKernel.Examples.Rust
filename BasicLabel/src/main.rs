mod bootstrap;
use geokernel::ViewerWindow;
use runtime::Result;

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    labels::run(window, path, "BasicLabel")
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("BasicLabel", "world_4326/world_4326.shp", run)
}

mod labels
{
    use crate::runtime::{self, Result};
    use geokernel::{ControlEvent, Extent, ViewerTool, ViewerWindow};
    use serde_json::{json, Value};

    fn combo(id: i32, label: &str, options: Value, value: &str) -> Value
    {
        json!({"id":id,"type":"combo","label":label,"options":options,"value":value})
    }

    fn number(id: i32, label: &str, min: f64, max: f64, step: f64, value: f64) -> Value
    {
        json!({"id":id,"type":"number","label":label,"minimum":min,"maximum":max,"step":step,"decimals":1,"value":value})
    }

    fn change(window: &mut ViewerWindow, name: &str, event: ControlEvent) -> Result<()>
    {
        let patch = match (name, event.id) {
            ("BasicLabel", 1) => json!({"showLabels":event.text == "Yes"}),
            ("BasicLabel", 2) => json!({"labelField":event.text}),
            ("BasicLabel", 3) => json!({"labelFontSize":event.number}),
            ("LabelFont", 1) => json!({"labelFontFamily":event.text}),
            ("LabelFont", 2) => json!({"labelBold":event.text == "Yes"}),
            ("LabelFont", 3) => json!({"labelItalic":event.text == "Yes"}),
            ("LabelHalo", 1) => json!({"labelHaloEnabled":event.text == "Yes"}),
            ("LabelHalo", 2) => json!({"labelHaloColor":match event.text.as_str() {"Black"=>"#000000","Yellow"=>"#FFF2A8","Blue"=>"#BAE6FD",_=>"#FFFFFF"}}),
            ("LabelHalo", 3) => json!({"labelHaloWidth":event.number}),
            ("LabelOffset", 1) => json!({"labelOffsetX":event.number}),
            ("LabelOffset", 2) => json!({"labelOffsetY":event.number}),
            ("LabelOffset", 3) => {
                window.set_control_value(1, 0.0, "")?;
                window.set_control_value(2, 0.0, "")?;
                json!({"labelOffsetX":0.0,"labelOffsetY":0.0})
            }
            ("LabelRotation", 1) => json!({"labelRotationDegrees":event.number}),
            ("LabelRotation", 3) => {
                window.set_control_value(1, 0.0, "")?;
                json!({"labelRotationDegrees":0.0})
            }
            _ => return Err("Unknown label control".into()),
        };
        if !window.viewer().set_layer_style_json(0, &patch.to_string())? {
            return Err("Label style update failed".into());
        }
        runtime::refresh(window)?;
        window.set_status_text(&format!("{name}: {patch}"))?;
        runtime::check_style(&serde_json::from_str(&window.viewer().get_layer_style_json(0)?)?, &patch)?;
        Ok(())
    }

    pub fn run(window: &mut ViewerWindow, path: &str, name: &str) -> Result<()>
    {
        if name == "LabelCollisionOff" {
            return collision(window, path);
        }
        runtime::show(window)?;
        let style = json!({"fillColor":"#D8E5E1","fillOpacity":215,"lineColor":"#6F8380","lineWidth":0.8,
            "showLabels":true,"labelField":"COUNTRY","labelFontSize":12.0,"labelFontFamily":"Arial","labelBold":false,"labelItalic":false,
            "labelColor":if name=="BasicLabel" {"#FFFF00"} else if name=="LabelFont" {"#1F2933"} else {"#253238"},
            "labelHaloEnabled":true,"labelHaloColor":if name=="BasicLabel" {"#000000"} else if name=="LabelHalo" {"#FFF2A8"} else {"#FFFFFF"},
            "labelHaloWidth":if name=="LabelHalo" {2.5} else {2.0},"labelOffsetX":0.0,"labelOffsetY":0.0,"labelRotationDegrees":0.0});
        runtime::load(&mut window.viewer(), path, "World - labels", &style)?;
        window.viewer().use_tool(ViewerTool::Pan);
        let yes_no = json!(["No", "Yes"]);
        let controls = match name {
            "BasicLabel" => {
                let fields: Value = serde_json::from_str(&window.viewer().get_layer_attribute_definitions_json(0)?)?;
                let names: Vec<Value> = fields
                    .as_array()
                    .ok_or("Invalid label fields")?
                    .iter()
                    .map(|field| field["name"].clone())
                    .collect();
                vec![
                    combo(1, "Show labels", yes_no, "Yes"),
                    combo(2, "Label field", json!(names), "COUNTRY"),
                    number(3, "Font size", 5.0, 32.0, 1.0, 12.0),
                ]
            }
            "LabelFont" => vec![
                json!({"id":1,"type":"text","label":"Font family","value":"Arial"}),
                combo(2, "Bold", yes_no.clone(), "No"),
                combo(3, "Italic", yes_no, "No"),
            ],
            "LabelHalo" => vec![
                combo(1, "Halo enabled", yes_no, "Yes"),
                combo(2, "Halo color", json!(["White", "Black", "Yellow", "Blue"]), "Yellow"),
                number(3, "Halo width", 0.5, 8.0, 0.5, 2.5),
            ],
            "LabelOffset" => vec![
                number(1, "Offset X", -80.0, 80.0, 2.0, 0.0),
                number(2, "Offset Y", -80.0, 80.0, 2.0, 0.0),
                json!({"id":3,"type":"button","text":"Reset Offset"}),
            ],
            "LabelRotation" => vec![
                number(1, "Rotation (degrees)", -180.0, 180.0, 5.0, 0.0),
                json!({"id":3,"type":"button","text":"Reset Rotation"}),
            ],
            _ => return Err("Unknown label example".into()),
        };
        let rx = window.add_control_panel(&json!({"title":name,"area":"left","width":245,"controls":controls}).to_string())?;
        window.process_events();
        window.viewer().set_view_extent(Extent {
            x_min: -180.0,
            y_min: -58.0,
            x_max: 180.0,
            y_max: 82.0,
        })?;
        window.set_status_text(&format!("{name}: COUNTRY labels"))?;
        while window.is_visible()? {
            window.process_events();
            for event in rx.try_iter() {
                if let Err(error) = change(window, name, event) {
                    window.set_status_text(&error.to_string())?;
                }
            }

        }

        Ok(())
    }

    pub fn collision(window: &mut ViewerWindow, path: &str) -> Result<()>
    {
        let cities = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../data/world_cities_4326/world_cities_4326.shp")
            .to_string_lossy().into_owned();
        runtime::show(window)?;
        window.process_events();
        for index in 0..2 {
            let mut viewer = window.pane(index)?;
            viewer.use_tool(ViewerTool::Pan);
            runtime::load(
                &mut viewer,
                path,
                "World",
                &json!({"fillColor":"#D8E5E1","fillOpacity":215,"lineColor":"#6F8380","lineWidth":0.8}),
            )?;
            let style = json!({"pointColor":"#D56037","lineColor":"#A23D23","pointSize":5.5,"lineWidth":0.8,
                "showLabels":true,"labelField":"CITY_NAME","labelFontSize":8.0,"labelColor":"#1F2933",
                "labelHaloEnabled":true,"labelHaloColor":"#FFFFFF","labelHaloWidth":1.5,
                "labelAllowOverlap":index==1,"labelPlacementMode":1,"labelOffsetX":7.0,"labelOffsetY":-7.0});
            runtime::load(
                &mut viewer,
                &cities,
                if index == 0 {
                    "Cities - collision filtering"
                } else {
                    "Cities - overlap allowed"
                },
                &style,
            )?;
            viewer.set_view_extent(Extent {
                x_min: -127.0,
                y_min: 23.0,
                x_max: -66.0,
                y_max: 50.0,
            })?;
            runtime::check_style(&serde_json::from_str(&viewer.get_layer_style_json(0)?)?, &style)?;
            if viewer.get_layer_feature_count(0)? <= 0 {
                return Err("No city features loaded".into());
            }
        }
        window.set_status_text("Left (Viewer A): collision filtering. Right (Viewer B): label overlap allowed.")?;
        {
            while window.is_visible()? {
                runtime::pump(window, 10);
            }
        }
        Ok(())
    }
}

mod runtime
{
    use geokernel::{Runtime, Viewer, ViewerWindow};
    use serde_json::Value;
    use std::{
        error::Error,
        path::PathBuf,
        time::{Duration, Instant},
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

    pub fn pump(window: &mut ViewerWindow, millis: u64)
    {
        let started = Instant::now();
        while started.elapsed() < Duration::from_millis(millis) {
            window.process_events();
            std::thread::sleep(Duration::from_millis(10));
        }
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

    pub fn check_style(actual: &Value, expected: &Value) -> Result<()>
    {
        for (key, value) in expected.as_object().ok_or("Expected style object")? {
            let matches = match (actual[key].as_f64(), value.as_f64()) {
                (Some(a), Some(b)) => (a - b).abs() < 0.00001,
                _ if value.is_string() => actual[key].as_str().map(str::to_lowercase) == value.as_str().map(str::to_lowercase),
                _ => actual[key] == *value,
            };
            if !matches {
                return Err(format!("Style mismatch for {key}: {} vs {value}", actual[key]).into());
            }
        }
        Ok(())
    }
}
