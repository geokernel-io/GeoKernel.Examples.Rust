use std::{env, error::Error, path::Path, process::Command};

fn set_environment(name: &str, value: impl AsRef<std::ffi::OsStr>) -> Result<(), Box<dyn Error>>
{
    env::set_var(name, value.as_ref());
    // GDAL/Qt read the C runtime's environment, which Rust's Windows setter
    // does not update. Synchronize it before loading any native SDK modules.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "ucrt")]
        extern "C" {
            fn _wputenv_s(name: *const u16, value: *const u16) -> i32;
        }
        let name: Vec<u16> = std::ffi::OsStr::new(name).encode_wide().chain(Some(0)).collect();
        let value: Vec<u16> = value.as_ref().encode_wide().chain(Some(0)).collect();
        if unsafe { _wputenv_s(name.as_ptr(), value.as_ptr()) } != 0 {
            return Err("Could not configure the native SDK environment".into());
        }
    }
    Ok(())
}

pub fn prepare() -> Result<(), Box<dyn Error>>
{
    if !cfg!(target_os = "windows") {
        return Err("This example launcher currently supports Windows x64".into());
    }
    let example = Path::new(env!("CARGO_MANIFEST_DIR"));
    let status = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(example.join("run.ps1"))
        .arg("-PrepareOnly")
        .status()?;
    if !status.success() {
        return Err("Could not prepare the published GeoKernel SDK and sample data".into());
    }

    let package = include_str!("../../Cargo.lock")
        .split("[[package]]")
        .find(|entry| entry.lines().any(|line| line == "name = \"geokernel\""))
        .ok_or("geokernel is missing from Cargo.lock")?;
    let version = package.lines()
        .find_map(|line| line.strip_prefix("version = ").map(|value| value.trim_matches('"')))
        .ok_or("geokernel version is missing from Cargo.lock")?;
    let sdk = example.join("../packages/GeoKernel").join(version).join("windows-x64").canonicalize()?;
    set_environment("GEOKERNEL_BIN", &sdk)?;
    set_environment("QT_PLUGIN_PATH", sdk.join("plugins"))?;
    set_environment("QT_QPA_PLATFORM_PLUGIN_PATH", sdk.join("plugins/platforms"))?;
    set_environment("GDAL_DRIVER_PATH", sdk.join("gdalplugins"))?;
    set_environment("GDAL_DATA", sdk.join("gdal-data"))?;
    set_environment("PROJ_DATA", sdk.join("proj-data"))?;
    let mut paths = vec![sdk];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    set_environment("PATH", env::join_paths(paths)?)?;
    Ok(())
}
