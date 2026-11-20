//! Live classical DSM-to-DTM processing (method native-multiscale-morphological-tin-v1), a port of
//! the Qt DsmToDtmProcessing filter: block lower envelope keeping each minimum's real source pixel,
//! reflected-boundary morphological openings at increasing scales with a slope/relief/roughness
//! allowance, GDAL linear TIN gridding at the native pixel centres, retained source samples, a DSM
//! upper bound and QA bits. Float arithmetic follows the C++ code (f32 grids, f32 differences), so
//! results match the Qt build. The reference is never an input to [`estimate`]. Worker-thread safe.
use crate::gdal::{self, Raster, METHOD, NODATA};
use crate::sample::{check, Result};
use serde_json::{json, Value};
use std::{collections::VecDeque, sync::atomic::AtomicBool, time::Instant};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parameters {
    pub working: f64,
    pub window: f64,
    pub threshold: f64,
    pub slope: f64,
    pub terrain: f64,
    pub retain: f64,
}

impl Parameters {
    pub fn json(&self) -> Value {
        json!({
            "requested_working_m": self.working, "maximum_window_m": self.window,
            "base_threshold_m": self.threshold, "slope_allowance": self.slope,
            "terrain_slope_factor": self.terrain, "retain_tolerance_m": self.retain,
            "roughness_scale_m": 0.6, "maximum_base_threshold_m": 3.0,
        })
    }
}

pub const PRESET_NAMES: [&str; 3] = ["Balanced", "Remove more objects", "Preserve more relief"];
pub const PRESETS: [Parameters; 3] = [
    Parameters {
        working: 2.0,
        window: 64.0,
        threshold: 0.4,
        slope: 0.12,
        terrain: 0.5,
        retain: 0.35,
    },
    Parameters {
        working: 1.0,
        window: 80.0,
        threshold: 0.25,
        slope: 0.08,
        terrain: 0.25,
        retain: 0.2,
    },
    Parameters {
        working: 2.0,
        window: 48.0,
        threshold: 0.6,
        slope: 0.16,
        terrain: 0.8,
        retain: 0.4,
    },
];

/// One completed estimate.
pub struct Terrain {
    pub parameters: Parameters,
    pub working: f64,
    pub seconds: f64,
    pub supports: usize,
    pub fallback: usize,
    pub dtm: Vec<f32>,
    pub removed: Vec<f32>,
    pub quality: Vec<u8>,
    pub summary: Value,
}

fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(message.to_owned())
    }
}

fn reflect(mut index: isize, count: isize) -> usize {
    while index < 0 || index >= count {
        index = if index < 0 {
            -index - 1
        } else {
            2 * count - index - 1
        };
    }
    index as usize
}

/// Separable sliding minimum/maximum over (2r+1)² with reflected boundaries (monotonic deque).
fn extremes(
    input: &[f32],
    width: usize,
    height: usize,
    radius: usize,
    minimum: bool,
    cancel: &AtomicBool,
) -> Result<Vec<f32>> {
    let pass = |source: &[f32], target: &mut [f32], vertical: bool| -> Result<()> {
        let (lines, length) = if vertical {
            (width, height)
        } else {
            (height, width)
        };
        let r = radius as isize;
        let mut queue: VecDeque<(isize, f32)> = VecDeque::new();
        for line in 0..lines {
            check(cancel)?;
            queue.clear();
            for k in -r..length as isize + r {
                let at = reflect(k, length as isize);
                let value = source[if vertical {
                    at * width + line
                } else {
                    line * width + at
                }];
                while queue
                    .back()
                    .is_some_and(|b| if minimum { b.1 >= value } else { b.1 <= value })
                {
                    queue.pop_back();
                }
                queue.push_back((k, value));
                while queue.front().is_some_and(|f| f.0 < k - r * 2) {
                    queue.pop_front();
                }
                let destination = k - r;
                if destination >= 0 {
                    let d = destination as usize;
                    target[if vertical {
                        d * width + line
                    } else {
                        line * width + d
                    }] = queue.front().map_or(value, |f| f.1);
                }
            }
        }
        Ok(())
    };
    let mut horizontal = vec![0f32; input.len()];
    let mut output = vec![0f32; input.len()];
    pass(input, &mut horizontal, false)?;
    pass(&horizontal, &mut output, true)?;
    Ok(output)
}

