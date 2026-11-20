//! Pinned acquisition: archive SHA-256 plus every extracted file (support sample, model package)
//! and single files checked by size and SHA-256 (the full 1 km terrain sources).
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub type Result<T> = std::result::Result<T, String>;

pub const SAMPLE_HINT: &str =
    "The sample ZIP must be uploaded to the SampleData v1 release. Use Retry after upload.";

pub struct Options {
    pub sample_archive: Option<PathBuf>,
    pub sample_url: Option<String>,
    pub model_archive: Option<PathBuf>,
    pub model_url: Option<String>,
    pub data_root: PathBuf,
}

impl Options {
    /// `--data-dir <folder>`, `--sample-archive <zip>` / `--sample-url <https-url>` (support ZIP)
    /// and `--model-archive <zip>` / `--model-url <https-url>` (model package).
    pub fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let (mut archive, mut url, mut model_archive, mut model_url, mut data) =
            (None, None, None, None, None);
        while let Some(arg) = args.next() {
            let mut value = || {
                args.next()
                    .ok_or_else(|| format!("Missing value for {arg}."))
            };
            match arg.as_str() {
                "--sample-archive" => archive = Some(PathBuf::from(value()?)),
                "--sample-url" => url = Some(value()?),
                "--model-archive" => model_archive = Some(PathBuf::from(value()?)),
                "--model-url" => model_url = Some(value()?),
                "--data-dir" => data = Some(PathBuf::from(value()?)),
                _ => {
                    return Err(format!(
                        "Unknown option {arg}. Use --data-dir, --sample-archive, --sample-url, --model-archive or --model-url."
                    ))
                }
            }
        }
        let data_root =
            data.unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data"));
        Ok(Self {
            sample_archive: archive,
            sample_url: url,
            model_archive,
            model_url,
            data_root,
        })
    }
}

/// A pinned archive: URL, archive name, cache folder, archive and per-file SHA-256.
pub struct Assets {
    pub url: String,
    pub archive: String,
    pub cache: String,
    pub sha256: String,
    pub files: Vec<(String, String)>,
}

impl Assets {
    fn from_json(text: &str) -> Self {
        let json: Value = serde_json::from_str(text).expect("valid pinned asset manifest");
        let text = |key: &str| json[key].as_str().unwrap_or_default().to_owned();
        let files = json["files"]
            .as_object()
            .map(|files| {
                files
                    .iter()
                    .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            url: text("url"),
            archive: text("archive"),
            cache: text("cache"),
            sha256: text("sha256"),
            files,
        }
    }

    /// The Zurich v2 support sample (matching PROJ database), compiled into the example.
    pub fn zurich_v2() -> Self {
        Self::from_json(include_str!("../sample-assets.json"))
    }

    /// The pinned pix_grad model package.
    pub fn model() -> Self {
        Self::from_json(include_str!("../model-assets.json"))
    }
}

/// The pinned full 1 km terrain sources.
pub fn terrain_assets() -> Value {
    serde_json::from_str(include_str!("../terrain-assets.json")).expect("valid terrain-assets.json")
}

pub fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err("Operation cancelled.".into())
    } else {
        Ok(())
    }
}

pub fn hash_file(path: &Path) -> String {
    let Ok(mut file) = fs::File::open(path) else {
        return String::new();
    };
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buffer[..n]),
            Err(_) => return String::new(),
        }
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn valid(folder: &Path, assets: &Assets) -> bool {
    !assets.files.is_empty()
        && assets
            .files
            .iter()
            .all(|(name, sha)| hash_file(&folder.join(name)) == *sha)
}

