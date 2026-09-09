mod bootstrap;
use geokernel::{Runtime, ViewerTool, ViewerWindow};
use std::{error::Error, path::PathBuf, time::{Duration, Instant}};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const PROGRESS_ID: i32 = 1;
const PROGRESS_PANEL: &str = r#"{"title":"Project progress","width":260,"controls":[{"id":1,"type":"progress","label":"Loading project","value":0,"format":"%p%"}]}"#;

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    window.add_navigation_toolbar();

    if !window.add_display_panel(PROGRESS_PANEL)? {
        return Err("Could not create the project progress panel".into());
    }

    window.show()?;
    window.process_events();

    window.viewer().use_tool(ViewerTool::Pan);
    window.set_status_text("Loading andalucia.geokernel...")?;

    if !window.open_project_with_progress(path, PROGRESS_ID)? {
        window.set_control_value(PROGRESS_ID, 0.0, "%p%")?;
        window.set_status_text("Project could not be loaded.")?;
        return Err(format!("Project could not be loaded: {path}").into());
    }

    window.set_control_value(PROGRESS_ID, 100.0, "%p%")?;
    window.set_status_text("Rendering map...")?;

    // Keep the saved project extent and styles, as the Qt sample does.
    let loaded_at = Instant::now();
    let mut progress_finished = false;

    loop {
        window.process_events();

        if !progress_finished && loaded_at.elapsed() >= Duration::from_millis(900) {
            window.set_status_text("Project loaded.")?;
            window.set_control_value(PROGRESS_ID, 0.0, "%p%")?;
            progress_finished = true;
        }

         if !window.is_visible()? {
            break;
        }

    }

    Ok(())
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data/andalucia/andalucia.geokernel");

    let path = path.canonicalize().map_err(|error| format!("{}: {error}. Run ./run.ps1 -Example Project first.", path.display()))?;
    let path = path.to_str().ok_or("Project path is not valid Unicode")?;

    // Trusted matching SDK; this executable owns the main OS thread.
    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| {
        let mut window = unsafe { ViewerWindow::new(&runtime, "Project", 1200, 800)? };
        run(&mut window, path)
    })();

    // The window drops before Qt/GPU shutdown, also when project loading fails.
    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    outcome?;
    shutdown?;

    Ok(())
}