fn gaussian(
    input: &[f32],
    width: usize,
    height: usize,
    sigma: f64,
    cancel: &AtomicBool,
) -> Result<Vec<f32>> {
    let radius = (3.0 * sigma).ceil() as isize;
    let mut kernel: Vec<f64> = (-radius..=radius)
        .map(|k| (-0.5 * (k * k) as f64 / (sigma * sigma)).exp())
        .collect();
    let sum: f64 = kernel.iter().fold(0.0, |s, v| s + v);
    kernel.iter_mut().for_each(|v| *v /= sum);
    let (w, h) = (width as isize, height as isize);
    let mut temporary = vec![0f32; input.len()];
    let mut output = vec![0f32; input.len()];
    for y in 0..height {
        check(cancel)?;
        for x in 0..width {
            let mut value = 0.0f64;
            for k in -radius..=radius {
                value += f64::from(input[y * width + reflect(x as isize + k, w)])
                    * kernel[(k + radius) as usize];
            }
            temporary[y * width + x] = value as f32;
        }
    }
    for y in 0..height {
        check(cancel)?;
        for x in 0..width {
            let mut value = 0.0f64;
            for k in -radius..=radius {
                value += f64::from(temporary[reflect(y as isize + k, h) * width + x])
                    * kernel[(k + radius) as usize];
            }
            output[y * width + x] = value as f32;
        }
    }
    Ok(output)
}

fn gradient(z: &[f32], width: usize, height: usize, pixel: f64) -> Vec<f32> {
    let mut result = vec![0f32; z.len()];
    for y in 0..height {
        for x in 0..width {
            let (l, r) = (x.saturating_sub(1), (x + 1).min(width - 1));
            let (t, b) = (y.saturating_sub(1), (y + 1).min(height - 1));
            // f32 height differences as in the C++ code, then f64 slopes.
            let dx = f64::from(z[y * width + r] - z[y * width + l]) / ((r - l) as f64 * pixel);
            let dy = f64::from(z[b * width + x] - z[t * width + x]) / ((b - t) as f64 * pixel);
            result[y * width + x] = dx.hypot(dy) as f32;
        }
    }
    result
}

/// Four-neighbour propagation of the nearest support index (fills holes and hull fallbacks).
fn support_map(
    valid: &[u8],
    width: usize,
    height: usize,
    cancel: &AtomicBool,
) -> Result<Vec<usize>> {
    let mut nearest = vec![usize::MAX; valid.len()];
    let mut queue: Vec<usize> = Vec::with_capacity(valid.len());
    for (i, v) in valid.iter().enumerate() {
        if *v != 0 {
            nearest[i] = i;
            queue.push(i);
        }
    }
    require(!queue.is_empty(), "No support cells.")?;
    let mut head = 0;
    while head < queue.len() {
        if head % 4096 == 0 {
            check(cancel)?;
        }
        let i = queue[head];
        head += 1;
        let (x, y) = (i % width, i / width);
        let neighbours = [
            (x > 0).then(|| i - 1),
            (x + 1 < width).then(|| i + 1),
            (y > 0).then(|| i - width),
            (y + 1 < height).then(|| i + width),
        ];
        for n in neighbours.into_iter().flatten() {
            if nearest[n] == usize::MAX {
                nearest[n] = nearest[i];
                queue.push(n);
            }
        }
    }
    Ok(nearest)
}

fn lround(value: f64) -> i64 {
    value.round() as i64 // f64::round rounds half away from zero, like std::lround
}

