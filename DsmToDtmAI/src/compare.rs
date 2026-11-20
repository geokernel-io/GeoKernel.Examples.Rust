//! The live AI / classical comparison: both estimates from the same full DSM, display maps, the
//! shared comparison crop, full and crop scores, the provenance report and the staged export.
use crate::ai::{self, AiHeight};
use crate::gdal::{self, Pixels, Raster, METHOD, NODATA};
use crate::sample;
use crate::terrain::{self, Parameters, Score, Terrain};
use serde_json::{json, Value};
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

/// A loaded source: DSM, optional evaluation-only reference and its relief display.
pub struct Scene {
    pub dsm: Arc<Raster>,
    pub reference: Option<Arc<Raster>>,
    pub original: PathBuf,
    pub manifest: Value,
}

/// One completed comparison: both estimates, display maps, scores, files and the report.
#[derive(Clone)]
pub struct Comparison {
    pub classical: Arc<Terrain>,
    pub ai: Arc<AiHeight>,
    pub overlap: i32,
    pub threads: i32,
    pub model_manifest: Value,
    /// `[method][mode]`: method 0 classical, 1 AI; mode 0 relief, 1 removed height, 2 error.
    pub maps: [[PathBuf; 3]; 2],
    pub full: [Score; 2],
    pub crop: [Score; 2],
    pub files: BTreeMap<String, PathBuf>,
    pub report: Value,
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

fn cut(values: &[f32], width: usize, [x0, y0, w, h]: [usize; 4]) -> Vec<f32> {
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

/// The fields of a classical result the display helpers read, for the AI estimate.
fn display_terrain(dtm: &[f32], removed: Vec<f32>) -> Terrain {
    Terrain {
        parameters: terrain::PRESETS[0],
        working: 0.0,
        dtm: dtm.to_vec(),
        removed,
        quality: Vec::new(),
        summary: Value::Null,
    }
}

fn rgb(d: &Raster, path: PathBuf, bands: Result<[Vec<u8>; 3]>) -> Result<PathBuf> {
    gdal::write_rgb(&path, d, &bands?)
}

pub fn evaluate(scene: &Scene, mut result: Comparison, archive_sha256: &str) -> Comparison {
    let (d, reference) = (&scene.dsm, scene.reference.as_deref());
    let rect = window(scene);
    let heights = [&result.classical.dtm, &result.ai.dtm];
    result.full = heights.map(|z| terrain::score(d, z, reference, 0));
    result.crop = heights.map(|z| area_score(d, z, reference, rect));
    let (c, a) = (&result.classical, &result.ai);
    result.report = json!({
        "schema_version": 1,
        "method": "pix_grad_live_and_independent_native_classical",
        "source_dsm": d.path.display().to_string(),
        "source_sha256": d.sha256,
        "reference": reference.map(|r| r.path.display().to_string()),
        "reference_sha256": reference.map(|r| r.sha256.clone()),
        "reference_used_for_processing": false,
        "classical_uses_ai": false,
        "ai_uses_classical": false,
        "model_archive_sha256": archive_sha256,
        "model_sha256": result.model_manifest["modelSha256"],
        "model_manifest": result.model_manifest,
        "sdk_inference": a.provenance,
        "ai": {
            "overlap_pixels": result.overlap, "cpu_threads": result.threads, "processed_tiles": a.tiles,
            "context_filled_source_pixels": a.context_filled, "context_fill": "four-neighbour DSM-only support propagation",
            "small_input_edge_padding": a.padded, "upper_bound_clamped_pixels": a.clamped, "raw_output_retained": true,
        },
        "classical": {"method": METHOD, "parameters": c.parameters.json(), "summary": c.summary},
        "grid": {"width": d.width, "height": d.height, "transform": d.transform, "crs_wkt": d.projection},
        "evaluation": {
            "full": {"classical": result.full[0].json(), "ai": result.full[1].json()},
            "comparison_crop": {
                "pixel_window": {"column": rect[0], "row": rect[1], "width": rect[2], "height": rect[3]},
                "classical": result.crop[0].json(), "ai": result.crop[1].json(),
            },
        },
        "source_manifest": scene.manifest,
        "output_files": result.files.keys().map(|k| (k.clone(), json!(k))).collect::<serde_json::Map<_, _>>(),
        "attribution": "Terrain sample: Federal Office of Topography swisstopo. Model weights: David Perez, Zenodo 7973800, CC BY 4.0; deterministic ONNX adaptation.",
        "limits": "Experimental regional model; fixed development scene, not held-out benchmark. Metre heights, no vertical datum conversion. Source filename years differ.",
    });
    result
}

/// Classical first, then an independent AI inference from the same full source DSM.
#[allow(clippy::too_many_arguments)]
pub fn compare(
    scene: &Scene,
    model: &Path,
    parameters: Parameters,
    overlap: i32,
    threads: i32,
    folder: &Path,
    archive_sha256: &str,
    cancel: &AtomicBool,
    progress: Progress,
) -> Result<Comparison> {
    let d = &scene.dsm;
    let classical = terrain::estimate(d, parameters, cancel, &|p, text| {
        progress((p * 0.35).trunc(), &format!("Classical: {text}"))
    })?;
    let ai = ai::infer(d, model, overlap, threads, folder, cancel, progress)?;
    let manifest: Value = serde_json::from_str(
        &fs::read_to_string(model.join("geokernel-model.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let mut files = BTreeMap::new();
    let mut numeric = |name: &str, pixels: Pixels| -> Result<()> {
        sample::check(cancel)?;
        files.insert(
            name.to_owned(),
            gdal::write_raster(d, &folder.join(name), pixels)?,
        );
        Ok(())
    };
    progress(82.0, "Writing native-resolution numeric results...");
    numeric("classical-dtm.tif", Pixels::Heights(&classical.dtm))?;
    numeric("ai-dtm.tif", Pixels::Heights(&ai.dtm))?;
    numeric("ai-raw.tif", Pixels::Heights(&ai.raw))?;
    numeric("classical-removed.tif", Pixels::Heights(&classical.removed))?;
    let removed: Vec<f32> = (0..ai.dtm.len())
        .map(|i| {
            if d.valid[i] != 0 {
                (d.z[i] - ai.dtm[i]).max(0.0)
            } else {
                NODATA
            }
        })
        .collect();
    numeric("ai-removed.tif", Pixels::Heights(&removed))?;
    numeric("classical-qa.tif", Pixels::Quality(&classical.quality))?;
    let terrains = [&classical, &display_terrain(&ai.dtm, removed)];
    let mut maps: [[PathBuf; 3]; 2] = Default::default();
    for (method, terrain) in terrains.into_iter().enumerate() {
        for (mode, slot) in maps[method].iter_mut().enumerate() {
            progress(
                (86 + method * 4 + mode) as f64,
                &format!(
                    "Preparing {} display...",
                    if method == 1 { "AI" } else { "classical" }
                ),
            );
            let bands = if mode == 0 {
                terrain::relief(d, &terrain.dtm, cancel)
            } else {
                terrain::diagnostic(
                    d,
                    terrain,
                    if mode == 1 { 0 } else { 2 },
                    scene.reference.as_deref(),
                    cancel,
                )
            };
            *slot = rgb(d, folder.join(format!("view-{method}-{mode}.tif")), bands)?;
        }
    }
    progress(96.0, "Preparing the shared comparison crop and scores...");
    let rect = window(scene);
    let cropped = cut_grid(d, rect);
    let mut crop = |name: &str, values: &[f32]| -> Result<()> {
        let path = gdal::write_raster(
            &cropped,
            &folder.join(name),
            Pixels::Heights(&cut(values, d.width, rect)),
        )?;
        files.insert(name.to_owned(), path);
        Ok(())
    };
    crop("comparison-dsm.tif", &d.z)?;
    crop("comparison-classical.tif", &classical.dtm)?;
    crop("comparison-ai.tif", &ai.dtm)?;
    if let Some(reference) = &scene.reference {
        crop("comparison-reference.tif", &reference.z)?;
    }
    let result = Comparison {
        classical: Arc::new(classical),
        ai: Arc::new(ai),
        overlap,
        threads,
        model_manifest: manifest,
        maps,
        full: Default::default(),
        crop: Default::default(),
        files,
        report: Value::Null,
    };
    let result = evaluate(scene, result, archive_sha256);
    sample::check(cancel)?;
    Ok(result)
}

/// New error maps, crop file and scores for a new evaluation reference; heights unchanged.
pub fn attach_reference(
    scene: &Scene,
    result: &Comparison,
    folder: &Path,
    archive_sha256: &str,
    cancel: &AtomicBool,
    progress: Progress,
) -> Result<Comparison> {
    let d = &scene.dsm;
    let mut next = result.clone();
    for (method, heights) in [&result.classical.dtm, &result.ai.dtm]
        .into_iter()
        .enumerate()
    {
        progress(
            40.0 + method as f64 * 20.0,
            "Preparing reference error maps...",
        );
        let bands = terrain::diagnostic(
            d,
            &display_terrain(heights, Vec::new()),
            2,
            scene.reference.as_deref(),
            cancel,
        );
        next.maps[method][2] = rgb(d, folder.join(format!("reference-{method}.tif")), bands)?;
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
        "dsm-dtm-ai-{}-{:02}{:02}{:02}-{:06x}",
        civil_date(days as i64),
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60,
        (now.subsec_nanos() ^ std::process::id()) & 0xff_ffff
    ));
    let staging = parent.join(format!(
        ".dsm-dtm-export-{:x}-{}",
        now.as_nanos(),
        std::process::id()
    ));
    fs::create_dir(&staging).map_err(|e| format!("Cannot create export staging directory: {e}"))?;
    let result = (|| -> Result<()> {
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
        report["output_files"] = result
            .files
            .keys()
            .map(|k| (k.clone(), json!(k)))
            .collect::<serde_json::Map<_, _>>()
            .into();
        let text = serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?;
        fs::write(staging.join("comparison.json"), text).map_err(|e| e.to_string())?;
        sample::check(cancel)?;
        if target.exists() {
            return Err("Cannot commit result directory: it already exists.".into());
        }
        fs::rename(&staging, &target).map_err(|e| format!("Cannot commit result directory: {e}"))
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result.map(|()| target)
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
