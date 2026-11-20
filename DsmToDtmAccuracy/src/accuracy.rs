//! Reference-accuracy helpers: the shared relief and error colours, evaluation windows of
//! native source cells, selected-area metrics and the atomic JSON report.
use crate::gdal::{self, Grids};
use crate::sample::{check, Result};
use serde_json::json;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

pub const DISPLAY_LIMITS: [f64; 4] = [1.0, 2.0, 5.0, 10.0];
pub const BIN_EDGES: [f64; 5] = [0.25, 0.5, 1.0, 2.0, 5.0];
pub const PRESETS: [&str; 5] = [
    "Full sample",
    "Central urban block",
    "Northern block",
    "Southern block",
    "Custom selection",
];

/// Hillshade with one fixed light (azimuth 315°, altitude 40°) and contrast for every map.
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

const fn rgb(v: u32) -> [f64; 3] {
    [
        (v >> 16) as f64,
        ((v >> 8) & 0xff) as f64,
        (v & 0xff) as f64,
    ]
}
const NEUTRAL: [f64; 3] = rgb(0xfafafa);
const BELOW: [f64; 3] = rgb(0x2464b5);
const ABOVE: [f64; 3] = rgb(0xc93645);
const ORANGE: [f64; 3] = rgb(0xf4a340);
const RED: [f64; 3] = rgb(0xb22f35);

/// Round half away from zero, as Qt's lround.
fn mix(a: [f64; 3], b: [f64; 3], t: f64) -> [u8; 3] {
    [0, 1, 2].map(|i| (a[i] + (b[i] - a[i]) * t).round() as u8)
}

/// Shared display scale: signed blue-white-red or absolute white-orange-red, saturating at the limit.
pub fn error_color(error: f64, absolute: bool, limit: f64) -> [u8; 3] {
    let t = (error.abs() / limit).clamp(0.0, 1.0);
    if !absolute {
        mix(NEUTRAL, if error < 0.0 { BELOW } else { ABOVE }, t)
    } else if t < 0.5 {
        mix(NEUTRAL, ORANGE, t * 2.0)
    } else {
        mix(ORANGE, RED, (t - 0.5) * 2.0)
    }
}

fn error_bands(
    grids: &Grids,
    method: usize,
    absolute: bool,
    limit: f64,
    cancel: &AtomicBool,
) -> Result<[Vec<u8>; 3]> {
    let (estimate, reference) = (&grids.z[method], &grids.z[3]);
    let n = reference.len();
    let mut bands = [vec![0u8; n], vec![0u8; n], vec![0u8; n]];
    for y in 0..grids.height {
        check(cancel)?;
        for i in y * grids.width..(y + 1) * grids.width {
            let c = error_color(
                f64::from(estimate[i]) - f64::from(reference[i]),
                absolute,
                limit,
            );
            for (band, value) in bands.iter_mut().zip(c) {
                band[i] = value;
            }
        }
    }
    Ok(bands)
}

/// Native source cells of an evaluation area.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Window {
    pub left: usize,
    pub top: usize,
    pub width: usize,
    pub height: usize,
}

impl Window {
    pub fn contains(&self, x: usize, y: usize) -> bool {
        x >= self.left && x < self.left + self.width && y >= self.top && y < self.top + self.height
    }
}

/// Native cells whose centres lie in the area `[x_min, y_min, x_max, y_max]`, clipped to the grid.
pub fn pixel_window(grids: &Grids, area: [f64; 4]) -> Result<Window> {
    let [x_min, y_min, x_max, y_max] = area;
    if !area.iter().all(|v| v.is_finite()) || x_max <= x_min || y_max <= y_min {
        return Err("Choose a nonempty finite study area.".into());
    }
    let t = &grids.transform;
    let (dx, dy, x0, y0) = (t[1], -t[5], t[0], t[3]);
    let clip = |v: f64, max: usize| v.clamp(0.0, max as f64) as usize;
    let left = clip(((x_min - x0) / dx - 0.5).ceil(), grids.width);
    let right = clip(((x_max - x0) / dx - 0.5).floor() + 1.0, grids.width);
    let top = clip(((y0 - y_max) / dy - 0.5).ceil(), grids.height);
    let bottom = clip(((y0 - y_min) / dy - 0.5).floor() + 1.0, grids.height);
    if right <= left || bottom <= top {
        return Err("The selected area contains no source pixel centres.".into());
    }
    Ok(Window {
        left,
        top,
        width: right - left,
        height: bottom - top,
    })
}

/// World bounds of the union of the window's cells.
pub fn window_extent(grids: &Grids, w: Window) -> [f64; 4] {
    let t = &grids.transform;
    [
        t[0] + w.left as f64 * t[1],
        t[3] + (w.top + w.height) as f64 * t[5],
        t[0] + (w.left + w.width) as f64 * t[1],
        t[3] + w.top as f64 * t[5],
    ]
}

/// Geographic windows (not land-cover classes): full, central, northern, southern.
pub fn preset_area(grids: &Grids, index: usize) -> [f64; 4] {
    let (x, y, size) = (
        grids.transform[0],
        grids.transform[3],
        grids.width as f64 * 0.5,
    );
    match index {
        0 => [x, y - size, x + size, y],
        1 => [
            x + size * 0.2,
            y - size * 0.8,
            x + size * 0.8,
            y - size * 0.2,
        ],
        2 => [x, y - size * 0.35, x + size, y],
        _ => [x, y - size, x + size, y - size * 0.65],
    }
}