pub fn estimate(
    dsm: &Raster,
    p: Parameters,
    cancel: &AtomicBool,
    progress: &dyn Fn(f64, &str),
) -> Result<Terrain> {
    let started = Instant::now();
    let factor = lround(p.working / dsm.pixel).max(1) as usize;
    let working = factor as f64 * dsm.pixel;
    require(
        p.window >= 2.0 * working,
        "Maximum window must span at least two working cells.",
    )?;
    let (width, height) = (dsm.width.div_ceil(factor), dsm.height.div_ceil(factor));
    require(
        width >= 3 && height >= 3 && width * height <= 262_144,
        "Working grid must have at least 3 x 3 and at most 262144 cells. Adjust working resolution.",
    )?;
    let count = width * height;
    let (z, sv, sw) = (&dsm.z, &dsm.valid, dsm.width);
    let mut low = vec![f32::INFINITY; count];
    let mut minima = vec![usize::MAX; count];
    let mut valid = vec![0u8; count];
    for y in 0..dsm.height {
        check(cancel)?;
        for x in 0..sw {
            let (source, cell) = (y * sw + x, (y / factor) * width + x / factor);
            if sv[source] != 0 && z[source] < low[cell] {
                low[cell] = z[source];
                minima[cell] = source;
                valid[cell] = 1;
            }
        }
    }
    let fill = support_map(&valid, width, height, cancel)?;
    for i in 0..count {
        if valid[i] == 0 {
            low[i] = low[fill[i]];
        }
    }
    let mut candidates = valid.clone();
    let radius_for = |window: f64| (lround(window / working) / 2).max(1) as usize;
    let opening = |values: &[f32], radius: usize| -> Result<Vec<f32>> {
        extremes(
            &extremes(values, width, height, radius, true, cancel)?,
            width,
            height,
            radius,
            false,
            cancel,
        )
    };

    progress(15.0, "Estimating DSM-derived slope and roughness...");
    let envelope = opening(&low, radius_for(p.window))?;
    let terrain_slope = gaussian(
        &gradient(&envelope, width, height, working),
        width,
        height,
        1.0,
        cancel,
    )?;
    let smoothed = gaussian(&low, width, height, 1.0, cancel)?;
    let local_slope = extremes(
        &gradient(&smoothed, width, height, working),
        width,
        height,
        2,
        false,
        cancel,
    )?;
    let high_pass: Vec<f32> = low
        .iter()
        .zip(&smoothed)
        .map(|(l, s)| (l - s) * (l - s))
        .collect();
    let variance = gaussian(&high_pass, width, height, 2.0, cancel)?;
    let mut windows = Vec::new();
    let mut window = (working * 2.0).max(4.0);
    while window < p.window {
        windows.push(window);
        window *= 2.0;
    }
    windows.push(p.window);
    for (step, window) in windows.iter().copied().enumerate() {
        progress(
            20.0 + (35.0 * step as f64 / windows.len() as f64).trunc(),
            &format!("Ground candidates: {window} m window"),
        );
        let opened = opening(&low, radius_for(window))?;
        let slope = extremes(
            &terrain_slope,
            width,
            height,
            radius_for(window),
            false,
            cancel,
        )?;
        for i in 0..count {
            // f32 square root, then the C runtime pow as in the C++ code.
            let weight = 1.0 / (1.0 + (f64::from(variance[i].max(0.0).sqrt()) / 0.6).powf(4.0));
            let threshold = (p.threshold + p.slope * window).min(3.0)
                + p.terrain * window * f64::from(slope[i].min(local_slope[i])) * weight;
            candidates[i] =
                u8::from(candidates[i] != 0 && f64::from(low[i] - opened[i]) <= threshold);
        }
    }
    let (mut xs, mut ys, mut zs) = (Vec::new(), Vec::new(), Vec::new());
    for i in (0..count).filter(|i| candidates[*i] != 0) {
        let source = minima[i];
        xs.push(((source % sw) as f64 + 0.5) * dsm.pixel);
        ys.push(((source / sw) as f64 + 0.5) * dsm.pixel);
        zs.push(f64::from(z[source]));
    }
    require(
        zs.len() >= 4,
        "Not enough ground supports; relax parameters.",
    )?;
    let nearest = support_map(&candidates, width, height, cancel)?;
    let mut dtm = gdal::triangulate(dsm, &xs, &ys, &zs, cancel, progress)?;
    let n = z.len();
    let (mut removed, mut quality) = (vec![0f32; n], vec![0u8; n]);
    let (mut retained, mut fallback, mut clamped, mut border_count) =
        (0usize, 0usize, 0usize, 0usize);
    let border = (p.window * 0.5 / dsm.pixel).ceil() as usize;
    for row in 0..dsm.height {
        check(cancel)?;
        for col in 0..sw {
            let (i, cell) = (row * sw + col, (row / factor) * width + col / factor);
            if sv[i] == 0 {
                dtm[i] = NODATA;
                removed[i] = NODATA;
                quality[i] = 1;
                continue;
            }
            let mut qa = 0u8;
            if dtm[i] == NODATA || !dtm[i].is_finite() {
                dtm[i] = low[nearest[cell]];
                qa |= 4;
                fallback += 1;
            }
            if candidates[cell] != 0 && f64::from((z[i] - dtm[i]).abs()) <= p.retain {
                dtm[i] = z[i];
                retained += 1;
            } else {
                qa |= 2;
            }
            if dtm[i] > z[i] {
                dtm[i] = z[i];
                qa |= 16;
                clamped += 1;
            }
            if row < border || col < border || row + border >= dsm.height || col + border >= sw {
                qa |= 8;
                border_count += 1;
            }
            removed[i] = (z[i] - dtm[i]).max(0.0);
            quality[i] = qa;
        }
    }
    let seconds = started.elapsed().as_secs_f64();
    let summary = json!({
        "method": METHOD, "effective_working_m": working, "support_points": zs.len(),
        "retained_source_pixels": retained, "outside_hull_fallback_pixels": fallback,
        "upper_bound_clamped_pixels": clamped, "edge_sensitive_pixels": border_count,
        "edge_band_m": p.window * 0.5, "compute_seconds": seconds,
    });
    progress(84.0, "Preparing numeric GeoTIFFs and display maps...");
    Ok(Terrain {
        parameters: p,
        working,
        seconds,
        supports: zs.len(),
        fallback,
        dtm,
        removed,
        quality,
        summary,
    })
}

