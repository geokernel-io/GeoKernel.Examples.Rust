use crate::gdal::{Gdal, Grid, Polygon, R};
use geokernel::Runtime;
use geokernel_geoai_examples::{
    assets::{self, Asset},
    profiles,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub const DATA:Asset=Asset{url:"https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/almere_oosterwold_change_2016_2025_v1.zip",sha:"ecc35ef2c6bbdd586adc6ecc812ce1c74aee62fbf8e404de1d63b75bf7137a88",required:"oosterwold_2016-epsg25831-50cm.tif"};
pub const HASHES: [&str; 2] = [
    "db723783db4c8738a716fe8b898de2e3099bab9394e335bd489db2412ae0c5c3",
    "7b1b12d8b27cd0714f140415004daf9fd0000495eda31366c7cd4aab218caa82",
];
pub const BOUNDS: [f64; 4] = [657533., 5800382., 658557., 5801406.];
pub const DEMO: [f64; 4] = [657885., 5800734., 658205., 5801054.];
pub const ATTRIBUTION:&str="Beeldmateriaal Nederland / PDOK, 2016 & 2025, CC BY 4.0. WMS crop/reprojection rendered at 0.5 m. Model: HOTOSM / Meta DINOv3; retain package licenses.";
#[derive(Clone, Copy)]
pub struct Policy {
    pub area: f64,
    pub coverage: f64,
    pub tolerance: f64,
    pub own: f64,
    pub other: f64,
}
impl Policy {
    pub const CONSERVATIVE: Self = Self {
        area: 50.,
        coverage: 0.10,
        tolerance: 2.,
        own: 0.75,
        other: 0.20,
    };
    pub const BALANCED: Self = Self {
        area: 20.,
        coverage: 0.20,
        tolerance: 1.,
        own: 0.55,
        other: 0.35,
    };
}
#[derive(Clone)]
pub struct Inputs {
    pub paths: [PathBuf; 2],
    pub model: PathBuf,
}
pub fn file_hash(path: &Path) -> R<String> {
    let mut f = fs::File::open(path).map_err(|e| e.to_string())?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = f.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
pub fn hash(value: &Value) -> String {
    format!("{:x}", Sha256::digest(value.to_string().as_bytes()))
}
pub fn prepare(cancel: &AtomicBool, progress: &Sender<(i32, String)>) -> R<Inputs> {
    let before = assets::prepare(&DATA, cancel, progress)?;
    let after = before.with_file_name("oosterwold_2025-epsg25831-50cm.tif");
    for (p, h) in [&before, &after].into_iter().zip(HASHES) {
        assets::check(cancel)?;
        if file_hash(p)? != h {
            return Err("Imagery SHA-256 mismatch".into());
        }
    }
    let manifest = assets::prepare(&profiles::HOTOSM.model, cancel, progress)?;
    Ok(Inputs {
        paths: [before, after],
        model: manifest
            .parent()
            .ok_or("Model directory missing")?
            .to_owned(),
    })
}
struct Prediction {
    native: Value,
    polygons: Vec<Polygon>,
    labels: Vec<i32>,
    scores: Vec<f32>,
    grid: Grid,
}
pub struct Workspace(pub PathBuf);
impl Workspace {
    pub fn new(prefix: &str) -> R<Self> {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}{}-{id}", std::process::id()));
        fs::create_dir(&path).map_err(|e| e.to_string())?;
        Ok(Self(path))
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        if self.0.parent() == Some(std::env::temp_dir().as_path())
            && self
                .0
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("geokernel-change-"))
        {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
pub fn analyze(
    bin: &Path,
    input: &Inputs,
    bounds: [f64; 4],
    policy: Policy,
    threshold: f64,
    provider: &str,
    cancel: &AtomicBool,
    progress: &Sender<(i32, String)>,
) -> R<Value> {
    let started = Instant::now();
    let work = Workspace::new("geokernel-change-")?;
    let runtime = unsafe { Runtime::load(bin) }.map_err(|e| e.to_string())?;
    let tasks = runtime.ai_tasks().map_err(|e| e.to_string())?;
    let mut ai = runtime.ai().map_err(|e| e.to_string())?;
    let g = Gdal::load(bin)?;
    g.initialize();
    let mut dates = vec![];
    for (date, year) in [2016, 2025].into_iter().enumerate() {
        assets::check(cancel)?;
        let crop = work.0.join(format!("{year}.tif"));
        g.crop(&input.paths[date], bounds, &crop)?;
        let task=tasks.start(&json!({"task":"hotosm-buildings","modelPackagePath":input.model,"rasterPath":crop,"threshold":threshold,"provider":provider,"intraOpThreads":2})).map_err(|e|e.to_string())?;
        loop {
            if cancel.load(Ordering::Relaxed) {
                let _ = task.cancel();
                return Err("Cancelled.".into());
            }
            let s = task.status().map_err(|e| e.to_string())?;
            let _ = progress.send((
                date as i32 * 45 + (s["percent"].as_f64().unwrap_or(0.) * 0.3) as i32,
                format!("{year}: {}", s["message"].as_str().unwrap_or("Inference")),
            ));
            match s["state"].as_str() {
                Some("failed") => {
                    return Err(s["error"].as_str().unwrap_or("Inference failed").into())
                }
                Some("cancelled") => return Err("Cancelled.".into()),
                Some("completed") => break,
                _ => thread::sleep(Duration::from_millis(80)),
            }
        }
        assets::check(cancel)?;
        let native = task.result().map_err(|e| e.to_string())?;
        let grid = {
            let ds = g.open(&crop)?;
            g.metadata(ds.handle)?
        };
        let (polygons, labels) = g.polygons_and_labels(
            Path::new(
                native["vectorPath"]
                    .as_str()
                    .ok_or("Missing prediction path")?,
            ),
            &grid,
        )?;
        let raw = work.0.join(format!("{year}-scores.tif"));
        let _ = progress.send((date as i32 * 45 + 31, format!("{year}: matching scores…")));
        // Raw FloatBands has no cancellation callback. Wait for the pass, then check
        // cancellation before publishing or releasing any worker-owned resources.
        ai.run_raster_inference(&json!({"modelPackagePath":input.model,"rasterPath":crop,"outputMode":"floatBands","outputPath":raw,"bands":[1,2,3],"provider":provider,"intraOpThreads":2,"interOpThreads":1}).to_string()).map_err(|e|e.to_string())?;
        assets::check(cancel)?;
        let mut scores = {
            let ds = g.open(&raw)?;
            if !g.metadata(ds.handle)?.same(&grid) {
                return Err("Score and label grids differ".into());
            }
            g.read_float(ds.handle, 1)?
        };
        for score in &mut scores {
            if !score.is_finite() {
                return Err("Nonfinite model logits".into());
            }
            *score = (1. / (1. + (-(*score as f64).clamp(-80., 80.)).exp())) as f32;
        }
        task.close().map_err(|e| e.to_string())?;
        dates.push(Prediction {
            native,
            polygons,
            labels,
            scores,
            grid,
        });
    }
    if !dates[0].grid.same(&dates[1].grid) {
        return Err("Date grids differ".into());
    }
    let mut features = vec![];
    let (mut edge_count, mut matched) = (0, 0);
    for date in [1, 0] {
        assets::check(cancel)?;
        let own = &dates[date];
        let other = &dates[1 - date];
        let (w, h) = (own.grid.width, own.grid.height);
        let n = own.labels.iter().copied().max().unwrap_or(0) as usize + 1;
        let mut counts = vec![0usize; n];
        let mut covered = vec![0usize; n];
        let mut own_sum = vec![0.; n];
        let mut other_sum = vec![0.; n];
        let mut edge = HashSet::new();
        let coverage = dilate(
            &other.scores,
            w,
            h,
            (policy.tolerance / 0.5).ceil() as usize,
        );
        for y in 0..h {
            assets::check(cancel)?;
            for x in 0..w {
                let p = y * w + x;
                let id = own.labels[p] as usize;
                counts[id] += 1;
                covered[id] += coverage[p] as usize;
                own_sum[id] += own.scores[p] as f64;
                other_sum[id] += other.scores[p] as f64;
                if id > 0 && (x == 0 || y == 0 || x == w - 1 || y == h - 1) {
                    edge.insert(id);
                }
            }
        }
        edge_count += edge.len();
        let mut seen = HashSet::new();
        for polygon in &own.polygons {
            let id = polygon.id;
            if edge.contains(&id)
                || counts[id] as f64 * 0.25 < policy.area
                || polygon.area < policy.area
            {
                continue;
            }
            let mean = own_sum[id] / counts[id] as f64;
            let opposite = other_sum[id] / counts[id] as f64;
            let overlap = covered[id] as f64 / counts[id] as f64;
            if mean < policy.own || opposite > policy.other || overlap >= policy.coverage {
                if seen.insert(id) {
                    matched += 1
                }
                continue;
            }
            let (geometry, _) = g.geometry(&polygon.geometry, 25831, 4326)?;
            let kind = if date == 1 { "new" } else { "removed" };
            let identifier = format!("{kind}-{:04}", features.len() + 1);
            features.push(json!({"type":"Feature","id":identifier,"geometry":geometry,"properties":{"candidate_id":identifier,"change_type":kind,"review_status":"pending","review_note":"","reviewed_by":"","area_m2":polygon.area,"source_instance_id":id,"building_score":mean,"other_date_score":opposite,"other_date_coverage":overlap,"before_year":2016,"after_year":2025}}));
        }
    }
    let elapsed = started.elapsed().as_secs_f64();
    let _ = progress.send((100, "Complete".into()));
    Ok(
        json!({"type":"FeatureCollection","features":features,"geokernel_provenance":{"dataset_url":DATA.url,"dataset_archive_sha256":DATA.sha,"before_sha256":HASHES[0],"after_sha256":HASHES[1],"before_year":2016,"after_year":2025,"source_crs":"EPSG:25831","model_id":"hotosm-dinov3s-buildings","model_archive_sha256":profiles::HOTOSM.model.sha,"model_sha256":file_hash(&input.model.join("model.onnx"))?,"threshold":threshold,"filters":{"area":policy.area,"coverage":policy.coverage,"tolerance":policy.tolerance,"own":policy.own,"other":policy.other},"weak_building_threshold":0.25,"provider":dates[0].native["provider"],"analysis_geotransform":dates[0].grid.gt,"analysis_width":dates[0].grid.width,"analysis_height":dates[0].grid.height,"elapsed_seconds":elapsed,"attribution":ATTRIBUTION,"imagery_license":"https://creativecommons.org/licenses/by/4.0/","method":"Native HOTOSM instances plus second FloatBands score pass; sigmoid after logit blending. Same filter policy as Qt; not bit-identical to per-tile decoded scoring. Predictions require review."},"stats":{"before":dates[0].native["instances"],"after":dates[1].native["instances"],"edge":edge_count,"matched":matched,"elapsed":elapsed}}),
    )
}
pub fn dilate(scores: &[f32], w: usize, h: usize, r: usize) -> Vec<bool> {
    let mut a = vec![false; scores.len()];
    let mut b = a.clone();
    for y in 0..h {
        let mut n = 0i32;
        for x in 0..w.min(r + 1) {
            n += (scores[y * w + x] >= 0.25) as i32;
        }
        for x in 0..w {
            a[y * w + x] = n > 0;
            if x >= r {
                n -= (scores[y * w + x - r] >= 0.25) as i32;
            }
            if x + r + 1 < w {
                n += (scores[y * w + x + r + 1] >= 0.25) as i32;
            }
        }
    }
    for x in 0..w {
        let mut n = 0i32;
        for y in 0..h.min(r + 1) {
            n += a[y * w + x] as i32;
        }
        for y in 0..h {
            b[y * w + x] = n > 0;
            if y >= r {
                n -= a[(y - r) * w + x] as i32;
            }
            if y + r + 1 < h {
                n += a[(y + r + 1) * w + x] as i32;
            }
        }
    }
    b
}
pub fn update(feature: &mut Value, geometry: Value, g: &Gdal) -> R<bool> {
    if hash(&feature["geometry"]) == hash(&geometry) {
        return Ok(false);
    }
    let (_, area) = g.geometry(&geometry, 4326, 25831)?;
    feature["geometry"] = geometry;
    feature["properties"]["area_m2"] = json!(area);
    feature["properties"]["review_status"] = json!("pending");
    feature["properties"]["reviewed_by"] = json!("");
    feature["properties"]["review_note"] = json!("Geometry edited; review again.");
    feature
        .as_object_mut()
        .ok_or("Invalid feature")?
        .remove("approved_geometry_sha256");
    Ok(true)
}
pub fn review(feature: &mut Value, status: &str, actor: &str, note: &str, g: &Gdal) -> R<()> {
    if actor.trim().is_empty() || !["approved", "rejected", "pending"].contains(&status) {
        return Err("Enter a reviewer and a valid decision".into());
    }
    if status == "approved" {
        let (_, area) = g.geometry(&feature["geometry"], 4326, 25831)?;
        feature["properties"]["area_m2"] = json!(area);
        feature["approved_geometry_sha256"] = json!(hash(&feature["geometry"]));
    } else {
        feature
            .as_object_mut()
            .ok_or("Invalid feature")?
            .remove("approved_geometry_sha256");
    }
    feature["properties"]["review_status"] = json!(status);
    feature["properties"]["reviewed_by"] = json!(actor.trim());
    feature["properties"]["review_note"] = json!(note.trim());
    Ok(())
}
pub fn export(result: &Value, target: &Path, g: &Gdal) -> R<()> {
    let mut features = vec![];
    for feature in result["features"].as_array().ok_or("Missing features")? {
        if feature["properties"]["review_status"] != "approved" {
            continue;
        }
        if feature["approved_geometry_sha256"].as_str() != Some(hash(&feature["geometry"]).as_str())
        {
            return Err("Geometry changed; approve again".into());
        }
        g.geometry(&feature["geometry"], 4326, 25831)?;
        let mut copy = feature.clone();
        copy.as_object_mut()
            .unwrap()
            .remove("approved_geometry_sha256");
        features.push(copy);
    }
    if features.is_empty() {
        return Err("Approve at least one candidate".into());
    }
    let output = json!({"type":"FeatureCollection","features":features,"geokernel_provenance":result["geokernel_provenance"]});
    let temporary = target.with_file_name(format!(
        "{}.part",
        target
            .file_name()
            .ok_or("Invalid target")?
            .to_string_lossy()
    ));
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(&output).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if target.exists() {
        // Windows rename cannot replace an existing file; preserve the old export on failure.
        let backup = target.with_file_name(format!(
            "{}.backup-{}",
            target.file_name().unwrap().to_string_lossy(),
            std::process::id()
        ));
        if backup.exists() {
            return Err("Export backup already exists".into());
        }
        fs::rename(target, &backup).map_err(|e| e.to_string())?;
        if let Err(e) = fs::rename(&temporary, target) {
            let _ = fs::rename(&backup, target);
            return Err(e.to_string());
        }
        let _ = fs::remove_file(backup);
    } else {
        fs::rename(temporary, target).map_err(|e| e.to_string())?;
    }
    Ok(())
}
