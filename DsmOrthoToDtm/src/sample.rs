//! Pinned acquisition: archive SHA-256 plus every extracted file (input sample, model packages).
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
    /// pix_grad and rgb_grad_fm package overrides.
    pub model_archives: [Option<PathBuf>; 2],
    pub model_urls: [Option<String>; 2],
    pub data_root: PathBuf,
}

impl Options {
    /// `--data-dir <folder>`, `--sample-archive <zip>` / `--sample-url <https-url>` (support ZIP)
    /// and `--model-archive <zip>` / `--model-url <https-url>` (model package).
    pub fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let (mut archive, mut url, mut data) = (None, None, None);
        let (mut model_archives, mut model_urls): ([Option<PathBuf>; 2], [Option<String>; 2]) =
            Default::default();
        while let Some(arg) = args.next() {
            let mut value = || {
                args.next()
                    .ok_or_else(|| format!("Missing value for {arg}."))
            };
            match arg.as_str() {
                "--sample-archive" => archive = Some(PathBuf::from(value()?)),
                "--sample-url" => url = Some(value()?),
                "--pix-model-archive" => model_archives[0] = Some(PathBuf::from(value()?)),
                "--rgb-model-archive" => model_archives[1] = Some(PathBuf::from(value()?)),
                "--pix-model-url" => model_urls[0] = Some(value()?),
                "--rgb-model-url" => model_urls[1] = Some(value()?),
                "--data-dir" => data = Some(PathBuf::from(value()?)),
                _ => {
                    return Err(format!(
                        "Unknown option {arg}. Use --data-dir, --sample-archive, --sample-url, --pix-model-archive, --rgb-model-archive, --pix-model-url or --rgb-model-url."
                    ))
                }
            }
        }
        let data_root =
            data.unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data"));
        Ok(Self {
            sample_archive: archive,
            sample_url: url,
            model_archives,
            model_urls,
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

    /// The pinned Zurich DSM + orthophoto sample (with its PROJ database), compiled into the example.
    pub fn zurich_ortho() -> Self {
        Self::from_json(include_str!("../sample-assets.json"))
    }

    /// The pinned pix_grad (0) or rgb_grad_fm (1) model package.
    pub fn model(index: usize) -> Self {
        Self::from_json(if index == 0 {
            include_str!("../pix-model-assets.json")
        } else {
            include_str!("../rgb-model-assets.json")
        })
    }
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
        "GeoKernel-DsmOrthoToDtm-Rust/1.0",
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
