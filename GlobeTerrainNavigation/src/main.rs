#![windows_subsystem = "windows"]

mod bootstrap;

use geokernel::{
    globe::{GlobeCamera, GlobeOptions, Navigation},
    Globe, Runtime, ViewerWindow,
};
use serde_json::json;
use std::{
    error::Error,
    ffi::c_void,
    thread,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[repr(C)]
#[derive(Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[link(name = "user32")]
extern "system" {
    fn GetClientRect(window: *mut c_void, rect: *mut Rect) -> i32;
    fn MessageBoxW(window: *mut c_void, text: *const u16, title: *const u16, flags: u32) -> i32;
}

fn main() {
    if let Err(error) = open_globe() {
        let text: Vec<u16> = error.to_string().encode_utf16().chain(Some(0)).collect();
        let title: Vec<u16> = "GlobeTerrainNavigation"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        unsafe {
            MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), 0x10);
        }
        std::process::exit(1);
    }
}

fn open_globe() -> Result<()> {
    bootstrap::prepare()?;
    let runtime = unsafe { Runtime::from_env()? };
    // Keep Qt alive until both the Globe and its parent have been dropped.
    let outcome = (|| -> Result<()> {
        let mut window = unsafe {
            ViewerWindow::new(&runtime, "GlobeTerrainNavigation — GeoKernel", 1200, 800)?
        };
        let controls = window.add_control_panel(&json!({
            "title": "Globe", "controls": [
                {"id":1,"type":"combo","label":"Atmosphere","options":["Off","On"],"value":"On"},
                {"id":2,"type":"combo","label":"Esri Satellite","options":["Off","On"],"value":"On"},
                {"id":3,"type":"combo","label":"DEM","options":["Off","On"],"value":"On"},
                {"id":4,"type":"combo","label":"Sunlight","options":["Off","On"],"value":"On"}
            ]
        }).to_string())?;
        let navigation = [
            ("Zoom in", Navigation::ZoomIn),
            ("Zoom out", Navigation::ZoomOut),
            ("Rotate left", Navigation::RotateLeft),
            ("Rotate right", Navigation::RotateRight),
            ("Tilt up", Navigation::TiltUp),
            ("Tilt down", Navigation::TiltDown),
            ("Top view", Navigation::Top),
            ("North", Navigation::North),
            ("Overview", Navigation::Home),
        ];
        let commands: Vec<_> = navigation
            .iter()
            .enumerate()
            .map(|(id, (text, _))| json!({"id": id, "text": text}))
            .collect();
        let navigation_events =
            window.add_command_toolbar(&json!({"commands": commands}).to_string())?;
        window.set_status_text("Left: pan | Right / wheel: zoom | Middle / Ctrl: orbit")?;
        window.show()?;
        window.process_events();
        let parent = window.viewer().get_native_handle()?;
        let home = GlobeCamera {
            longitude: 2.1744,
            latitude: 41.4036,
            target_height_metres: 40.0,
            distance_metres: 2500.0,
            pitch_degrees: 0.65_f64.to_degrees(),
            heading_degrees: 0.0,
        };
        let options = GlobeOptions {
            terrain_enabled: true,
            atmosphere_enabled: true,
            sunlight_enabled: true,
            ..GlobeOptions::default()
        };
        let globe = unsafe { Globe::embedded(&runtime, parent, &options, Some(&home))? };
        let esri = globe
            .basemaps()?
            .into_iter()
            .find(|source| source.name == "Esri Satellite")
            .ok_or("Esri Satellite basemap is unavailable")?
            .id;
        globe.set_basemap(esri)?;
        globe.set_home_camera(&home)?;
        globe.set_visible(true)?;
        let mut size = (0, 0);
        let mut status_time = Instant::now();
        while window.is_visible()? {
            globe.process_events()?;
            if !window.is_visible()? {
                break;
            }
            let mut rect = Rect::default();
            if unsafe { GetClientRect(parent as *mut c_void, &mut rect) } == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let scale = globe.state()?.device_pixel_ratio;
            let current = (
                ((rect.right - rect.left) as f64 / scale).round() as i32,
                ((rect.bottom - rect.top) as f64 / scale).round() as i32,
            );
            if current != size && current.0 > 0 && current.1 > 0 {
                globe.resize(current.0, current.1)?;
                size = current;
            }
            while let Ok(event) = controls.try_recv() {
                let enabled = event.text == "On";
                let mut options = globe.options()?;
                match event.id {
                    1 => options.atmosphere_enabled = enabled,
                    2 => options.basemap = if enabled { esri } else { -1 },
                    3 => options.terrain_enabled = enabled,
                    4 => options.sunlight_enabled = enabled,
                    _ => continue,
                }
                globe.set_options(&options)?;
            }
            while let Ok(id) = navigation_events.try_recv() {
                if let Some((_, action)) = navigation.get(id as usize) {
                    globe.navigate(*action)?;
                }
            }
            while globe.poll_event()?.is_some() {}
            if status_time.elapsed() >= Duration::from_millis(250) {
                let state = globe.state()?;
                let mut messages = vec![globe.attribution()?];
                messages.extend(
                    state
                        .errors
                        .into_values()
                        .filter(|message| !message.is_empty()),
                );
                messages.retain(|message| !message.is_empty());
                window.set_status_text(
                    if messages.is_empty() {
                        "Left: pan | Right / wheel: zoom | Middle / Ctrl: orbit".into()
                    } else {
                        messages.join(" | ")
                    }
                    .as_str(),
                )?;
                status_time = Instant::now();
            }
            thread::sleep(Duration::from_millis(16));
        }
        Ok(())
    })();
    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;
    Ok(())
}
