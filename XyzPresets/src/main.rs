mod bootstrap;
use geokernel::{Extent, ViewerWindow};
use runtime::Result;
use serde_json::{json, Value};

// Preset names and templates match the SDK predefinedXyzLayerPresets catalog.
const PRESETS: &[(&str,&str)] = &[("Bing Virtual Earth","http://ecn.t3.tiles.virtualearth.net/tiles/a{q}.jpeg?g=1"),
("CartoDb Dark Matter","http://basemaps.cartocdn.com/dark_all/{z}/{x}/{y}.png"),
("CartoDb Dark Matter (No Labels)","http://basemaps.cartocdn.com/dark_nolabels/{z}/{x}/{y}.png"),
("CartoDb Positron","http://basemaps.cartocdn.com/light_all/{z}/{x}/{y}.png"),
("CartoDb Positron (No Labels)","http://basemaps.cartocdn.com/light_nolabels/{z}/{x}/{y}.png"),
("Esri Boundaries Places","https://server.arcgisonline.com/ArcGIS/rest/services/Reference/World_Boundaries_and_Places/MapServer/tile/{z}/{y}/{x}"),
("Esri Gray (dark)","http://services.arcgisonline.com/ArcGIS/rest/services/Canvas/World_Dark_Gray_Base/MapServer/tile/{z}/{y}/{x}"),
("Esri Gray (light)","http://services.arcgisonline.com/ArcGIS/rest/services/Canvas/World_Light_Gray_Base/MapServer/tile/{z}/{y}/{x}"),
("Esri Hillshade","http://services.arcgisonline.com/ArcGIS/rest/services/Elevation/World_Hillshade/MapServer/tile/{z}/{y}/{x}"),
("Esri National Geographic","http://services.arcgisonline.com/ArcGIS/rest/services/NatGeo_World_Map/MapServer/tile/{z}/{y}/{x}"),
("Esri Navigation Charts","http://services.arcgisonline.com/ArcGIS/rest/services/Specialty/World_Navigation_Charts/MapServer/tile/{z}/{y}/{x}"),
("Esri Ocean","https://services.arcgisonline.com/ArcGIS/rest/services/Ocean/World_Ocean_Base/MapServer/tile/{z}/{y}/{x}"),
("Esri Physical Map","https://services.arcgisonline.com/ArcGIS/rest/services/World_Physical_Map/MapServer/tile/{z}/{y}/{x}"),
("Esri Satellite","https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{z}/{y}/{x}"),
("Esri Shaded Relief","https://server.arcgisonline.com/ArcGIS/rest/services/World_Shaded_Relief/MapServer/tile/{z}/{y}/{x}"),
("Esri Standard","https://server.arcgisonline.com/ArcGIS/rest/services/World_Street_Map/MapServer/tile/{z}/{y}/{x}"),
("Esri Topo World","http://services.arcgisonline.com/ArcGIS/rest/services/World_Topo_Map/MapServer/tile/{z}/{y}/{x}"),
("Esri Transportation","https://server.arcgisonline.com/ArcGIS/rest/services/Reference/World_Transportation/MapServer/tile/{z}/{y}/{x}"),
("Google Maps","https://mt1.google.com/vt/lyrs=m&x={x}&y={y}&z={z}"),
("Google Satellite","https://mt1.google.com/vt/lyrs=s&x={x}&y={y}&z={z}"),
("Google Satellite Hybrid","https://mt1.google.com/vt/lyrs=y&x={x}&y={y}&z={z}"),
("Google Terrain","https://mt1.google.com/vt/lyrs=t&x={x}&y={y}&z={z}"),
("Google Terrain Hybrid","https://mt1.google.com/vt/lyrs=p&x={x}&y={y}&z={z}"),
("Mapzen Global Terrain","https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png"),
("Gempa","https://demo.gempa.de/gaps/tiles/{z}/{y}/{x}"),
("OpenStreetMap","https://tile.openstreetmap.org/{z}/{x}/{y}.png"),
("OpenTopoMap","https://tile.opentopomap.org/{z}/{x}/{y}.png")];