/// Wait for a helper process, killing it on cancellation.
fn wait(mut child: Child, cancel: &AtomicBool, what: &str) -> Result<()> {
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("{what} failed ({status})."))
            };
        }
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Operation cancelled.".into());
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn hidden(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

/// HTTPS download (curl, cancellable) through a `.part` sibling that replaces `destination`.
pub fn download(
    url: &str,
    destination: &Path,
    cancel: &AtomicBool,
    what: &str,
    hint: &str,
) -> Result<()> {
    if !url.starts_with("https://") {
        return Err("Download URL must use HTTPS.".into());
    }
    let mut partial = destination.as_os_str().to_owned();
    partial.push(".part");
    let partial = PathBuf::from(partial);
    let mut curl = hidden("curl.exe");
    curl.args([
        "--fail",
        "--location",
        "--max-redirs",
        "5",
        "--silent",
        "--show-error",
        "--connect-timeout",
        "15",
        "--max-time",
        "1800",
        "--user-agent",
        "GeoKernel-DsmToDtmAI-Rust/1.0",
        "--output",
    ])
    .arg(&partial)
    .arg(url);
    let result = curl
        .spawn()
        .map_err(|e| e.to_string())
        .and_then(|child| wait(child, cancel, what));
    if let Err(error) = result {
        let _ = fs::remove_file(&partial);
        return Err(if cancel.load(Ordering::Relaxed) {
            error
        } else {
            format!("{error}\n{url}\n{hint}")
        });
    }
    fs::rename(&partial, destination).map_err(|e| e.to_string())
}

/// Return a verified archive folder; download/extract only when the cache is invalid. A local
/// archive is used instead of the URL when supplied; the hash pins apply to both.
pub fn prepare(
    assets: &Assets,
    root: &Path,
    local_archive: Option<&Path>,
    url: Option<&str>,
    cancel: &AtomicBool,
    progress: &dyn Fn(String),
    hint: &str,
) -> Result<PathBuf> {
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let folder = root.join(&assets.cache);
    progress("Checking cached files...".into());
    if valid(&folder, assets) {
        return Ok(folder);
    }
    let archive = local_archive
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join(&assets.archive));
    if local_archive.is_none() && hash_file(&archive) != assets.sha256 {
        progress("Downloading verified archive...".into());
        download(
            url.unwrap_or(&assets.url),
            &archive,
            cancel,
            "Download",
            hint,
        )?;
    }
    check(cancel)?;
    if hash_file(&archive) != assets.sha256 {
        return Err("Archive SHA-256 mismatch. The existing data was retained.".into());
    }
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let staging = root.join(format!(".extract-{}-{id}", std::process::id()));
    progress("Extracting verified archive...".into());
    let result = (|| -> Result<()> {
        let mut extract = hidden("powershell.exe");
        extract
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Extract.ps1"))
            .arg("-Archive")
            .arg(&archive)
            .arg("-Destination")
            .arg(&staging);
        wait(
            extract.spawn().map_err(|e| e.to_string())?,
            cancel,
            "ZIP extraction",
        )?;
        if !valid(&staging, assets) {
            return Err("Extracted files failed SHA-256 verification.".into());
        }
        if folder.exists() {
            fs::rename(&folder, root.join(format!("{}.invalid-{id}", assets.cache)))
                .map_err(|e| e.to_string())?;
        }
        fs::rename(&staging, &folder).map_err(|e| e.to_string())
    })();
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    result.map(|()| folder)
}

/// Download (when needed) and verify the full 1 km DSM and reference by size and SHA-256.
pub fn prepare_terrain(
    assets: &Value,
    root: &Path,
    cancel: &AtomicBool,
    progress: &dyn Fn(String),
) -> Result<PathBuf> {
    let folder = root.join(assets["cache"].as_str().unwrap_or("zurich-full-1km-v1"));
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    for name in ["dsm", "reference"] {
        let asset = &assets["sources"][name];
        let path = folder.join(asset["file"].as_str().unwrap_or_default());
        let sha = asset["sha256"].as_str().unwrap_or_default();
        if hash_file(&path) != sha {
            progress(format!("Full 1 km {name}: downloading..."));
            download(
                asset["url"].as_str().unwrap_or_default(),
                &path,
                cancel,
                "Terrain download",
                "Retry when the swisstopo download is reachable.",
            )?;
        }
        check(cancel)?;
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or_default();
        if Some(size) != asset["bytes"].as_u64() || hash_file(&path) != sha {
            return Err("Full terrain source failed size/SHA-256 verification.".into());
        }
    }
    Ok(folder)
}
