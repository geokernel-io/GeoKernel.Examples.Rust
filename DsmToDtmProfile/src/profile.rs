//! Interactive-profile helpers: the shared display relief, bilinear transect samples with
//! line-only RMSE/MAE, the fixed example transect and the atomic CSV/metadata export.
use crate::gdal::Grids;
use crate::sample::{check, Result};
use serde_json::json;
use std::{fs, io::Write, path::Path, sync::atomic::AtomicBool};

pub const SURFACE_NAMES: [&str; 4] = ["Original DSM", "Classical DTM", "AI DTM", "Reference DTM"];

/// Hillshade with one fixed light (azimuth 315°, altitude 40°) and contrast for every map.
/// No per-map normalization: identical heights give identical gray values in every relief.
pub fn relief(grids: &Grids, band: usize, cancel: &AtomicBool) -> Result<Vec<u8>> {
    let (w, h, z) = (grids.width, grids.height, &grids.z[band]);
    let (azimuth, altitude) = (315f64.to_radians(), 40f64.to_radians());
    let (lx, ly, lz) = (
        azimuth.sin() * altitude.cos(),
        azimuth.cos() * altitude.cos(),
        altitude.sin(),
    );
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        check(cancel)?;
        let (top, bottom) = (y.saturating_sub(1), (y + 1).min(h - 1));
        for x in 0..w {
            let (left, right) = (x.saturating_sub(1), (x + 1).min(w - 1));
            let at = |x: usize, y: usize| f64::from(z[y * w + x]);
            let dx = (at(right, y) - at(left, y)) / ((right - left) as f64 * 0.5);
            let dy = (at(x, top) - at(x, bottom)) / ((bottom - top) as f64 * 0.5);
            let light = (-dx * lx - dy * ly + lz) / (dx * dx + dy * dy + 1.0).sqrt();
            out[y * w + x] = (255.0 * (0.25 + 0.75 * light.max(0.0)) + 0.5)
                .floor()
                .clamp(0.0, 255.0) as u8;
        }
    }
    Ok(out)
}

/// Bilinear height between pixel centres; None outside them (no extrapolation).
pub fn height(grids: &Grids, band: usize, x: f64, y: f64) -> Option<f64> {
    let t = &grids.transform;
    let (col, row) = ((x - t[0]) / t[1] - 0.5, (y - t[3]) / t[5] - 0.5);
    let (w, h) = (grids.width as f64, grids.height as f64);
    if !col.is_finite()
        || !row.is_finite()
        || col < -1e-7
        || row < -1e-7
        || col > w - 1.0 + 1e-7
        || row > h - 1.0 + 1e-7
    {
        return None;
    }
    let (cx, cy) = (col.clamp(0.0, w - 1.0), row.clamp(0.0, h - 1.0));
    let ix = (cx as usize).min(grids.width - 2);
    let iy = (cy as usize).min(grids.height - 2);
    let (u, v) = (cx - ix as f64, cy - iy as f64);
    let z = &grids.z[band];
    let i = iy * grids.width + ix;
    let at = |k: usize| f64::from(z[k]);
    Some(
        (1.0 - v) * ((1.0 - u) * at(i) + u * at(i + 1))
            + v * ((1.0 - u) * at(i + grids.width) + u * at(i + grids.width + 1)),
    )
}

pub struct Sample {
    pub x: f64,
    pub y: f64,
    pub distance: f64,
    pub heights: [f64; 4],
}

pub struct Profile {
    pub start: [f64; 2],
    pub end: [f64; 2],
    pub length: f64,
    pub spacing: f64,
    pub samples: Vec<Sample>,
    pub rmse: [f64; 2],
    pub mae: [f64; 2],
}

/// ceil(length / spacing) + 1 equal-spaced samples (at most 4096).
pub fn make(grids: &Grids, a: [f64; 2], b: [f64; 2], requested: f64) -> Result<Profile> {
    if height(grids, 0, a[0], a[1]).is_none() || height(grids, 0, b[0], b[1]).is_none() {
        return Err("Choose both endpoints inside the raster pixel centres.".into());
    }
    if !(requested.is_finite() && (0.5..=10.0).contains(&requested)) {
        return Err("Profile spacing must be between 0.5 and 10 metres.".into());
    }
    let length = (b[0] - a[0]).hypot(b[1] - a[1]);
    if length < 0.5 {
        return Err("Choose endpoints at least 0.5 m apart.".into());
    }
    let intervals = ((length / requested).ceil() as usize).clamp(1, 4095);
    let (mut squared, mut absolute) = ([0.0; 2], [0.0; 2]);
    let mut samples = Vec::with_capacity(intervals + 1);
    for i in 0..=intervals {
        let t = i as f64 / intervals as f64;
        let (x, y) = (a[0] * (1.0 - t) + b[0] * t, a[1] * (1.0 - t) + b[1] * t);
        let mut heights = [0.0; 4];
        for (band, value) in heights.iter_mut().enumerate() {
            *value =
                height(grids, band, x, y).ok_or("The transect crosses unavailable raster data.")?;
        }
        for m in 0..2 {
            let e = heights[m + 1] - heights[3];
            squared[m] += e * e;
            absolute[m] += e.abs();
        }
        samples.push(Sample {
            x,
            y,
            distance: length * t,
            heights,
        });
    }
    let n = samples.len() as f64;
    Ok(Profile {
        start: a,
        end: b,
        length,
        spacing: length / intervals as f64,
        samples,
        rmse: squared.map(|s| (s / n).sqrt()),
        mae: absolute.map(|s| s / n),
    })
}

/// The fixed building transect of the first example (row `profile.row`, west half).
pub fn example(grids: &Grids) -> ([f64; 2], [f64; 2]) {
    let t = &grids.transform;
    let y = t[3] + (grids.profile_row as f64 + 0.5) * t[5];
    (
        [t[0] + 0.25, y],
        [t[0] + (grids.width as f64 - 0.5) * 0.5, y],
    )
}

fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Write the numeric CSV, then `<name>.csv.metadata.json`; each file atomically.
pub fn export(path: &Path, profile: &Profile, grids: &Grids, archive_sha256: &str) -> Result<()> {
    let mut csv = String::from("distance_m,easting_epsg2056_m,northing_epsg2056_m,dsm_ln02_m,classical_ln02_m,ai_ln02_m,reference_ln02_m,classical_error_m,ai_error_m\n");
    for s in &profile.samples {
        let h = &s.heights;
        let values = [
            s.distance,
            s.x,
            s.y,
            h[0],
            h[1],
            h[2],
            h[3],
            h[1] - h[3],
            h[2] - h[3],
        ];
        csv += &values.map(|v| format!("{v:.6}")).join(",");
        csv.push('\n');
    }
    atomic_write(path, csv.as_bytes()).map_err(|e| format!("Cannot save CSV output: {e}"))?;
    let metadata = json!({
        "sample_archive_sha256": archive_sha256,
        "source": grids.manifest,
        "sampling": "Bilinear numeric heights, projected metric distances",
        "line_length_m": profile.length,
        "sample_spacing_m": profile.spacing,
        "sample_count": profile.samples.len(),
        "source_attribution": "Federal Office of Topography swisstopo; AI: David Perez, pix_grad adaptation",
    });
    let mut sidecar = path.as_os_str().to_owned();
    sidecar.push(".metadata.json");
    let text = serde_json::to_string_pretty(&metadata).map_err(|e| e.to_string())?;
    atomic_write(Path::new(&sidecar), text.as_bytes())
        .map_err(|e| format!("CSV saved; cannot save metadata sidecar: {e}"))?;
    Ok(())
}
