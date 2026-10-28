use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub type WorkerResult<T> = std::result::Result<T, String>;
#[derive(Clone, Copy)]
pub struct Asset {
    pub url: &'static str,
    pub sha: &'static str,
    pub required: &'static str,
}
pub const DATA: Asset = Asset {
    url: "https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/spain_sentinel2_l1c.zip",
    sha: "07811e9600882c526185268c5b280134e5715c95042754e414d6736fa5647839", required: "dynamicworld_spain_sentinel2_l1c.tif",
};
pub const MODEL: Asset = Asset {
    url: "https://pub-00d214f25f754884b22349c19ac37bbf.r2.dev/geokernel-models/google-dynamicworld-geokernel-onnx-v1.zip",
    sha: "326bb2899cdea6e620a02f5e16fd330ec848e0a5b76f72f576038d7bc1e0aa63", required: "geokernel-model.json",
};
pub fn check(cancel: &AtomicBool) -> WorkerResult<()> {
    if cancel.load(Ordering::Relaxed) {
        Err("Cancelled.".into())
    } else {
        Ok(())
    }
}
pub fn prepare(
    asset: &Asset,
    cancel: &AtomicBool,
    progress: &Sender<(i32, String)>,
) -> WorkerResult<PathBuf> {
    prepare_inner(asset, cancel, progress).map_err(|e| e.to_string())
}
fn prepare_inner(
    asset: &Asset,
    cancel: &AtomicBool,
    progress: &Sender<(i32, String)>,
) -> crate::Result<PathBuf> {
    check(cancel)?;
    let base = std::env::var_os("GEOKERNEL_EXAMPLES_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
                .join("GeoKernel/Examples/GeoAI")
        });
    let root = base.join(asset.sha);
    fs::create_dir_all(&root)?;
    let root = root.canonicalize()?;
    let raw = asset.url.split('?').next().unwrap_or("").ends_with(".tif");
    let archive = root.join(if raw { asset.required } else { "download.zip" });
    let id = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    if !archive.is_file() || hash(&archive, cancel)? != asset.sha {
        let partial = root.join(format!("{id}.part"));
        let downloaded = (|| -> crate::Result<()> {
            let mut command = Command::new("curl.exe");
            command
                .args([
                    "--fail",
                    "--location",
                    "--max-redirs",
                    "5",
                    "--silent",
                    "--show-error",
                    "--connect-timeout",
                    "15",
                    "--speed-time",
                    "30",
                    "--speed-limit",
                    "1",
                    "--output",
                ])
                .arg(&partial)
                .arg(asset.url);
            child(command, cancel, || {
                let bytes = fs::metadata(&partial).map(|m| m.len()).unwrap_or(0);
                let _ = progress.send((
                    0,
                    format!("Downloading — {:.1} MB", bytes as f64 / 1048576.0),
                ));
            })?;
            check(cancel)?;
            if hash(&partial, cancel)? != asset.sha {
                return Err("Asset SHA-256 mismatch".into());
            }
            if archive.exists() {
                fs::remove_file(&archive)?;
            }
            fs::rename(&partial, &archive)?;
            Ok(())
        })();
        if partial.exists() {
            fs::remove_file(&partial)?;
        }
        downloaded?;
    }
    check(cancel)?;
    if raw {
        return Ok(archive);
    }
    let content = root.join("content");
    if !content.is_dir() {
        let staging = root.join(format!("extract-{id}"));
        fs::create_dir(&staging)?;
        let extracted = (|| -> crate::Result<()> {
            let script = root.join(format!("{id}.ps1"));
            fs::File::create(&script)?.write_all(include_bytes!("assets.ps1"))?;
            let mut command = Command::new("powershell.exe");
            command
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(&script)
                .arg("-Archive")
                .arg(&archive)
                .arg("-Destination")
                .arg(&staging);
            let status = child(command, cancel, || {
                let _ = progress.send((0, "Extracting verified asset…".into()));
            });
            fs::remove_file(script)?;
            status?;
            check(cancel)?;
            match fs::rename(&staging, &content) {
                Ok(()) => {}
                Err(_) if content.is_dir() => {}
                Err(e) => return Err(e.into()),
            }
            Ok(())
        })();
        if staging.exists() {
            // Only remove this worker's newly-created staging directory, after checking its resolved parent.
            let resolved = staging.canonicalize()?;
            if resolved.parent() != Some(root.as_path()) {
                return Err("Unsafe staging cleanup path".into());
            }
            fs::remove_dir_all(resolved)?;
        }
        extracted?;
    }
    let mut matches = Vec::new();
    find(&content, asset.required, &mut matches)?;
    if matches.len() != 1 {
        return Err(format!("Expected one {}, found {}", asset.required, matches.len()).into());
    }
    Ok(matches.remove(0))
}
fn hash(path: &Path, cancel: &AtomicBool) -> crate::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        check(cancel)?;
        let size = file.read(&mut buffer)?;
        if size == 0 {
            break;
        }
        hash.update(&buffer[..size]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn find(root: &Path, name: &str, results: &mut Vec<PathBuf>) -> crate::Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            return Err("Symlinks are not allowed in asset content".into());
        }
        if entry.file_type()?.is_dir() {
            find(&entry.path(), name, results)?;
        } else if entry.file_name() == name {
            results.push(entry.path());
        }
    }
    Ok(())
}
fn child(
    mut command: Command,
    cancel: &AtomicBool,
    mut progress: impl FnMut(),
) -> crate::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut process = command
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    while process.try_wait()?.is_none() {
        if cancel.load(Ordering::Relaxed) {
            let _ = process.kill();
            let _ = process.wait();
            return Err("Cancelled.".into());
        }
        progress();
        thread::sleep(Duration::from_millis(100));
    }
    let output = process.wait_with_output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr)
            .trim()
            .to_owned()
            .into());
    }
    Ok(())
}