// ----- displays, scores and report -----------------------------------------------------------------
type Rgb = [u8; 3];

fn hex(color: &str) -> Rgb {
    let v = u32::from_str_radix(color.trim_start_matches('#'), 16).unwrap_or(0);
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

fn blend(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|k| lround(f64::from(a[k]) + (f64::from(b[k]) - f64::from(a[k])) * t) as u8)
}

/// Hillshade (azimuth 315°, altitude 40°) on true pixel spacing; NoData gray.
pub fn relief(raster: &Raster, z: &[f32], cancel: &AtomicBool) -> Result<[Vec<u8>; 3]> {
    let (w, h) = (raster.width, raster.height);
    let mut bands = [vec![0u8; w * h], vec![0u8; w * h], vec![0u8; w * h]];
    let gray = hex("#75818c");
    let (az, alt) = (
        315.0 * std::f64::consts::PI / 180.0,
        40.0 * std::f64::consts::PI / 180.0,
    );
    for y in 0..h {
        check(cancel)?;
        for x in 0..w {
            let i = y * w + x;
            if raster.valid[i] == 0 {
                (0..3).for_each(|k| bands[k][i] = gray[k]);
                continue;
            }
            let at = |xx: usize, yy: usize| {
                let k = yy * w + xx;
                if raster.valid[k] != 0 {
                    z[k]
                } else {
                    z[i]
                }
            };
            let (l, r) = (x.saturating_sub(1), (x + 1).min(w - 1));
            let (t, b) = (y.saturating_sub(1), (y + 1).min(h - 1));
            let dx = f64::from(at(r, y) - at(l, y)) / ((r - l) as f64 * raster.pixel);
            let dy = f64::from(at(x, t) - at(x, b)) / ((b - t) as f64 * raster.pixel);
            let light = (-dx * f64::sin(az) * f64::cos(alt) - dy * f64::cos(az) * f64::cos(alt)
                + f64::sin(alt))
                / (dx * dx + dy * dy + 1.0).sqrt();
            let value = lround(255.0 * (0.25 + 0.75 * light.max(0.0))).clamp(0, 255) as u8;
            (0..3).for_each(|k| bands[k][i] = value);
        }
    }
    Ok(bands)
}

