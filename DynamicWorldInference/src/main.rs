use geokernel_geoai_examples::{runtime, DynamicWorldExample, Result};
use std::{thread, time::Duration};

fn main() -> Result<()> {
    let runtime = runtime::load()?;
    {
        let mut example = DynamicWorldExample::new(&runtime)?;
        example.window.show()?;
        while !example.closed {
            if !example.window.is_visible()? {
                example.begin_close()?;
            }
            if let Err(error) = example.tick() {
                eprintln!("{error}");
                example.begin_close()?;
            }
            thread::sleep(Duration::from_millis(16));
        }
    }
    unsafe {
        runtime.shutdown_viewer_application()?;
    }
    Ok(())
}