pub struct Metric {
    pub count: usize,
    pub rmse: f64,
    pub mae: f64,
    pub bias: f64,
    pub p95: f64,
    pub above: usize,
    pub bins: [usize; 6],
}

impl Metric {
    pub fn above_percent(&self) -> f64 {
        if self.count > 0 {
            100.0 * self.above as f64 / self.count as f64
        } else {
            0.0
        }
    }
    fn json(&self) -> serde_json::Value {
        json!({"pixels": self.count, "rmse_m": self.rmse, "mae_m": self.mae, "bias_m": self.bias,
               "p95_absolute_m": self.p95, "above_threshold_pixels": self.above,
               "above_threshold_percent": self.above_percent(), "absolute_error_histogram": self.bins})
    }
}

/// RMSE, MAE, bias, nearest-rank P95 of |error|, count above threshold and six-bin histogram.
pub fn measure(grids: &Grids, w: Window, threshold: f64) -> Result<[Metric; 2]> {
    if !(threshold.is_finite() && threshold > 0.0) {
        return Err("Error threshold must be positive.".into());
    }
    if w.width == 0
        || w.height == 0
        || w.left + w.width > grids.width
        || w.top + w.height > grids.height
    {
        return Err("Invalid source pixel window.".into());
    }
    let reference = &grids.z[3];
    let one = |method: usize| {
        let estimate = &grids.z[method];
        let (mut squared, mut absolute, mut signed, mut above) = (0.0, 0.0, 0.0, 0usize);
        let mut bins = [0usize; 6];
        let mut errors = Vec::with_capacity(w.width * w.height);
        for y in w.top..w.top + w.height {
            for i in y * grids.width + w.left..y * grids.width + w.left + w.width {
                let error = f64::from(estimate[i]) - f64::from(reference[i]);
                let magnitude = error.abs();
                squared += error * error;
                absolute += magnitude;
                signed += error;
                if magnitude > threshold {
                    above += 1;
                }
                bins[BIN_EDGES
                    .iter()
                    .take_while(|edge| magnitude > **edge)
                    .count()] += 1;
                errors.push(magnitude);
            }
        }
        errors.sort_by(f64::total_cmp);
        let n = errors.len();
        Metric {
            count: n,
            rmse: (squared / n as f64).sqrt(),
            mae: absolute / n as f64,
            bias: signed / n as f64,
            p95: errors[(0.95 * n as f64).ceil() as usize - 1],
            above,
            bins,
        }
    };
    Ok([one(1), one(2)])
}

/// Selected-area metrics and provenance as JSON, written atomically.
pub fn export_report(
    path: &Path,
    grids: &Grids,
    w: Window,
    area_name: &str,
    threshold: f64,
    metrics: &[Metric; 2],
    archive_sha256: &str,
) -> Result<()> {
    let report = json!({
        "schema_version": 1,
        "sample_archive_sha256": archive_sha256,
        "source_manifest": grids.manifest,
        "area_name": area_name,
        "source_pixel_window": {"column": w.left, "row": w.top, "width": w.width, "height": w.height},
        "evaluated_cell_bounds_epsg2056": window_extent(grids, w),
        "pixel_footprint_m2": metrics[0].count as f64 * 0.25,
        "large_error_threshold_m": threshold,
        "p95_definition": "Nearest-rank empirical 95th percentile of absolute reference errors",
        "histogram_bin_edges_m": BIN_EDGES,
        "classical": metrics[0].json(),
        "ai_pix_grad": metrics[1].json(),
        "reference_used_for_inference": false,
        "metric_scope": "Shared native pixel centres in this selected area; fixed development scene; not held-out generalization accuracy",
        "attribution": "Federal Office of Topography swisstopo; AI: David Perez, pix_grad adaptation",
    });
    let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temporary = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if temporary.exists() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("Cannot save evaluation report: {e}"))
}

pub struct Displays {
    pub original: PathBuf,
    pub methods: [Vec<PathBuf>; 2],
}

/// Nineteen shared-scale Byte RGB maps: DSM relief; per method 4 signed, 4 absolute, 1 relief.
pub fn prepare_displays(
    grids: &Grids,
    folder: &Path,
    cancel: &AtomicBool,
    progress: &dyn Fn(f64),
) -> Result<Displays> {
    let mut done = 0;
    let mut save = |bands: [&[u8]; 3], name: &str| -> Result<PathBuf> {
        check(cancel)?;
        let path = gdal::write_rgb(
            &folder.join(name),
            grids.width,
            grids.height,
            &grids.transform,
            &grids.projection,
            bands,
        )?;
        done += 1;
        progress(f64::from(done) * 100.0 / 19.0);
        Ok(path)
    };
    let gray = relief(grids, 0, cancel)?;
    let original = save([&gray, &gray, &gray], "original.tif")?;
    let mut methods: [Vec<PathBuf>; 2] = Default::default();
    for (index, files) in methods.iter_mut().enumerate() {
        let method = index + 1;
        let mut list = vec![PathBuf::new(); 9];
        for (k, limit) in DISPLAY_LIMITS.iter().enumerate() {
            let [r, g, b] = error_bands(grids, method, false, *limit, cancel)?;
            list[k] = save([&r, &g, &b], &format!("signed-{method}-{k}.tif"))?;
            let [r, g, b] = error_bands(grids, method, true, *limit, cancel)?;
            list[4 + k] = save([&r, &g, &b], &format!("absolute-{method}-{k}.tif"))?;
        }
        let gray = relief(grids, method, cancel)?;
        list[8] = save([&gray, &gray, &gray], &format!("terrain-{method}.tif"))?;
        *files = list;
    }
    Ok(Displays { original, methods })
}
