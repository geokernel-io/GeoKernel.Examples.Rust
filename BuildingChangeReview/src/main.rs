use building_change_review::ui::App;
use geokernel_geoai_examples::{runtime, Result};
use std::{thread, time::Duration};
fn main() -> Result<()> {
    let runtime = runtime::load()?;
    {
        let mut app = App::new(&runtime)?;
        while !app.closed {
            if let Err(e) = app.tick() {
                eprintln!("{e}");
                app.begin_close();
            }
            thread::sleep(Duration::from_millis(16));
        }
    }
    unsafe {
        runtime.shutdown_viewer_application()?;
    }
    Ok(())
}
