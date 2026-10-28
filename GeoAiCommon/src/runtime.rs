use std::{env, path::PathBuf};

pub fn load() -> crate::Result<geokernel::Runtime> {
    if !cfg!(windows) {
        return Err("This example currently supports Windows x64".into());
    }
    let bin = if let Some(bin) = env::var_os("GEOKERNEL_BIN") {
        PathBuf::from(bin)
    } else {
        prepare_published_sdk()?
    };
    let bin = bin.canonicalize()?;
    let plugins = if bin.join("plugins/platforms/qwindows.dll").is_file() {
        bin.join("plugins")
    } else {
        bin.clone()
    };
    if !plugins.join("platforms/qwindows.dll").is_file() {
        return Err("Matching Qt platform plugins are missing; deploy Qt with windeployqt".into());
    }
    set("GEOKERNEL_BIN", bin.as_os_str())?;
    set("QT_PLUGIN_PATH", plugins.as_os_str())?;
    set(
        "QT_QPA_PLATFORM_PLUGIN_PATH",
        plugins.join("platforms").as_os_str(),
    )?;
    set("GDAL_DATA", bin.join("gdal-data").as_os_str())?;
    set("PROJ_DATA", bin.join("proj-data").as_os_str())?;
    set("PROJ_LIB", bin.join("proj-data").as_os_str())?;
    let mut paths = vec![bin.clone()];
    paths.extend(env::split_paths(&env::var_os("PATH").unwrap_or_default()));
    set("PATH", &env::join_paths(paths)?)?;
    // Published SDK (or an explicit compatible runtime override), loaded before workers.
    Ok(unsafe { geokernel::Runtime::load(bin)? })
}

fn set(name: &str, value: &std::ffi::OsStr) -> crate::Result<()> {
    env::set_var(name, value);
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "ucrt")]
        extern "C" {
            fn _wputenv_s(name: *const u16, value: *const u16) -> i32;
        }
        let name: Vec<u16> = std::ffi::OsStr::new(name)
            .encode_wide()
            .chain(Some(0))
            .collect();
        let value: Vec<u16> = value.encode_wide().chain(Some(0)).collect();
        if unsafe { _wputenv_s(name.as_ptr(), value.as_ptr()) } != 0 {
            return Err("Cannot configure native runtime environment".into());
        }
    }
    Ok(())
}

fn prepare_published_sdk() -> crate::Result<PathBuf> {
    use std::process::Command;
    let common = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut command = Command::new("powershell.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let status = command
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(common.join("AcquireSdk.ps1"))
        .status()?;
    if !status.success() {
        return Err("Published SDK preparation failed".into());
    }
    Ok(common.join("../packages/GeoKernel/1.5.39/windows-x64"))
}
