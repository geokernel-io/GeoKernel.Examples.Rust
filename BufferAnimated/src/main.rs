mod bootstrap;
use geokernel::{Extent, ViewerWindow};
use runtime::Result;
use serde_json::json;
use std::time::{Duration, Instant};

fn render(window: &mut ViewerWindow, distance: f64) -> Result<()>
{
    let buffer_style = json!({"fillColor":"#A7D8F0","fillOpacity":130,"lineColor":"#247BA0","lineWidth":2}).to_string();
    let source_style = json!({"fillColor":"#F9C74F","fillOpacity":100,"pointColor":"#D95D39","pointSize":13,"lineColor":"#D95D39","lineWidth":2}).to_string();
    window.viewer().clear_shapes()?;
    if !window.viewer().add_point_buffer_shape(0.0, 0.0, distance, 18, &buffer_style)? { return Err("Buffer operation failed".into()); }
    window.viewer().add_point_shape(0.0, 0.0, &source_style)?;
    window.set_status_text(&format!("BufferAnimated — buffer distance: {distance:.2} map units"))?;
    runtime::refresh(window)?;
    Ok(())
}

fn run(window: &mut ViewerWindow, _path: &str) -> Result<()>
{
    window.add_navigation_toolbar();
    let events = window.add_control_panel(&json!({"title":"BufferAnimated","controls":[
        {"id":1,"type":"number","label":"Buffer distance","minimum":0.05,"maximum":5,"step":0.05,"value":0.35},
        {"id":2,"type":"button","text":"Full Extent"}, {"id":3,"type":"button","text":"Pause / Resume"}, {"id":4,"type":"number","label":"Frame interval (ms)","minimum":20,"maximum":200,"step":10,"value":60}
    ]}).to_string())?;
    let mut distance = 0.35;
    let mut playing = true;
    let mut direction = 0.08;
    let mut interval = 60;
    let mut tick = Instant::now();
    render(window, distance)?;
    runtime::show(window)?;
    runtime::pump(window, 100);
    window.viewer().set_view_extent(Extent { x_min:-8.0, y_min:-6.0, x_max:8.0, y_max:6.0 })?;

    while window.is_visible()? {
        window.process_events();
        while let Ok(event) = events.try_recv() {
            match event.id {
                1 => { distance = event.number; render(window, distance)?; }
                2 => window.viewer().set_view_extent(Extent { x_min:-8.0, y_min:-6.0, x_max:8.0, y_max:6.0 })?,
                3 => playing = !playing,
                4 => interval = event.number as u64,
                _ => {}
            }
        }
        if playing && tick.elapsed() >= Duration::from_millis(interval) {
            distance += direction;
            if distance >= 3.0 || distance <= 0.35 { direction = -direction; }
            distance = distance.clamp(0.35, 3.0);
            render(window, distance)?;
            tick = Instant::now();
        }

    }
    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("BufferAnimated", "", run)
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

    pub fn refresh(window: &mut ViewerWindow) -> Result<()>
    {
        window.viewer().invalidate_render_cache(true, true)?;
        window.viewer().refresh_layers()?;
        Ok(())
    }
}
