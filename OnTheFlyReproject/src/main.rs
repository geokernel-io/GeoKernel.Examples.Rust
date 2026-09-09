mod bootstrap;
use geokernel::{Extent, ViewerTool, ViewerWindow};
use runtime::Result;
use serde_json::json;

fn apply(window: &mut ViewerWindow, preset: &str) -> Result<()>
{
    if !window.viewer().set_coordinate_system_preset(preset)? { return Err(format!("Unknown CRS: {preset}").into()); }
    if preset == "EPSG:4326" {
        window.viewer().set_view_extent(Extent { x_min:-180.0,y_min:-85.0,x_max:180.0,y_max:85.0 })?;
    } else if preset == "EPSG:3857" {
        window.viewer().set_view_extent(Extent { x_min:-20037508.34,y_min:-20037508.34,x_max:20037508.34,y_max:20037508.34 })?;
    } else {
        let (x,y) = match preset {
            "EPSG:3395" => (20037508.34, 20000000.0),
            "Miller" => (20037508.34, 15500000.0),
            "Sinusoidal" => (20037508.34, 10500000.0),
            _ => (18500000.0, 9500000.0)
        };
        window.viewer().set_view_extent(Extent { x_min:-x,y_min:-y,x_max:x,y_max:y })?;
    }
    window.set_status_text(&format!("Viewer CRS: {preset}; source layer remains EPSG:4326"))?;
    Ok(())
}

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    window.add_navigation_toolbar();
    let controls = window.add_control_panel(&json!({"title":"OnTheFlyReproject","controls":[
        {"id":1,"type":"button","text":"Full Extent"}, {"id":2,"type":"combo","label":"Viewer CRS","options":["EPSG:4326","EPSG:3857","EPSG:3395","Miller","Mollweide","Sinusoidal","Eckert IV","Eckert VI"],"value":"EPSG:4326"}
    ]}).to_string())?;
    let events = window.subscribe_events();
    if !window.viewer().add_layer_file(path)? { return Err("Could not load world".into()); }
    let _diagnostics = projection_diagnostics::install()?;
    if !window.viewer().set_layer_coordinate_system_preset(0,"EPSG:4326")? {
        return Err("Could not set source layer CRS to EPSG:4326".into());
    }
    window.viewer().use_tool(ViewerTool::Pan);
    runtime::show(window)?;
    let mut preset = String::from("EPSG:4326");
    apply(window, &preset)?;

    while window.is_visible()? {
        window.process_events();
        while let Ok(control)=controls.try_recv() {
            match control.id {
                1 => apply(window, &preset)?,
                2 => { apply(window, &control.text)?; preset = control.text; },
                _ => {}
            }
        }
        while let Ok(event)=events.try_recv() {
            if event.data.event_type==19 {
                let e=event.data.extent;
                window.set_status_text(&format!("{preset}: {:.6}, {:.6}",e.x_min,e.y_min))?;
            }
        }

    }
    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("OnTheFlyReproject","world_4326/world_4326.shp",run)
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

// This standalone application filters only expected projection-domain diagnostics.
mod projection_diagnostics
{
    use std::ffi::{c_char, c_void, CStr};
    use std::sync::OnceLock;
    type Handler = unsafe extern "system" fn(i32, i32, *const c_char);
    type SetHandler = unsafe extern "system" fn(Option<Handler>) -> Option<Handler>;
    static DEFAULT: OnceLock<Handler> = OnceLock::new();

    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    }

    fn is_domain_error(level: i32, message: &[u8]) -> bool
    {
        level == 3 && (message == b"Point outside of projection domain"
            || message == b"Reprojection failed, err = 2050, further errors will be suppressed on the transform object.")
    }

    unsafe extern "system" fn filter(level: i32, number: i32, message: *const c_char)
    {
        if !message.is_null() && is_domain_error(level, CStr::from_ptr(message).to_bytes()) { return; }
        if let Some(default) = DEFAULT.get() { default(level, number, message); }
    }

    pub struct Guard { set: SetHandler, previous: Option<Handler> }
    impl Drop for Guard {
        fn drop(&mut self) { unsafe { (self.set)(self.previous); } }
    }

    pub fn install() -> crate::runtime::Result<Guard>
    {
        // Loading the sample layer already loaded the SDK GDAL dependency.
        // The guard restores diagnostics before the application releases the SDK.
        unsafe {
            let name: Vec<u16> = "gdal.dll".encode_utf16().chain(Some(0)).collect();
            let module = GetModuleHandleW(name.as_ptr());
            if module.is_null() { return Err("SDK GDAL module is not loaded".into()); }
            let set = GetProcAddress(module, b"CPLSetErrorHandler\0".as_ptr().cast());
            let default = GetProcAddress(module, b"CPLDefaultErrorHandler\0".as_ptr().cast());
            if set.is_null() || default.is_null() { return Err("GDAL diagnostic API is unavailable".into()); }
            let set: SetHandler = std::mem::transmute(set);
            let default: Handler = std::mem::transmute(default);
            let _ = DEFAULT.set(default);
            Ok(Guard { set, previous: set(Some(filter)) })
        }
    }

    #[test]
    fn filters_only_expected_nonfatal_projection_messages()
    {
        assert!(is_domain_error(3, b"Point outside of projection domain"));
        assert!(is_domain_error(3, b"Reprojection failed, err = 2050, further errors will be suppressed on the transform object."));
        assert!(!is_domain_error(4, b"Point outside of projection domain"));
        assert!(!is_domain_error(3, b"Cannot open dataset"));
        assert!(!is_domain_error(3, b"Reprojection failed, err = 2049"));
    }
}
