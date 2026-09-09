mod bootstrap;
use geokernel::{Extent,ViewerWindow};
use runtime::Result;

fn run(window: &mut ViewerWindow,_path:&str)->Result<()>
{
    window.add_navigation_toolbar();
    window.add_log_panel("Tile size comparison")?;
    window.append_log("Left: 256 px tiles. Right: 512 px tiles. Same OSM URL and map extent; declared tile size changes tile selection.")?;
    for (index,size) in [(0,256),(1,512)] {
        let cache=std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("../data/xyz_cache/XyzTileSize/{size}"));
        std::fs::create_dir_all(&cache)?;
        let mut viewer=window.pane(index)?;
        if viewer.add_xyz_layer(&format!("{size} px tiles"),"https://tile.openstreetmap.org/{z}/{x}/{y}.png",0,19,size,"© OpenStreetMap contributors",true,&cache.to_string_lossy())? < 0 { return Err("Could not add XYZ layer".into()); }
        viewer.set_coordinate_system_preset("EPSG:3857")?;
    }
    runtime::show(window)?;
    runtime::pump(window,100);
    let extent=Extent{x_min:-1400000.0,y_min:4100000.0,x_max:4200000.0,y_max:7800000.0};
    for index in 0..2 { window.pane(index)?.set_view_extent(extent)?; }
    let mut previous=[window.pane(0)?.view_extent()?,window.pane(1)?.view_extent()?];

    while window.is_visible()? {
        window.process_events();
        synchronize(window,&mut previous)?;

    }
    Ok(())
}

fn synchronize(window: &mut ViewerWindow, previous: &mut [Extent;2]) -> Result<()>
{
    let left=window.pane(0)?.view_extent()?;
    let right=window.pane(1)?.view_extent()?;
    if !runtime::extent_equal(left,previous[0]) {
        window.pane(1)?.set_view_extent(left)?;
    } else if !runtime::extent_equal(right,previous[1]) {
        window.pane(0)?.set_view_extent(right)?;
    }
    // Each pane fits the requested extent to its own aspect ratio. Record the
    // resulting bounds so our update is not mistaken for another user gesture.
    *previous=[window.pane(0)?.view_extent()?,window.pane(1)?.view_extent()?];
    Ok(())
}

fn main()->Result<()>
{
    bootstrap::prepare()?;
    runtime::main("XyzTileSize","",run)
}

mod runtime
{
    use geokernel::{Extent, Runtime, ViewerWindow};

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
                if matches!(name, "MultiWindowSync" | "LabelCollisionOff" | "XyzTileSize") {
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

    pub fn extent_equal(a: Extent, b: Extent) -> bool
    {
        [(a.x_min, b.x_min), (a.y_min, b.y_min), (a.x_max, b.x_max), (a.y_max, b.y_max)]
            .iter()
            .all(|(x, y)| (x - y).abs() <= 0.00001 * x.abs().max(1.0))
    }
}
