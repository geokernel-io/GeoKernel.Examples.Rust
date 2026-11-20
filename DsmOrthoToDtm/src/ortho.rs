//! DSM + orthophoto to DTM: the aligned RGB input, three independent estimates from the same full
//! DSM (classical, DSM-only pix_grad, DSM+RGB rgb_grad_fm), display maps, the shared comparison crop,
//! full and crop scores, the provenance report and the staged export.
use crate::ai::{self, AiHeight, METHODS};
use crate::gdal::{self, Pixels, Raster, METHOD, NODATA};
use crate::sample;
use crate::terrain::{self, Parameters, Score, Terrain};
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, Arc},
    time::{SystemTime, UNIX_EPOCH},
};

type Result<T> = sample::Result<T>;
pub type Progress<'a> = &'a (dyn Fn(f64, &str) + Sync);
const MAX_SUPPORT_CELLS: usize = 262_144;

/// An aligned RGB orthophoto: source bytes and the same bands with RGB-only context fill.
pub struct Ortho {
    pub path: PathBuf,
    pub sha256: String,
    pub bands: [Vec<u8>; 3],
    pub context: [Vec<u8>; 3],
    pub filled: usize,
}

/// Three RGB-ordered Byte bands on the DSM's exact grid that cover every valid DSM cell.
pub fn read_ortho(dsm: &Raster, path: &Path, cancel: &AtomicBool) -> Result<Ortho> {
    let (bands, valid) = gdal::read_rgb(dsm, path, cancel)?;
    if dsm
        .valid
        .iter()
        .zip(&valid)
        .any(|(d, v)| *d != 0 && *v == 0)
    {
        return Err("RGB does not cover every valid DSM pixel. Crop or prepare common source coverage explicitly.".into());
    }
    let filled = valid.iter().filter(|v| **v == 0).count();
    let context = if filled > 0 {
        let nearest = terrain::support_map(&valid, dsm.width, dsm.height, cancel)?;
        bands.clone().map(|band| {
            (0..band.len())
                .map(|i| {
                    if valid[i] != 0 {
                        band[i]
                    } else {
                        band[nearest[i]]
                    }
                })
                .collect()
        })
    } else {
        bands.clone()
    };
    sample::check(cancel)?;
    Ok(Ortho {
        path: std::path::absolute(path).map_err(|e| e.to_string())?,
        sha256: sample::hash_file(path),
        bands,
        context,
        filled,
    })
}

/// A loaded source: DSM, orthophoto, optional evaluation-only reference and the DSM relief.
pub struct Scene {
    pub dsm: Arc<Raster>,
    pub rgb: Arc<Ortho>,
    pub reference: Option<Arc<Raster>>,
    pub original: PathBuf,
    pub manifest: Value,
}

/// One completed comparison of the classical, pix_grad and rgb_grad_fm estimates.
#[derive(Clone)]
pub struct Comparison {
    pub classical: Arc<Terrain>,
    pub ai: [Arc<AiHeight>; 2],
    pub overlap: i32,
    pub threads: i32,
    pub model_manifests: [Value; 2],
    /// `[method][mode]`: mode 0 relief, 1 removed height, 2 error.
    pub maps: [[PathBuf; 3]; 3],
    pub full: [Score; 3],
    pub crop: [Score; 3],
    pub files: BTreeMap<String, PathBuf>,
    pub report: Value,
}

impl Comparison {
    pub fn heights(&self, method: usize) -> &[f32] {
        if method == 0 {
            &self.classical.dtm
        } else {
            &self.ai[method - 1].dtm
        }
    }
}

/// Keep the selected settings but coarsen the working grid until it fits the support-cell cap.
pub fn effective_parameters(dsm: &Raster, mut p: Parameters) -> Parameters {
    let cells = |size: usize, working: f64| {
        (size as f64 / (working / dsm.pixel).round().max(1.0)).ceil() as usize
    };
    while cells(dsm.width, p.working) * cells(dsm.height, p.working) > MAX_SUPPORT_CELLS {
        p.working += dsm.pixel;
    }
    p
}

/// `[column, row, width, height]`: the shared Zurich crop, or the central 60% of a custom DSM.
pub fn window(scene: &Scene) -> [usize; 4] {
    let d = &scene.dsm;
    if scene.manifest["standard_full_tile"] == json!(true) {
        return [200, 800, 500, 500];
    }
    let (left, top) = (d.width / 5, d.height / 5);
    [left, top, d.width - 2 * left, d.height - 2 * top]
}