struct Settings
{
    url: String,
    min: i32,
    max: i32,
    cache: bool,
}

fn apply(window: &mut ViewerWindow, settings: &Settings) -> Result<()>
{
    if settings.min > settings.max { return Err("Minimum zoom must not exceed maximum zoom".into()); }
    let preset = PRESETS.iter().find(|preset| preset.1 == settings.url).ok_or("Unknown XYZ preset URL")?;
    // The SDK stores tiles by size/z/x/y inside an explicitly supplied cache directory.
    // Separate providers so the same tile coordinates cannot reuse another basemap.
    let folder: String = preset.0.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect();
    let cache = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/xyz_cache/XyzPresets").join(folder);
    std::fs::create_dir_all(&cache)?;
    window.viewer().clear_layers()?;
    if window.viewer().add_xyz_layer(preset.0,&settings.url,settings.min,settings.max,256,"",settings.cache,&cache.to_string_lossy())? < 0 { return Err("Could not add XYZ layer".into()); }
    window.viewer().set_coordinate_system_preset("EPSG:3857")?;
    window.set_attribution_text("© OpenStreetMap contributors (default source); other sources retain their own attribution")?;
    Ok(())
}

fn diagnostics(window: &mut ViewerWindow) -> Result<()>
{
    let value: Value=serde_json::from_str(&window.viewer().get_xyz_layer_diagnostics_json(0)?)?;
    if value.as_object().is_none_or(serde_json::Map::is_empty) { return Err("XYZ diagnostics unavailable".into()); }
    window.clear_log()?;
    let text=serde_json::to_string_pretty(&value)?.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;");
    window.append_log(&format!("<pre>{text}</pre>"))?;
    Ok(())
}

fn run(window: &mut ViewerWindow, _path: &str) -> Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("XYZ diagnostics")?;
    let controls=window.add_control_panel(&json!({"title":"XyzPresets","controls":[
        {"id":4,"type":"combo","label":"Local cache","options":["Disabled","Enabled"],"value":"Enabled"},{"id":8,"type":"combo","label":"Preset","options":PRESETS.iter().map(|p|p.0).collect::<Vec<_>>(),"value":"OpenStreetMap"},
        {"id":5,"type":"button","text":"Apply"},
        {"id":6,"type":"button","text":"Refresh Stats"},
        {"id":7,"type":"button","text":"Full Extent"}
    ]}).to_string())?;
    let mut settings=Settings {url:"https://tile.openstreetmap.org/{z}/{x}/{y}.png".into(),min:0,max:19,cache:true};
    apply(window,&settings)?;
    runtime::show(window)?;
    runtime::pump(window,100);
    window.viewer().set_view_extent(Extent {x_min:-1400000.0,y_min:4100000.0,x_max:4200000.0,y_max:7800000.0})?;

    while window.is_visible()? {
        window.process_events();
        while let Ok(event)=controls.try_recv() {
            match event.id {
                1 => settings.url=event.text,
                2 => settings.min=event.number as i32,
                3 => settings.max=event.number as i32,
                4 => settings.cache=event.text=="Enabled",
                5 => { if let Err(error)=apply(window,&settings) { window.set_status_text(&error.to_string())?; } },
                6 => diagnostics(window)?,
                7 => window.viewer().set_view_extent(Extent {x_min:-1400000.0,y_min:4100000.0,x_max:4200000.0,y_max:7800000.0})?,
                8 => {
                    if let Some(preset)=PRESETS.iter().find(|preset| preset.0==event.text) {
                        settings.url=preset.1.into();
                        match apply(window,&settings) {
                            Ok(()) => window.set_status_text(&format!("Basemap: {}", preset.0))?,
                            Err(error) => window.set_status_text(&error.to_string())?,
                        }
                    }
                },
                _ => {}
            }
        }

    }
    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("XyzPresets","",run)
}

mod runtime
{
    use geokernel::{Runtime, ViewerWindow};

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
}
