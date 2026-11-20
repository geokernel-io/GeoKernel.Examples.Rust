//! Pinned sample acquisition: archive SHA-256 plus every extracted file.
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

pub struct Options {
    pub sample_archive: Option<PathBuf>,
    pub sample_url: Option<String>,
    pub data_root: PathBuf,
}

impl Options {
    /// `--sample-archive <zip>`, `--sample-url <https-url>`, `--data-dir <folder>`.
    pub fn parse() -> Result<Self> {
        let mut args = std::env::args().skip(1);
        let (mut archive, mut url, mut data) = (None, None, None);
        while let Some(arg) = args.next() {
            let mut value = || {
                args.next()
                    .ok_or_else(|| format!("Missing value for {arg}."))
            };
            match arg.as_str() {
                "--sample-archive" => archive = Some(PathBuf::from(value()?)),
                "--sample-url" => url = Some(value()?),
                "--data-dir" => data = Some(PathBuf::from(value()?)),
                _ => {
                    return Err(format!(
                        "Unknown option {arg}. Use --sample-archive, --sample-url or --data-dir."
                    ))
                }
            }
        }
        let data_root =
            data.unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data"));
        Ok(Self {
            sample_archive: archive,
            sample_url: url,
            data_root,
        })
    }
}

pub struct Assets {
    pub url: String,
    pub archive: String,
    pub cache: String,
    pub sha256: String,
    pub files: Vec<(String, String)>,
}

impl Assets {
    pub fn zurich_v2() -> Self {
        let json: Value = serde_json::from_str(include_str!("../sample-assets.json"))
            .expect("valid sample-assets.json");
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

/// Return a verified sample folder; download/extract only when the cache is invalid.
pub fn prepare(
    assets: &Assets,
    options: &Options,
    cancel: &AtomicBool,
    progress: &dyn Fn(String),
) -> Result<PathBuf> {
    let root = &options.data_root;
    fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let folder = root.join(&assets.cache);
    progress("Checking cached sample files...".into());
    if valid(&folder, assets) {
        return Ok(folder);
    }
    let archive = options
        .sample_archive
        .clone()
        .unwrap_or_else(|| root.join(&assets.archive));
    if options.sample_archive.is_none() && hash_file(&archive) != assets.sha256 {
        let url = options
            .sample_url
            .clone()
            .unwrap_or_else(|| assets.url.clone());
        if !url.starts_with("https://") {
            return Err("Sample URL must use HTTPS.".into());
        }
        progress("Downloading sample data...".into());
        let partial = archive.with_extension("zip.part");
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
            "600",
            "--user-agent",
            "GeoKernel-DsmToDtmAccuracy-Rust/1.0",
            "--output",
        ])
        .arg(&partial)
        .arg(&url);
        let result = curl
            .spawn()
            .map_err(|e| e.to_string())
            .and_then(|child| wait(child, cancel, "Sample download"));
        if let Err(error) = result {
            let _ = fs::remove_file(&partial);
            return Err(if cancel.load(Ordering::Relaxed) {
                error
            } else {
                format!("{error}\n{url}\nThe sample ZIP must be uploaded to the SampleData v1 release. Use Retry after upload.")
            });
        }
        fs::rename(&partial, &archive).map_err(|e| e.to_string())?;
    }
    check(cancel)?;
    if hash_file(&archive) != assets.sha256 {
        return Err("Sample ZIP SHA-256 mismatch. The existing data was retained.".into());
    }
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let staging = root.join(format!(".extract-{}-{id}", std::process::id()));
    progress("Extracting verified sample ZIP...".into());
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
            "Sample ZIP extraction",
        )?;
        if !valid(&staging, assets) {
            return Err("Extracted sample files failed SHA-256 verification.".into());
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