pub fn crop_extent(scene: &Scene) -> [f64; 4] {
    let [x, y, w, h] = window(scene).map(|v| v as f64);
    let (t, p) = (&scene.dsm.transform, scene.dsm.pixel);
    let (left, top) = (t[0] + x * p, t[3] - y * p);
    [left, top - h * p, left + w * p, top]
}

fn area_score(
    dsm: &Raster,
    z: &[f32],
    reference: Option<&Raster>,
    [x0, y0, w, h]: [usize; 4],
) -> Score {
    let Some(reference) = reference else {
        return Score::default();
    };
    let mut s = Score::default();
    let (mut squared, mut absolute, mut signed) = (0.0, 0.0, 0.0);
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let i = y * dsm.width + x;
            if dsm.valid[i] != 0 && reference.valid[i] != 0 {
                let e = f64::from(z[i]) - f64::from(reference.z[i]);
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

fn cut<T: Copy>(values: &[T], width: usize, [x0, y0, w, h]: [usize; 4]) -> Vec<T> {
    let mut out = Vec::with_capacity(w * h);
    for y in y0..y0 + h {
        out.extend_from_slice(&values[y * width + x0..y * width + x0 + w]);
    }
    out
}

fn cut_grid(d: &Raster, [x0, y0, w, h]: [usize; 4]) -> Raster {
    let mut t = d.transform;
    t[0] += x0 as f64 * d.pixel;
    t[3] -= y0 as f64 * d.pixel;
    Raster {
        path: d.path.clone(),
        projection: d.projection.clone(),
        sha256: d.sha256.clone(),
        width: w,
        height: h,
        pixel: d.pixel,
        transform: t,
        z: Vec::new(),
        valid: Vec::new(),
    }
}

/// The fields of a result the display helpers read: heights and DSM − heights.
fn display_terrain(d: &Raster, dtm: &[f32], with_removed: bool) -> Terrain {
    let removed = if with_removed {
        (0..dtm.len())
            .map(|i| {
                if d.valid[i] != 0 {
                    (d.z[i] - dtm[i]).max(0.0)
                } else {
                    NODATA
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    Terrain {
        parameters: terrain::PRESETS[0],
        working: 0.0,
        dtm: dtm.to_vec(),
        removed,
        quality: Vec::new(),
        summary: Value::Null,
    }
}

fn rgb_file(d: &Raster, path: PathBuf, bands: Result<[Vec<u8>; 3]>) -> Result<PathBuf> {
    gdal::write_rgb(&path, d, &bands?)
}

fn file_map(files: &BTreeMap<String, PathBuf>) -> Value {
    files
        .keys()
        .map(|k| (k.clone(), json!(k)))
        .collect::<Map<_, _>>()
        .into()
}

pub fn evaluate(scene: &Scene, mut result: Comparison, archive_sha256: &[String; 2]) -> Comparison {
    let (d, reference) = (&scene.dsm, scene.reference.as_deref());
    let rect = window(scene);
    let (mut full, mut crop, mut models) = (Map::new(), Map::new(), Map::new());
    for method in 0..3 {
        result.full[method] = terrain::score(d, result.heights(method), reference, 0);
        result.crop[method] = area_score(d, result.heights(method), reference, rect);
        full.insert(METHODS[method].into(), result.full[method].json());
        crop.insert(METHODS[method].into(), result.crop[method].json());
    }
    crop.insert(
        "pixel_window".into(),
        json!({"column": rect[0], "row": rect[1], "width": rect[2], "height": rect[3]}),
    );
    for model in 0..2 {
        let a = &result.ai[model];
        models.insert(
            METHODS[model + 1].into(),
            json!({"archive_sha256": archive_sha256[model], "manifest": result.model_manifests[model], "sdk_inference": a.provenance,
                   "context_filled_dsm_pixels": a.context_filled, "small_input_edge_padding": a.padded,
                   "upper_bound_clamped_pixels": a.clamped}),
        );
    }
    let c = &result.classical;
    result.report = json!({
        "schema_version": 1,
        "experiment": "independent classical / DSM-only pix_grad / DSM+RGB rgb_grad_fm",
        "source_dsm": d.path.display().to_string(),
        "source_dsm_sha256": d.sha256,
        "source_rgb": scene.rgb.path.display().to_string(),
        "source_rgb_sha256": scene.rgb.sha256,
        "rgb_context_filled_pixels": scene.rgb.filled,
        "reference": reference.map(|r| r.path.display().to_string()),
        "reference_sha256": reference.map(|r| r.sha256.clone()),
        "reference_used_for_processing": false,
        "methods_feed_each_other": false,
        "model_input_order": "pix_grad: DSM; rgb_grad_fm: DSM, R, G, B",
        "ai_options": {"overlap_pixels": result.overlap, "cpu_threads": result.threads, "tile_pixels": 256},
        "models": models,
        "classical": {"method": METHOD, "parameters": c.parameters.json(), "summary": c.summary},
        "grid": {"width": d.width, "height": d.height, "transform": d.transform, "crs_wkt": d.projection},
        "evaluation": {"full": full, "comparison_crop": crop},
        "source_manifest": scene.manifest,
        "output_files": file_map(&result.files),
        "attribution": "Federal Office of Topography swisstopo. AI weights: David Perez, Zenodo 7973800 / 7973886, CC BY 4.0; independent deterministic ONNX adaptations.",
        "interpretation": "Different pretrained checkpoints/training objectives; observed differences do not isolate the causal contribution of RGB. Fixed development scene, not a held-out benchmark. Survey years and training GSD differ or remain unverified.",
    });
    result
}

/// Classical, then pix_grad, then rgb_grad_fm, each from the same full source grid.
#[allow(clippy::too_many_arguments)]
pub fn compare(
    scene: &Scene,
    models: &[PathBuf; 2],
    parameters: Parameters,
    overlap: i32,
    threads: i32,
    folder: &Path,
    archive_sha256: &[String; 2],
    cancel: &AtomicBool,
    progress: Progress,
) -> Result<Comparison> {
    let d = &scene.dsm;
    let manifests = [
        ai::read_json(&models[0].join("geokernel-model.json"))?,
        ai::read_json(&models[1].join("geokernel-model.json"))?,
    ];
    let classical = terrain::estimate(d, parameters, cancel, &|p, text| {
        progress((p * 0.24).trunc(), &format!("Classical: {text}"))
    })?;
    let mut heights = Vec::new();
    for model in 0..2 {
        let report = |p: f64, text: &str| {
            progress(
                25.0 + model as f64 * 29.0 + (p * 0.28).trunc(),
                &format!("{}: {text}", METHODS[model + 1]),
            )
        };
        let rgb = (model == 1).then_some(&scene.rgb.context);
        heights.push(Arc::new(ai::infer(
            d,
            &models[model],
            rgb,
            overlap,
            threads,
            folder,
            cancel,
            &report,
        )?));
    }
    let mut result = Comparison {
        classical: Arc::new(classical),
        ai: [heights[0].clone(), heights[1].clone()],
        overlap,
        threads,
        model_manifests: manifests,
        maps: Default::default(),
        full: Default::default(),
        crop: Default::default(),
        files: BTreeMap::new(),
        report: Value::Null,
    };
    let mut files = BTreeMap::new();
    let mut numeric = |name: String, pixels: Pixels| -> Result<()> {
        sample::check(cancel)?;
        let path = gdal::write_raster(d, &folder.join(&name), pixels)?;
        files.insert(name, path);
        Ok(())
    };
    for method in 0..3 {
        progress(
            (84 + method * 3) as f64,
            &format!("Preparing {} outputs...", METHODS[method]),
        );
        let display = display_terrain(d, result.heights(method), true);
        numeric(
            format!("{}-dtm.tif", METHODS[method]),
            Pixels::Heights(&display.dtm),
        )?;
        numeric(
            format!("{}-removed.tif", METHODS[method]),
            Pixels::Heights(&display.removed),
        )?;
        if method > 0 {
            numeric(
                format!("{}-raw.tif", METHODS[method]),
                Pixels::Heights(&result.ai[method - 1].raw),
            )?;
        }
        for (mode, slot) in result.maps[method].iter_mut().enumerate() {
            let bands = if mode == 0 {
                terrain::relief(d, &display.dtm, cancel)
            } else {
                terrain::diagnostic(
                    d,
                    &display,
                    if mode == 1 { 0 } else { 2 },
                    scene.reference.as_deref(),
                    cancel,
                )
            };
            *slot = rgb_file(d, folder.join(format!("view-{method}-{mode}.tif")), bands)?;
        }
    }
    numeric(
        "classical-qa.tif".into(),
        Pixels::Quality(&result.classical.quality),
    )?;
    progress(96.0, "Writing identical source-pixel comparison crops...");
    let rect = window(scene);
    let cropped = cut_grid(d, rect);
    let mut crop = |name: String, values: &[f32]| -> Result<()> {
        let path = gdal::write_raster(
            &cropped,
            &folder.join(&name),
            Pixels::Heights(&cut(values, d.width, rect)),
        )?;
        files.insert(name, path);
        Ok(())
    };
    crop("comparison-dsm.tif".into(), &d.z)?;
    for method in 0..3 {
        crop(
            format!("comparison-{}.tif", METHODS[method]),
            result.heights(method),
        )?;
    }
    if let Some(reference) = &scene.reference {
        crop("comparison-reference.tif".into(), &reference.z)?;
    }
    let rgb_crop = scene
        .rgb
        .bands
        .clone()
        .map(|band| cut(&band, d.width, rect));
    files.insert(
        "comparison-rgb.tif".into(),
        gdal::write_rgb(&folder.join("comparison-rgb.tif"), &cropped, &rgb_crop)?,
    );
    result.files = files;
    let result = evaluate(scene, result, archive_sha256);
    sample::check(cancel)?;
    Ok(result)
}

/// New error maps, crop file and scores for a new evaluation reference; heights unchanged.
pub fn attach_reference(
    scene: &Scene,
    result: &Comparison,
    folder: &Path,
    archive_sha256: &[String; 2],
    cancel: &AtomicBool,
    progress: Progress,
) -> Result<Comparison> {
    let d = &scene.dsm;
    let mut next = result.clone();
    for method in 0..3 {
        progress(
            30.0 + method as f64 * 20.0,
            "Preparing reference error maps...",
        );
        let terrain = display_terrain(d, result.heights(method), false);
        let bands = terrain::diagnostic(d, &terrain, 2, scene.reference.as_deref(), cancel);
        next.maps[method][2] = rgb_file(d, folder.join(format!("reference-{method}.tif")), bands)?;
    }
    let reference = scene.reference.as_ref().ok_or("No reference attached.")?;
    let rect = window(scene);
    let path = gdal::write_raster(
        &cut_grid(d, rect),
        &folder.join("comparison-reference.tif"),
        Pixels::Heights(&cut(&reference.z, d.width, rect)),
    )?;
    next.files.insert("comparison-reference.tif".into(), path);
    Ok(evaluate(scene, next, archive_sha256))
}

/// Stage every file plus comparison.json beside the target, then rename into a new unique folder.
pub fn export(
    result: &Comparison,
    parent: &Path,
    cancel: &AtomicBool,
    progress: Progress,
) -> Result<PathBuf> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?;
    let (days, seconds) = (now.as_secs() / 86_400, now.as_secs() % 86_400);
    let target = parent.join(format!(
        "dsm-ortho-dtm-{}-{:02}{:02}{:02}-{:06x}",
        civil_date(days as i64),
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        (now.subsec_nanos() ^ std::process::id()) & 0xff_ffff
    ));
    let staging = parent.join(format!(
        ".dsm-ortho-export-{:x}-{}",
        now.as_nanos(),
        std::process::id()
    ));
    fs::create_dir(&staging).map_err(|e| format!("Cannot create export staging directory: {e}"))?;
    let outcome = (|| -> Result<()> {
        for (index, (name, source)) in result.files.iter().enumerate() {
            sample::check(cancel)?;
            progress(
                ((index + 1) * 90 / result.files.len()) as f64,
                &format!("Saving {name}"),
            );
            let bytes = fs::read(source).map_err(|e| e.to_string())?;
            let mut file = fs::File::create(staging.join(name)).map_err(|e| e.to_string())?;
            file.write_all(&bytes)
                .and_then(|()| file.sync_all())
                .map_err(|e| e.to_string())?;
        }
        let mut report = result.report.clone();
        report["output_files"] = file_map(&result.files);
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        fs::write(staging.join("comparison.json"), text).map_err(|e| e.to_string())?;
        sample::check(cancel)?;
        if target.exists() {
            return Err("Cannot commit export directory: it already exists.".into());
        }
        fs::rename(&staging, &target).map_err(|e| format!("Cannot commit export directory: {e}"))
    })();
    if outcome.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    outcome.map(|()| target)
}

/// `yyyymmdd` of a day count since 1970-01-01 (UTC), Howard Hinnant's civil-from-days.
fn civil_date(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}{month:02}{day:02}")
}