/// Mode 0 removed height (0–20 m), 1 support/edge quality, 2 signed reference error (−2..+2 m).
#[allow(clippy::needless_range_loop)] // one index addresses the source cell and three bands
pub fn diagnostic(
    dsm: &Raster,
    terrain: &Terrain,
    mode: usize,
    reference: Option<&Raster>,
    cancel: &AtomicBool,
) -> Result<[Vec<u8>; 3]> {
    let n = dsm.width * dsm.height;
    let mut bands = [vec![0u8; n], vec![0u8; n], vec![0u8; n]];
    let (gray, white) = (hex("#75818c"), hex("#fafafa"));
    for y in 0..dsm.height {
        check(cancel)?;
        for i in y * dsm.width..(y + 1) * dsm.width {
            let mut color = gray;
            if dsm.valid[i] != 0 {
                if mode == 0 {
                    color = blend(white, hex("#bc3e31"), f64::from(terrain.removed[i]) / 20.0);
                } else if mode == 1 {
                    let qa = terrain.quality[i];
                    color = hex(if qa & 4 != 0 {
                        "#a743a6"
                    } else if qa & 8 != 0 {
                        "#e8ad49"
                    } else if qa & 2 != 0 {
                        "#edf3f5"
                    } else {
                        "#15969b"
                    });
                } else if let Some(reference) = reference.filter(|r| r.valid[i] != 0) {
                    let error = f64::from(terrain.dtm[i]) - f64::from(reference.z[i]);
                    color = if error < 0.0 {
                        blend(white, hex("#2464b5"), -error / 2.0)
                    } else {
                        blend(white, hex("#c93645"), error / 2.0)
                    };
                }
            }
            (0..3).for_each(|k| bands[k][i] = color[k]);
        }
    }
    Ok(bands)
}

#[derive(Clone, Copy, Default)]
pub struct Score {
    pub count: usize,
    pub rmse: f64,
    pub mae: f64,
    pub bias: f64,
}

impl Score {
    fn json(&self) -> Value {
        let has = self.count > 0;
        json!({"pixels": self.count, "rmse_m": has.then_some(self.rmse), "mae_m": has.then_some(self.mae), "bias_m": has.then_some(self.bias)})
    }
}

fn score(dsm: &Raster, result: &[f32], reference: Option<&Raster>, border: usize) -> Score {
    let Some(reference) = reference else {
        return Score::default();
    };
    let mut s = Score::default();
    let (mut squared, mut absolute, mut signed) = (0.0, 0.0, 0.0);
    for y in border..dsm.height.saturating_sub(border) {
        for x in border..dsm.width.saturating_sub(border) {
            let i = y * dsm.width + x;
            if dsm.valid[i] != 0 && reference.valid[i] != 0 {
                let e = f64::from(result[i]) - f64::from(reference.z[i]);
                s.count += 1;
                squared += e * e;
                absolute += e.abs();
                signed += e;
            }
        }
    }
    if s.count > 0 {
        let n = s.count as f64;
        (s.rmse, s.mae, s.bias) = ((squared / n).sqrt(), absolute / n, signed / n);
    }
    s
}

/// Scores over all shared valid cells and outside the edge band (half the maximum window).
pub fn evaluate(dsm: &Raster, terrain: &Terrain, reference: Option<&Raster>) -> (Score, Score) {
    let border = (terrain.parameters.window * 0.5 / dsm.pixel).ceil() as usize;
    (
        score(dsm, &terrain.dtm, reference, 0),
        score(dsm, &terrain.dtm, reference, border),
    )
}

pub fn report(
    dsm: &Raster,
    reference: Option<&Raster>,
    manifest: &Value,
    terrain: &Terrain,
    all: Score,
    interior: Score,
) -> Value {
    json!({
        "schema_version": 1,
        "method": METHOD,
        "parameters": terrain.parameters.json(),
        "summary": terrain.summary,
        "source_dsm": dsm.path.display().to_string(),
        "source_sha256": dsm.sha256,
        "reference": reference.map(|r| r.path.display().to_string()),
        "reference_sha256": reference.map(|r| r.sha256.clone()),
        "reference_used_for_processing": false,
        "grid": {"width": dsm.width, "height": dsm.height, "geotransform": dsm.transform, "crs_wkt": dsm.projection},
        "source_manifest": manifest,
        "all_valid_reference_error": all.json(),
        "interior_reference_error": interior.json(),
        "edge_band_definition": "Half the requested maximum window, rounded up to source cells",
        "quality_bits": {"1": "Source NoData", "2": "Interpolated (not explicitly retained source sample)",
                         "4": "Outside TIN hull: grid-support fallback", "8": "Edge-sensitive band",
                         "16": "Prediction clamped to DSM"},
        "height_units": "metre; unspecified source units assumed metres; no vertical datum conversion",
        "processing_scope": "This input raster only. Supply surrounding DSM context and crop afterward to reduce boundary effects.",
    })
}
