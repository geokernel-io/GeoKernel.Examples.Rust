//! Spatial embedding search: places that look alike, but only in one CRS, one area and one time span.
//! The AlphaEarth embeddings of the Netherlands (2018 / 2021 / 2024, 1 km cells in UTM zones 31N and
//! 32N, GeoKernel.SampleData) become 227 424 records of a geokernel `EmbeddingIndex`, each with its
//! extent, CRS and year. A query takes a seed cell's embedding with a CRS, an extent and years; the
//! index returns the most similar records that pass all three filters and counts what each filter
//! rejected. Rust port of the Qt example.
//!
//! Needs geokernel with `Runtime::embedding_index` (newer than 1.5.40).
//! `spatial-embedding-search [--smoke]` (--smoke: three searches that each show one filter, print, quit)
use geokernel::{EmbeddingIndex, Extent, Runtime, ViewerWindow};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver, Sender},
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const TITLE: &str = "Spatial embedding search";
const DATA_URL: &str = "https://github.com/geokernel-io/GeoKernel.SampleData/releases/download/v1/netherlands_alphaearth_2018_2024_1km_v1.zip";
const DATA_SHA: &str = "6d28e052793bd19c44e6c6e0ce22b52d12e392cbf83b5df931aa90652579762b";
const YEARS: [i64; 3] = [2018, 2021, 2024];
const ATTRIBUTION: &str = "The AlphaEarth Foundations Satellite Embedding dataset is produced by Google and Google DeepMind (CC BY 4.0) · 2018, 2021 and 2024 layers, Source Cooperative mirror, resampled to 1 km by GeoKernel";
const PLACE: i32 = 1;
const SEED_YEAR: i32 = 2;
const EXTENT: i32 = 3;
const FROM: i32 = 4;
const TO: i32 = 5;
const SEARCH: i32 = 6;
const MATCHES: i32 = 7;
const PROGRESS: i32 = 8;
const RESULT_LAYERS: [(&str, &str); 3] = [
    ("query-extent", r##"{"lineColor":"#FFD400","lineWidth":2.0,"fillOpacity":0}"##),
    ("matches", r##"{"pointColor":"#E31A1C","fillColor":"#E31A1C","lineColor":"#7F0000","pointSize":9.0,"lineWidth":1.0}"##),
    ("seed", r##"{"pointColor":"#1F78B4","fillColor":"#1F78B4","lineColor":"#FFFFFF","pointSize":12.0,"lineWidth":1.5}"##),
];

/// Downloads, SHA-256 checks and extracts the data into the cache shared with the other GeoAI
/// examples; returns the directory. Runs on a worker thread.
fn prepare(progress: &Sender<String>) -> Result<PathBuf> {
    let base = std::env::var_os("GEOKERNEL_EXAMPLES_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("GeoKernel/Examples/GeoAI")
    });
    let root = base.join(DATA_SHA);
    fs::create_dir_all(&root)?;
    let archive = root.join("download.zip");
    let hash = |file: &Path| -> Result<String> {
        let mut digest = Sha256::new();
        let mut reader = fs::File::open(file)?;
        let mut buffer = vec![0u8; 1 << 20];
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        Ok(digest.finalize().iter().map(|b| format!("{b:02x}")).collect())
    };
    if !archive.is_file() || hash(&archive)? != DATA_SHA {
        let partial = root.join(format!("{}.part", std::process::id()));
        let _ = progress.send("Downloading netherlands_alphaearth_2018_2024_1km_v1.zip (14 MB)…".into());
        let status = Command::new("curl.exe").args(["--fail", "--location", "--silent", "--show-error", "--output"]).arg(&partial).arg(DATA_URL).status()?;
        if !status.success() || hash(&partial)? != DATA_SHA {
            let _ = fs::remove_file(&partial);
            return Err("Download failed or SHA-256 mismatch. Please retry.".into());
        }
        fs::rename(&partial, &archive)?;
    }
    let content = root.join("content");
    if !content.is_dir() {
        let staging = root.join(format!("extract-{}", std::process::id()));
        fs::create_dir_all(&staging)?;
        let _ = progress.send("Extracting the verified data…".into());
        if !Command::new("tar.exe").arg("-xf").arg(&archive).arg("-C").arg(&staging).status()?.success() {
            return Err("Could not extract the data.".into());
        }
        fs::rename(&staging, &content)?;
    }
    if !content.join("embeddings.json").is_file() {
        return Err("embeddings.json is missing from the download.".into());
    }
    Ok(content)
}

/// embeddings.json (layout, places, extents), embeddings.bin (int8 vectors) and centers.bin (WGS84 cell centres).
struct Data {
    directory: PathBuf,
    meta: Value,
    raw: Vec<u8>,
    centers: Vec<u8>,
    scale: f64,
}

impl Data {
    fn load(directory: PathBuf) -> Result<Self> {
        let meta: Value = serde_json::from_str(&fs::read_to_string(directory.join("embeddings.json"))?)?;
        let raw = fs::read(directory.join("embeddings.bin"))?;
        let centers = fs::read(directory.join("centers.bin"))?;
        let scale = meta["scale"].as_f64().unwrap_or(127.0);
        Ok(Self { directory, meta, raw, centers, scale })
    }
    fn grids(&self) -> impl Iterator<Item = &Value> {
        self.meta["grids"].as_array().into_iter().flatten()
    }
    fn grid(&self, year: i64, crs: &str) -> Option<&Value> {
        self.grids().find(|g| g["year"].as_i64() == Some(year) && g["crs"] == crs)
    }
    fn embedding(&self, grid: &Value, column: i64, row: i64) -> Option<Vec<f64>> {
        let width = grid["width"].as_i64()?;
        let start = (grid["offset"].as_i64()? + (row * width + column) * 64) as usize;
        let cell = &self.raw[start..start + 64];
        if cell.iter().all(|&b| b as i8 == -128) {
            return None;
        }
        Some(cell.iter().map(|&b| f64::from(b as i8) / self.scale).collect())
    }
    fn cell_extent(grid: &Value, column: i64, row: i64) -> [f64; 4] {
        let size = grid["cellSize"].as_f64().unwrap_or(1000.0);
        let x0 = grid["originX"].as_f64().unwrap_or(0.0) + column as f64 * size;
        let y1 = grid["originY"].as_f64().unwrap_or(0.0) - row as f64 * size;
        [x0, y1 - size, x0 + size, y1]
    }
    fn center(&self, crs: &str, column: i64, row: i64) -> Option<(f32, f32)> {
        let entry = self.meta["centers"].as_array()?.iter().find(|c| c["crs"] == crs)?;
        let offset = (entry["offset"].as_i64()? + (row * entry["width"].as_i64()? + column) * 8) as usize;
        let float = |at: usize| f32::from_le_bytes(self.centers[at..at + 4].try_into().unwrap_or([0; 4]));
        Some((float(offset), float(offset + 4)))
    }
}

fn years_range(first: i64, last: i64) -> Value {
    json!({"startUtc": format!("{first}-01-01T00:00:00Z"), "endUtc": format!("{}-01-01T00:00:00Z", last + 1)})
}

/// One record per cell with data: its 1 km extent in the zone's CRS and the year as time range.
/// Built on the GUI thread a few rows at a time, so the window keeps pumping events.
fn build_index(runtime: &Runtime, window: &mut ViewerWindow, data: &Data) -> Result<EmbeddingIndex> {
    let index = runtime.embedding_index(64, Some(&json!({"method": "hnsw", "efConstruction": 64})))?;
    let total = data.meta["records"].as_i64().unwrap_or(1).max(1);
    let started = Instant::now();
    let mut added = 0i64;
    for grid in data.grids() {
        let (year, crs) = (grid["year"].as_i64().unwrap_or(0), grid["crs"].as_str().unwrap_or_default());
        let zone = &crs[crs.len() - 2..];
        let time = years_range(year, year);
        for row in 0..grid["height"].as_i64().unwrap_or(0) {
            for column in 0..grid["width"].as_i64().unwrap_or(0) {
                let Some(embedding) = data.embedding(grid, column, row) else { continue };
                index.add(&json!({"id": format!("{year}-{zone}-{column}-{row}"), "embedding": embedding, "crs": crs, "time": time,
                                  "extent": Data::cell_extent(grid, column, row), "metadata": {"year": year, "column": column, "row": row}}))?;
                added += 1;
            }
            if row % 16 == 0 {
                window.set_control_value(PROGRESS, (100 * added / total) as f64, "")?;
                window.set_status_text(&format!("Building the index: {added} of {total} records…"))?;
                window.process_events();
                if !window.is_visible()? {
                    return Err("Closed.".into());
                }
            }
        }
    }
    window.set_status_text(&format!("Index built: {added} records in {:.1} s.", started.elapsed().as_secs_f64()))?;
    Ok(index)
}

/// The search state: data, index and the current choices.
struct Search {
    data: Data,
    index: EmbeddingIndex,
    output: PathBuf,
    place: usize,
    seed_year: i64,
    extent: usize,
    from: i64,
    to: i64,
    last: Value,
}

impl Search {
    fn place(&self, index: usize) -> &Value {
        &self.data.meta["places"][index]
    }
    fn extent(&self, index: usize) -> &Value {
        &self.data.meta["extents"][index]
    }
    fn find(&self, key: &str, name: &str, crs: Option<&str>) -> usize {
        self.data.meta[key].as_array().into_iter().flatten()
            .position(|v| v["name"] == name && crs.map_or(true, |c| v["crs"] == c))
            .unwrap_or(0)
    }

    fn show_background(window: &mut ViewerWindow, year: i64) -> Result<()> {
        // The previews are the bottom three layers: 2018 at the very bottom, then 2021, then 2024.
        let mut viewer = window.viewer();
        let count = viewer.get_layer_count()?;
        for (offset, preview) in YEARS.iter().enumerate() {
            viewer.set_layer_visible(count - 1 - offset as i32, *preview == year)?;
        }
        Ok(())
    }

    /// GeoJSON layers in WGS84 (cell centres from centers.bin); the viewer projects them onto the map.
    fn show_results(&self, window: &mut ViewerWindow, place: &Value, extent: &Value, matches: &[Value]) -> Result<()> {
        let point = |lon: f64, lat: f64, properties: Value| json!({"type": "Feature", "properties": properties, "geometry": {"type": "Point", "coordinates": [lon, lat]}});
        let match_points: Vec<Value> = matches.iter().enumerate().filter_map(|(i, m)| {
            let (lon, lat) = self.data.center(m["crs"].as_str()?, m["metadata"]["column"].as_i64()?, m["metadata"]["row"].as_i64()?)?;
            Some(point(f64::from(lon), f64::from(lat), json!({"rank": i + 1, "similarity": m["similarity"], "year": m["metadata"]["year"]})))
        }).collect();
        let layers = [
            vec![json!({"type": "Feature", "properties": {"name": extent["name"]}, "geometry": {"type": "Polygon", "coordinates": [extent["outline"]]}})],
            match_points,
            vec![point(place["lon"].as_f64().unwrap_or(0.0), place["lat"].as_f64().unwrap_or(0.0), json!({"name": place["name"]}))],
        ];
        let mut viewer = window.viewer();
        for ((name, style), features) in RESULT_LAYERS.iter().zip(layers) {
            viewer.remove_layer_by_name(name)?;
            let file = self.output.join(format!("{name}.geojson"));
            fs::write(&file, json!({"type": "FeatureCollection", "features": features}).to_string())?;
            if viewer.add_layer_file(file.to_str().ok_or("Invalid path")?)? {
                viewer.set_layer_style_json(0, style)?;
            }
        }
        viewer.refresh_layers()?;
        Ok(())
    }

    /// The query: the seed cell's embedding with the CRS, extent and time filters. True when every match passes them.
    fn run(&mut self, window: &mut ViewerWindow, place_index: usize, seed_year: i64, extent_index: usize, first: i64, last: i64) -> Result<bool> {
        let place = self.place(place_index).clone();
        let extent = self.extent(extent_index).clone();
        let crs = extent["crs"].as_str().unwrap_or_default().to_owned();
        let grid = self.data.grid(seed_year, place["crs"].as_str().unwrap_or_default()).ok_or("No grid for the seed")?;
        let embedding = self.data.embedding(grid, place["column"].as_i64().unwrap_or(0), place["row"].as_i64().unwrap_or(0)).ok_or("The seed cell has no data")?;
        let started = Instant::now();
        let result = self.index.query(&json!({"embedding": embedding, "extent": extent["extent"], "crs": crs,
                                              "time": years_range(first, last), "maximumResults": 20}))?;
        let ms = started.elapsed().as_secs_f64() * 1000.0;
        let matches = result["matches"].as_array().cloned().unwrap_or_default();
        let lines = [
            String::new(),
            format!("━━ {}, {seed_year} → {crs}, {}, {first}–{last}", text(&place["name"]), text(&extent["name"])),
            format!("{} matches in {ms:.1} ms among {} records.", matches.len(), self.index.len()),
            format!("Similarity evaluated: {}{}", result["candidatesExamined"], if result["candidateBudgetReached"] == true { " (evaluation budget reached)" } else { "" }),
            format!("Rejected by the CRS filter (not {crs}): {}", result["crsRejected"]),
            format!("Rejected by the extent filter (outside {}): {}", text(&extent["name"]), result["spatialRejected"]),
            format!("Rejected by the time filter (not {first}–{last}): {}", result["temporalRejected"]),
            if place["crs"] != crs.as_str() {
                format!("The seed is a cell in {}; its embedding is compared with records in {crs}, never its extent.", text(&place["crs"]))
            } else {
                "Every match shares the query's CRS, intersects its extent and lies in its years.".to_owned()
            },
        ];
        for line in &lines {
            window.append_log(line)?;
        }
        let options: Vec<String> = matches.iter().enumerate()
            .map(|(i, m)| format!("{}. {} · {:.3} ({})", i + 1, m["metadata"]["year"], m["similarity"].as_f64().unwrap_or(0.0), text(&m["id"])))
            .collect();
        window.set_control_options_json(MATCHES, &serde_json::to_string(&if options.is_empty() { vec!["(no match)".to_owned()] } else { options })?)?;
        self.show_results(window, &place, &extent, &matches)?;
        Self::show_background(window, last)?;
        window.set_status_text(&format!("Seed: {}, {seed_year} (cell {}, {} in {}).", text(&place["name"]), place["column"], place["row"], text(&place["crs"])))?;
        self.last = result;
        // The guarantee: nothing comes from outside the filters.
        let b: Vec<f64> = extent["extent"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
        Ok(!matches.is_empty() && matches.iter().all(|m| {
            let e: Vec<f64> = m["extent"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
            let year = m["metadata"]["year"].as_i64().unwrap_or(0);
            m["crs"] == crs.as_str() && year >= first && year <= last && e[0] <= b[2] && e[2] >= b[0] && e[1] <= b[3] && e[3] >= b[1]
        }))
    }

    fn zoom_to_match(&self, window: &mut ViewerWindow, position: usize) -> Result<()> {
        let Some(m) = self.last["matches"].get(position) else { return Ok(()) };
        // The match is in a UTM zone, the map in RD New (the previews' CRS).
        let e: Vec<f64> = m["extent"].as_array().into_iter().flatten().filter_map(Value::as_f64).collect();
        let epsg: i32 = text(&m["crs"]).trim_start_matches("EPSG:").parse()?;
        let mut viewer = window.viewer();
        let (x, y) = viewer.transform_point(epsg, 28992, (e[0] + e[2]) / 2.0, (e[1] + e[3]) / 2.0)?;
        viewer.set_view_extent(Extent { x_min: x - 8000.0, y_min: y - 8000.0, x_max: x + 8000.0, y_max: y + 8000.0 })?;
        Ok(())
    }
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

fn run(runtime: &Runtime, window: &mut ViewerWindow, smoke: bool) -> Result<bool> {
    let years: Vec<String> = YEARS.iter().map(i64::to_string).collect();
    let events = window.add_control_panel(&json!({"title": TITLE, "width": 380, "controls": [
        {"id": PLACE, "type": "combo", "label": "Look for places like", "options": ["(loading)"], "value": "(loading)"},
        {"id": SEED_YEAR, "type": "combo", "label": "Seed year", "options": years, "value": "2024"},
        {"id": EXTENT, "type": "combo", "label": "Search in (CRS · extent)", "options": ["(loading)"], "value": "(loading)"},
        {"id": FROM, "type": "combo", "label": "From year", "options": years, "value": "2024"},
        {"id": TO, "type": "combo", "label": "To year", "options": years, "value": "2024"},
        {"id": SEARCH, "type": "button", "text": "Search"},
        {"id": MATCHES, "type": "combo", "label": "Matches (pick to zoom)", "options": ["(search first)"], "value": ""},
        {"id": PROGRESS, "type": "progress", "label": "Progress", "value": 0}
    ]}).to_string())?;
    window.add_log_panel("Filters")?;
    window.set_attribution_text(ATTRIBUTION)?;
    window.show()?;
    window.process_events();
    window.set_status_text("Preparing the Netherlands AlphaEarth embeddings (14 MB on the first run)…")?;
    let (progress, messages) = mpsc::channel::<String>();
    let (sender, preparing): (Sender<std::result::Result<PathBuf, String>>, Receiver<_>) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(prepare(&progress).map_err(|e| e.to_string()));
    });
    let mut search: Option<Search> = None;
    let mut exit_at = None::<Instant>;
    let mut passed = true;

    while window.is_visible()? {
        window.process_events();
        while let Ok(message) = messages.try_recv() {
            window.set_status_text(&message)?;
        }
        if search.is_none() {
            if let Ok(prepared) = preparing.try_recv() {
                let data = Data::load(PathBuf::from(prepared?))?;
                let index = build_index(runtime, window, &data)?;
                {
                    // The map is in RD New, the CRS of the false-colour previews; they stay at the bottom.
                    let mut viewer = window.viewer();
                    for year in YEARS {
                        viewer.add_layer_file(data.directory.join(format!("preview_{year}.tif")).to_str().ok_or("Invalid path")?)?;
                        viewer.set_layer_opacity(0, 0.75)?;
                    }
                }
                Search::show_background(window, 2024)?;
                window.viewer().set_view_extent(Extent { x_min: 0.0, y_min: 300000.0, x_max: 300000.0, y_max: 625000.0 })?;
                let places: Vec<String> = data.meta["places"].as_array().into_iter().flatten().map(|p| format!("{} — {}", text(&p["name"]), text(&p["note"]))).collect();
                let extents: Vec<String> = data.meta["extents"].as_array().into_iter().flatten().map(|e| format!("{} · {}", text(&e["crs"]), text(&e["name"]))).collect();
                window.set_control_options_json(PLACE, &serde_json::to_string(&places)?)?;
                window.set_control_options_json(EXTENT, &serde_json::to_string(&extents)?)?;
                window.set_control_value(PROGRESS, 100.0, "")?;
                window.append_log(&format!("Ready: {} records (3 years × 2 UTM zones). Pick a place, an extent (it sets the CRS) and years, then Search.", index.len()))?;
                let output = std::env::temp_dir().join(format!("geokernel-embedding-search-{}", std::process::id()));
                fs::create_dir_all(&output)?;
                let mut opened = Search { data, index, output, place: 0, seed_year: 2024, extent: 0, from: 2024, to: 2024, last: Value::Null };
                if smoke {
                    let solar = opened.find("places", "Solar park", None);
                    let checks = [
                        ("time: solar park 2024, zone 31, 2018 only", solar, opened.find("extents", "Whole zone 31 (west of 6° E)", Some("EPSG:32631")), 2018, 2018),
                        ("extent: greenhouses 2024, zone 31, Randstad, all years", opened.find("places", "Greenhouses", None), opened.find("extents", "Randstad", Some("EPSG:32631")), 2018, 2024),
                        ("crs: solar park 2024 (zone 31) searched in zone 32, Groningen and Drenthe, 2024", solar, opened.find("extents", "Groningen and Drenthe", Some("EPSG:32632")), 2024, 2024),
                    ];
                    for (label, place, extent, first, last) in checks {
                        let ok = opened.run(window, place, 2024, extent, first, last)?;
                        passed &= ok;
                        let l = &opened.last;
                        println!("smoke | {label}: {} — {} matches, best {:.3}, rejected crs {} / extent {} / time {}",
                                 if ok { "ok" } else { "FAILED" }, l["matches"].as_array().map_or(0, Vec::len), l["matches"][0]["similarity"].as_f64().unwrap_or(0.0),
                                 l["crsRejected"], l["spatialRejected"], l["temporalRejected"]);
                    }
                    exit_at = Some(Instant::now() + Duration::from_secs(1));
                }
                search = Some(opened);
            }
        }
        while let Ok(event) = events.try_recv() {
            let Some(active) = search.as_mut() else { continue };
            let year = |number: f64, fallback: i64| YEARS.get(number.round() as usize).copied().unwrap_or(fallback);
            match event.id {
                PLACE => active.place = event.number.round() as usize,
                SEED_YEAR => active.seed_year = year(event.number, active.seed_year),
                EXTENT => active.extent = event.number.round() as usize,
                FROM => active.from = year(event.number, active.from),
                TO => active.to = year(event.number, active.to),
                SEARCH => {
                    let (place, seed, extent, first, last) = (active.place, active.seed_year, active.extent, active.from.min(active.to), active.from.max(active.to));
                    if let Err(error) = active.run(window, place, seed, extent, first, last) {
                        window.set_status_text(&error.to_string())?;
                    }
                }
                MATCHES => {
                    if let Some(position) = event.text.split('.').next().and_then(|n| n.trim().parse::<usize>().ok()) {
                        let _ = active.zoom_to_match(window, position.saturating_sub(1));
                    }
                }
                _ => {}
            }
        }
        if exit_at.is_some_and(|at| Instant::now() >= at) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(search);
    Ok(passed)
}

fn main() -> Result<()> {
    let smoke = std::env::args().any(|argument| argument == "--smoke");
    // This process owns the main thread and loads the trusted runtime named by GEOKERNEL_BIN.
    let runtime = unsafe { Runtime::from_env()? };
    let outcome = (|| {
        let mut window = unsafe { ViewerWindow::new(&runtime, TITLE, 1460, 920)? };
        run(&runtime, &mut window, smoke)
    })();
    let shutdown = unsafe { runtime.shutdown_viewer_application() };
    let passed = outcome?;
    shutdown?;
    if !passed {
        std::process::exit(2);
    }
    Ok(())
}
