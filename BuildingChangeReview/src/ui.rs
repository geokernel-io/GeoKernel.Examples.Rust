use crate::{
    core::{self, Inputs, Policy, Workspace},
    gdal::{self, Gdal},
};
use geokernel::{ControlEvent, Extent, Runtime, Viewer, ViewerTool, ViewerWindow};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc,
    },
    thread::{self, JoinHandle},
};
pub type Result<T> = geokernel_geoai_examples::Result<T>;
const AREA: i32 = 1;
const FILTER: i32 = 2;
const PROVIDER: i32 = 3;
const THRESHOLD: i32 = 4;
const RUN: i32 = 5;
const CANCEL: i32 = 6;
const PROGRESS: i32 = 7;
const SUMMARY: i32 = 8;
const CANDIDATE: i32 = 9;
const SELECTED: i32 = 10;
const ACTOR: i32 = 11;
const NOTE: i32 = 12;
const APPROVE: i32 = 13;
const REJECT: i32 = 14;
const PENDING: i32 = 15;
const LOCATE: i32 = 16;
const SAVE: i32 = 17;
const OVERLAYS: i32 = 18;
enum Done {
    Prepared(Inputs, [PathBuf; 2]),
    Analyzed(Value),
}
pub struct App {
    pub window: ViewerWindow,
    workspace: Workspace,
    bin: PathBuf,
    gdal: Gdal,
    controls: Receiver<ControlEvent>,
    commands: Receiver<i32>,
    progress_tx: Sender<(i32, String)>,
    progress_rx: Receiver<(i32, String)>,
    worker: Option<JoinHandle<gdal::R<Done>>>,
    cancel: Arc<AtomicBool>,
    inputs: Option<Inputs>,
    pub result: Option<Value>,
    pub selected: Option<usize>,
    revision: usize,
    candidates_dirty: bool,
    candidate_styles: [Option<String>; 2],
    last: Option<[Extent; 2]>,
    area: String,
    filter: String,
    provider: String,
    threshold: f64,
    actor: String,
    note: String,
    overlays: bool,
    pub closed: bool,
    closing: bool,
}
impl App {
    pub fn new(runtime: &Runtime) -> Result<Self> {
        let bin = PathBuf::from(std::env::var_os("GEOKERNEL_BIN").ok_or("Runtime path missing")?);
        let gdal = Gdal::load(&bin)?;
        gdal.initialize();
        let mut window = unsafe {
            ViewerWindow::new_dual(
                runtime,
                "Building change review — Rust | 2016 before / 2025 after",
                1460,
                860,
            )?
        };
        for side in 0..2 {
            window.pane(side)?.set_coordinate_system_preset("wgs84")?;
            window.pane(side)?.use_tool(ViewerTool::Pan);
        }
        let controls=window.add_control_panel(&json!({"title":"Oosterwold · 2016 → 2025","area":"right","width":380,"controls":[
   {"id":AREA,"type":"combo","label":"Area","options":["Demo area (fixed)","Current view","Full dataset"],"value":"Demo area (fixed)"},
   {"id":FILTER,"type":"combo","label":"Filter","options":["Conservative","Balanced"],"value":"Conservative"},
   {"id":PROVIDER,"type":"combo","label":"Provider","options":["CPU","CUDA"],"value":"CPU"},
   {"id":THRESHOLD,"type":"number","label":"Building threshold","minimum":0.05,"maximum":0.95,"decimals":4,"step":0.025,"value":0.4371},
   {"id":RUN,"type":"button","text":"Run analysis"},{"id":CANCEL,"type":"button","text":"Cancel"},
   {"id":PROGRESS,"type":"progress","format":"%p%","value":0},{"id":SUMMARY,"type":"text","label":"Candidates","value":"Preparing imagery…"},
   {"id":CANDIDATE,"type":"combo","label":"Candidate","options":[],"value":""},{"id":SELECTED,"type":"text","label":"Review","value":""},
   {"id":ACTOR,"type":"text","label":"Reviewer","value":"demo-reviewer"},{"id":NOTE,"type":"text","label":"Note","value":""},
   {"id":APPROVE,"type":"button","text":"Approve"},{"id":REJECT,"type":"button","text":"Reject"},{"id":PENDING,"type":"button","text":"Pending"},
   {"id":LOCATE,"type":"button","text":"Locate"},{"id":SAVE,"type":"button","text":"Export approved GeoJSON"},
   {"id":OVERLAYS,"type":"checkbox","label":"Overlays","text":"Show candidate polygons","checked":true}
  ]}).to_string())?;
        let commands=window.add_command_toolbar(&json!({"commands":[{"id":101,"text":"Zoom in"},{"id":102,"text":"Zoom out"},{"id":103,"text":"Demo area"},{"id":104,"text":"Full extent"},{"id":105,"text":"Pan"},{"id":106,"text":"Edit vertices"},{"id":107,"text":"Undo"},{"id":108,"text":"Redo"},{"id":109,"text":"Attribution"}]}).to_string())?;
        let (tx, rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let workspace = Workspace::new("geokernel-change-view-")?;
        let worker =
            Self::spawn_preparation(bin.clone(), workspace.0.clone(), cancel.clone(), tx.clone());
        window.show()?;
        let mut app = Self {
            window,
            workspace,
            bin,
            gdal,
            controls,
            commands,
            progress_tx: tx,
            progress_rx: rx,
            worker: Some(worker),
            cancel,
            inputs: None,
            result: None,
            selected: None,
            revision: 0,
            candidates_dirty: true,
            candidate_styles: [None, None],
            last: None,
            area: "Demo area (fixed)".into(),
            filter: "Conservative".into(),
            provider: "cpu".into(),
            threshold: 0.4371,
            actor: "demo-reviewer".into(),
            note: String::new(),
            overlays: true,
            closed: false,
            closing: false,
        };
        app.controls()?;
        Ok(app)
    }
    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }
    pub fn is_ready(&self) -> bool {
        self.inputs.is_some()
    }
    fn spawn_preparation(
        bin: PathBuf,
        directory: PathBuf,
        cancel: Arc<AtomicBool>,
        progress: Sender<(i32, String)>,
    ) -> JoinHandle<gdal::R<Done>> {
        thread::spawn(move || {
            let inputs = core::prepare(&cancel, &progress)?;
            let gdal = Gdal::load(&bin)?;
            gdal.initialize();
            let displays = [
                directory.join("display-0.tif"),
                directory.join("display-1.tif"),
            ];
            for side in 0..2 {
                geokernel_geoai_examples::assets::check(&cancel)?;
                let _ = progress.send((
                    50 + side as i32 * 20,
                    format!("Preparing {} map…", if side == 0 { 2016 } else { 2025 }),
                ));
                gdal.display_raster(&inputs.paths[side], &displays[side])?;
            }
            geokernel_geoai_examples::assets::check(&cancel)?;
            Ok(Done::Prepared(inputs, displays))
        })
    }
    fn publish_inputs(&mut self, input: Inputs, displays: [PathBuf; 2]) -> Result<()> {
        for (side, path) in displays.iter().enumerate() {
            let mut viewer = self.window.pane(side)?;
            if !viewer.add_layer_file(&path.to_string_lossy())? {
                return Err("Cannot display imagery".into());
            }
            viewer.set_layer_name(0, "Imagery")?;
            let e = viewer.layer_projected_extent(0)?;
            if ![e.x_min, e.y_min, e.x_max, e.y_max]
                .iter()
                .all(|v| v.is_finite())
                || e.x_min < -180.
                || e.x_max > 180.
                || e.y_min < -90.
                || e.y_max > 90.
                || e.x_min >= e.x_max
                || e.y_min >= e.y_max
            {
                return Err("Imagery did not load in WGS84".into());
            }
        }
        self.focus(core::DEMO)?;
        self.inputs = Some(input);
        self.window
            .set_control_value(SUMMARY, 0., "Run analysis to find candidates.")?;
        self.window.set_control_value(PROGRESS, 0., "%p%")?;
        self.window
            .set_status_text("Ready · Beeldmateriaal / PDOK · CC BY 4.0")?;
        Ok(())
    }
    fn controls(&mut self) -> Result<()> {
        let enabled = !self.busy() && !self.closing;
        for id in [AREA, FILTER, PROVIDER, THRESHOLD, CANDIDATE] {
            self.window
                .set_control_enabled(id, enabled && self.is_ready())?;
        }
        self.window.set_control_enabled(RUN, enabled)?;
        for id in [APPROVE, REJECT, PENDING, LOCATE] {
            self.window
                .set_control_enabled(id, enabled && self.selected.is_some())?;
        }
        self.window
            .set_control_enabled(CANCEL, self.busy() && !self.closing)?;
        let approved = self.result.as_ref().is_some_and(|r| {
            r["features"].as_array().is_some_and(|features| {
                features
                    .iter()
                    .any(|f| f["properties"]["review_status"] == "approved")
            })
        });
        self.window.set_control_enabled(SAVE, enabled && approved)?;
        if let Some(result) = &self.result {
            let features = result["features"].as_array().ok_or("Missing features")?;
            let new = features
                .iter()
                .filter(|f| f["properties"]["change_type"] == "new")
                .count();
            self.window.set_control_value(
                SUMMARY,
                0.,
                &format!("{new} new · {} removed", features.len() - new),
            )?;
            if let Some(index) = self.selected {
                let f = &features[index];
                self.window.set_control_value(
                    SELECTED,
                    0.,
                    &format!(
                        "{:0.1} m² · {}",
                        f["properties"]["area_m2"].as_f64().unwrap_or(0.),
                        f["properties"]["review_status"]
                            .as_str()
                            .unwrap_or("pending")
                    ),
                )?;
            }
        }
        Ok(())
    }
    pub fn focus(&mut self, bounds: [f64; 4]) -> Result<()> {
        let (geometry, _) = self.gdal.geometry(&gdal::rectangle(bounds), 25831, 4326)?;
        let b = gdal::envelope(&geometry)?;
        let extent = Extent {
            x_min: b[0],
            y_min: b[1],
            x_max: b[2],
            y_max: b[3],
        };
        for side in 0..2 {
            self.window.pane(side)?.set_view_extent(extent)?;
        }
        self.last = Some([
            self.window.pane(0)?.view_extent()?,
            self.window.pane(1)?.view_extent()?,
        ]);
        Ok(())
    }
    fn sync(&mut self) -> Result<()> {
        if self.inputs.is_none() {
            return Ok(());
        }
        let current = [
            self.window.pane(0)?.view_extent()?,
            self.window.pane(1)?.view_extent()?,
        ];
        fn same(a: Extent, b: Extent) -> bool {
            [
                (a.x_min, b.x_min),
                (a.y_min, b.y_min),
                (a.x_max, b.x_max),
                (a.y_max, b.y_max),
            ]
            .iter()
            .all(|(x, y)| (x - y).abs() < 1e-10)
        }
        if let Some(last) = self.last {
            if !same(current[0], last[0]) {
                self.window.pane(1)?.set_view_extent(current[0])?;
            } else if !same(current[1], last[1]) {
                self.window.pane(0)?.set_view_extent(current[1])?;
            }
        }
        self.last = Some([
            self.window.pane(0)?.view_extent()?,
            self.window.pane(1)?.view_extent()?,
        ]);
        Ok(())
    }
    pub fn run(&mut self) -> Result<()> {
        if self.busy() {
            return Ok(());
        }
        if self.inputs.is_none() {
            self.cancel.store(false, Ordering::Relaxed);
            self.window
                .set_control_value(SUMMARY, 0., "Preparing imagery…")?;
            self.worker = Some(Self::spawn_preparation(
                self.bin.clone(),
                self.workspace.0.clone(),
                self.cancel.clone(),
                self.progress_tx.clone(),
            ));
            return self.controls();
        }
        self.capture()?;
        if self.result.is_some()
            && !crate::dialog::confirm(self.window.viewer().get_native_handle()?)
        {
            return Ok(());
        }
        let input = self.inputs.clone().ok_or("Imagery not prepared")?;
        let mut bounds = if self.area == "Full dataset" {
            core::BOUNDS
        } else {
            core::DEMO
        };
        if self.area == "Current view" {
            let e = self.window.pane(0)?.view_extent()?;
            bounds = gdal::envelope(
                &self
                    .gdal
                    .geometry(
                        &gdal::rectangle([e.x_min, e.y_min, e.x_max, e.y_max]),
                        4326,
                        25831,
                    )?
                    .0,
            )?;
        }
        let policy = if self.filter == "Balanced" {
            Policy::BALANCED
        } else {
            Policy::CONSERVATIVE
        };
        let threshold = self.threshold;
        let provider = self.provider.clone();
        let bin = self.bin.clone();
        let c = self.cancel.clone();
        c.store(false, Ordering::Relaxed);
        let tx = self.progress_tx.clone();
        self.worker = Some(thread::spawn(move || {
            core::analyze(&bin, &input, bounds, policy, threshold, &provider, &c, &tx)
                .map(Done::Analyzed)
        }));
        self.controls()?;
        Ok(())
    }
    fn write(&mut self, features: Value) -> Result<PathBuf> {
        self.revision += 1;
        let path = self
            .workspace
            .0
            .join(format!("overlay-{}.geojson", self.revision));
        fs::write(
            &path,
            json!({"type":"FeatureCollection","features":features}).to_string(),
        )?;
        Ok(path)
    }
    pub fn redraw(&mut self) -> Result<()> {
        if !self.overlays {
            return Ok(());
        }
        if self.candidates_dirty {
            self.end_edit()?;
        }
        self.ensure_candidates()?;
        self.update_selection()
    }
    fn layer_index(viewer: &mut Viewer<'_>, name: &str) -> Result<i32> {
        let info: Value = serde_json::from_str(&viewer.get_layer_info_by_name_json(name)?)?;
        Ok(if info["isValid"] == true {
            info["index"].as_i64().unwrap_or(-1) as i32
        } else {
            -1
        })
    }
    fn ensure_candidates(&mut self) -> Result<()> {
        if !self.candidates_dirty {
            return Ok(());
        }
        for side in 0..2 {
            while self.window.pane(side)?.remove_layer_by_name("Candidates")? {}
            while self
                .window
                .pane(side)?
                .remove_layer_by_name("Selection targets")?
            {}
            self.candidate_styles[side] = None;
            if self.result.is_none() {
                continue;
            }
            let mut targets = self.result.as_ref().unwrap()["features"].clone();
            if side == 0 {
                for feature in targets.as_array_mut().ok_or("Missing features")? {
                    feature["geometry"] = gdal::rectangle(gdal::envelope(&feature["geometry"])?);
                }
            }
            if !targets.as_array().ok_or("Missing features")?.is_empty() {
                let path = self.write(targets)?;
                let mut viewer = self.window.pane(side)?;
                if !viewer.add_layer_file(&path.to_string_lossy())? {
                    return Err("Cannot display selection references".into());
                }
                viewer.set_layer_name(0, "Selection targets")?;
                if !viewer.set_layer_style_json(0, &json!({"fillOpacity":0,"lineColor":"#00000000","lineWidth":0,"selectedLineColor":"#ef9b00","selectedLineWidth":2}).to_string())? {
                    return Err("Cannot style selection references".into());
                }
            }
            let kind = if side == 0 { "removed" } else { "new" };
            let features: Vec<Value> = self.result.as_ref().unwrap()["features"]
                .as_array()
                .ok_or("Missing features")?
                .iter()
                .filter(|f| f["properties"]["change_type"] == kind)
                .cloned()
                .collect();
            if !features.is_empty() {
                let path = self.write(json!(features))?;
                if !self
                    .window
                    .pane(side)?
                    .add_layer_file(&path.to_string_lossy())?
                {
                    return Err("Cannot display candidates".into());
                }
                self.window.pane(side)?.set_layer_name(0, "Candidates")?;
            }
        }
        self.candidates_dirty = false;
        self.apply_candidate_styles()
    }
    fn apply_candidate_styles(&mut self) -> Result<()> {
        let Some(result) = &self.result else {
            return Ok(());
        };
        for side in 0..2 {
            let mut viewer = self.window.pane(side)?;
            let index = Self::layer_index(&mut viewer, "Candidates")?;
            if index < 0 {
                continue;
            }
            let kind = if side == 0 { "removed" } else { "new" };
            let color = if side == 0 { "#ee554f" } else { "#20bf73" };
            let rules: Vec<Value> = result["features"].as_array().ok_or("Missing features")?.iter()
                .filter(|f| f["properties"]["change_type"] == kind && f["properties"]["review_status"] != "pending")
                .map(|f| json!({"field":"candidate_id","operator":"equals","value":f["id"],"style":if f["properties"]["review_status"] == "approved" {
                    json!({"fillColor":color,"lineColor":color,"fillOpacity":90,"lineWidth":3})
                } else { json!({"fillColor":"#82909f","lineColor":"#82909f","fillOpacity":10,"lineWidth":1}) }})).collect();
            let renderer = json!({"type":"ruleBased","defaultStyle":{"fillColor":color,"lineColor":color,"fillOpacity":35,"lineWidth":2},"rules":rules}).to_string();
            if self.candidate_styles[side].as_ref() != Some(&renderer) {
                if !viewer.set_layer_symbol_renderer_json(index, &renderer)? {
                    return Err("Cannot style candidates".into());
                }
                self.candidate_styles[side] = Some(renderer);
            }
        }
        Ok(())
    }
    fn highlight(
        viewer: &mut Viewer<'_>,
        layer: i32,
        geometry: &Value,
        candidate: Option<&str>,
    ) -> Result<()> {
        if layer < 0 {
            return Err("Selection references are unavailable".into());
        }
        let [a, b, c, d] = gdal::envelope(geometry)?;
        let hits: Value =
            serde_json::from_str(&viewer.hit_test_features_in_extent_json(a, b, c, d)?)?;
        let hit = hits
            .as_array()
            .ok_or("Invalid hit list")?
            .iter()
            .find(|h| {
                h["layerIndex"] == layer
                    && candidate.is_none_or(|id| h["attributes"]["candidate_id"] == id)
            })
            .ok_or("Cannot find selected candidate")?;
        // The centre may be outside a concave roof; the ID and tolerance identify it.
        if !viewer.select_feature_hit(
            (a + c) / 2.,
            (b + d) / 2.,
            (c - a).max(d - b).max(1e-9) * 2.,
            layer,
            hit["shapeId"].as_i64().ok_or("Missing shape ID")? as i32,
            hit["featureId"].as_i64().ok_or("Missing feature ID")? as i32,
        )? {
            return Err("Cannot highlight selected candidate".into());
        }
        Ok(())
    }
    fn update_selection(&mut self) -> Result<()> {
        for side in 0..2 {
            let mut viewer = self.window.pane(side)?;
            let Some(selected) = self
                .selected
                .filter(|_| self.overlays && self.result.is_some())
            else {
                viewer.clear_selected_features()?;
                continue;
            };
            for name in ["Candidates", "Selection targets", "Selection"] {
                let info: Value = serde_json::from_str(&viewer.get_layer_info_by_name_json(name)?)?;
                if info["isValid"] == true && info["visible"] == false {
                    viewer.set_layer_visible(
                        info["index"].as_i64().ok_or("Missing index")? as i32,
                        true,
                    )?;
                }
            }
            let feature = &self.result.as_ref().unwrap()["features"][selected];
            let geometry = if side == 0 {
                gdal::rectangle(gdal::envelope(&feature["geometry"])?)
            } else {
                feature["geometry"].clone()
            };
            let editor = if side == 1 {
                Self::layer_index(&mut viewer, "Selection")?
            } else {
                -1
            };
            let target = if editor >= 0 {
                editor
            } else {
                Self::layer_index(&mut viewer, "Selection targets")?
            };
            Self::highlight(
                &mut viewer,
                target,
                &geometry,
                if editor >= 0 {
                    None
                } else {
                    feature["id"].as_str()
                },
            )?;
        }
        Ok(())
    }
    fn end_edit(&mut self) -> Result<()> {
        let mut viewer = self.window.pane(1)?;
        if Self::layer_index(&mut viewer, "Selection")? >= 0 {
            viewer.clear_selected_features()?;
            viewer.remove_layer_by_name("Selection")?;
            viewer.use_tool(ViewerTool::Pan);
        }
        Ok(())
    }
    pub fn use_tool(&mut self, tool: ViewerTool) -> Result<()> {
        if matches!(tool, ViewerTool::EditVertices) {
            let selected = self
                .selected
                .filter(|_| self.overlays)
                .ok_or("Select a visible candidate before editing")?;
            let mut viewer = self.window.pane(1)?;
            if Self::layer_index(&mut viewer, "Selection")? < 0 {
                let geometry = &self.result.as_ref().ok_or("Run analysis first")?["features"]
                    [selected]["geometry"];
                if geometry["type"] != "Polygon" {
                    return Err(
                        "Multipart geometry is preserved but cannot be edited in this example"
                            .into(),
                    );
                }
                let ring: Vec<[f64; 2]> = geometry["coordinates"][0]
                    .as_array()
                    .ok_or("Polygon ring missing")?
                    .iter()
                    .map(|p| {
                        Ok([
                            p[0].as_f64().ok_or("Invalid X")?,
                            p[1].as_f64().ok_or("Invalid Y")?,
                        ])
                    })
                    .collect::<std::result::Result<_, String>>()?;
                viewer.clear_selected_features()?;
                let index = viewer.add_polygon_layer(
                    "Selection",
                    &ring,
                    &json!({"fillOpacity":0,"lineColor":"#ef9b00","lineWidth":2,"selectedLineColor":"#ef9b00","selectedLineWidth":2}).to_string(),
                )?;
                if index < 0 {
                    return Err("Cannot create selection".into());
                }
                if !viewer.begin_edit_layer(index)? || !viewer.set_active_edit_layer_index(index)? {
                    return Err("Cannot edit candidate".into());
                }
                Self::highlight(&mut viewer, index, geometry, None)?;
            }
        }
        self.window.pane(1)?.use_tool(tool);
        Ok(())
    }
    pub fn capture(&mut self) -> Result<bool> {
        if !self.overlays {
            return Ok(false);
        }
        if let (Some(selected), Some(result)) = (self.selected, &mut self.result) {
            let mut viewer = self.window.pane(1)?;
            let index = Self::layer_index(&mut viewer, "Selection")?;
            if index >= 0 && viewer.is_layer_editing(index)? {
                let text = viewer.write_layer_last_shape_geo_json(index)?;
                let value: Value = serde_json::from_str(&text)?;
                let edited = if value["geometry"].is_object() {
                    &value["geometry"]
                } else {
                    &value
                };
                let mut geometry = result["features"][selected]["geometry"].clone();
                if geometry["type"] != "Polygon" {
                    return Err("Expected Polygon".into());
                } // Public Rust AddPolygonLayer edits the exterior ring; keep all original holes.
                geometry["coordinates"][0] = edited["coordinates"][0].clone();
                let previous_status =
                    result["features"][selected]["properties"]["review_status"].clone();
                let changed =
                    core::update(&mut result["features"][selected], geometry, &self.gdal)?;
                if changed {
                    self.candidates_dirty = true;
                    if previous_status != "pending" {
                        self.apply_candidate_styles()?;
                    }
                }
                return Ok(changed);
            }
        }
        Ok(false)
    }
    pub fn select(&mut self, index: usize) -> Result<()> {
        if self.selected == Some(index) {
            return Ok(());
        }
        self.capture()?;
        self.end_edit()?;
        let previous = self.selected;
        self.selected = Some(index);
        if let Err(error) = self.redraw() {
            self.selected = previous;
            return Err(error);
        }
        self.note = self.result.as_ref().unwrap()["features"][index]["properties"]["review_note"]
            .as_str()
            .unwrap_or("")
            .into();
        self.window.set_control_value(NOTE, 0., &self.note)?;
        self.controls()
    }
    pub fn review(&mut self, status: &str) -> Result<()> {
        self.capture()?;
        if let (Some(selected), Some(result)) = (self.selected, &mut self.result) {
            core::review(
                &mut result["features"][selected],
                status,
                &self.actor,
                &self.note,
                &self.gdal,
            )?;
        }
        self.apply_candidate_styles()?;
        self.controls()
    }
    pub fn export(&mut self, path: &Path) -> Result<()> {
        self.capture()?;
        core::export(
            self.result.as_ref().ok_or("Run analysis first")?,
            path,
            &self.gdal,
        )?;
        Ok(())
    }
    fn locate(&mut self) -> Result<()> {
        if let (Some(index), Some(result)) = (self.selected, &self.result) {
            let b = gdal::envelope(&result["features"][index]["geometry"])?;
            let dx = ((b[2] - b[0]) * 0.5).max(0.00015);
            let dy = ((b[3] - b[1]) * 0.5).max(0.00015);
            self.window.pane(0)?.set_view_extent(Extent {
                x_min: b[0] - dx,
                y_min: b[1] - dy,
                x_max: b[2] + dx,
                y_max: b[3] + dy,
            })?;
        }
        Ok(())
    }
    fn control(&mut self, e: ControlEvent) -> Result<()> {
        match e.id {
            AREA => self.area = e.text,
            FILTER => self.filter = e.text,
            PROVIDER => self.provider = e.text.to_lowercase(),
            THRESHOLD => self.threshold = e.number,
            ACTOR => self.actor = e.text,
            NOTE => self.note = e.text,
            CANCEL => self.cancel.store(true, Ordering::Relaxed),
            RUN => self.run()?,
            CANDIDATE => {
                let index = self
                    .result
                    .as_ref()
                    .and_then(|r| r["features"].as_array())
                    .and_then(|features| features.iter().position(|f| f["id"] == e.text));
                if let Some(index) = index {
                    if !self.busy() {
                        if let Err(error) = self.select(index) {
                            if let Some(previous) = self.selected {
                                let id = self.result.as_ref().unwrap()["features"][previous]["id"]
                                    .as_str()
                                    .unwrap_or("");
                                self.window.set_control_value(CANDIDATE, 0., id)?;
                            }
                            return Err(error);
                        }
                    }
                }
            }
            APPROVE => self.review("approved")?,
            REJECT => self.review("rejected")?,
            PENDING => self.review("pending")?,
            LOCATE => self.locate()?,
            SAVE => {
                let owner = self.window.viewer().get_native_handle()?;
                if let Some(path) = crate::dialog::save(owner)? {
                    self.export(Path::new(&path))?;
                }
            }
            OVERLAYS => {
                if let Err(error) = self.capture() {
                    self.window.set_control_value(
                        OVERLAYS,
                        if self.overlays { 1. } else { 0. },
                        "",
                    )?;
                    return Err(error);
                }
                self.overlays = e.number != 0.;
                if self.overlays {
                    self.redraw()?;
                } else {
                    for side in 0..2 {
                        let mut viewer = self.window.pane(side)?;
                        viewer.clear_selected_features()?;
                        for name in ["Selection", "Selection targets", "Candidates"] {
                            let index = Self::layer_index(&mut viewer, name)?;
                            if index >= 0 {
                                viewer.set_layer_visible(index, false)?;
                            }
                        }
                    }
                }
            }
            _ => (),
        }
        Ok(())
    }
    fn command(&mut self, id: i32) -> Result<()> {
        match id {
            101 => self.window.pane(0)?.zoom_in(1.5)?,
            102 => self.window.pane(0)?.zoom_out(1.5)?,
            103 => self.focus(core::DEMO)?,
            104 => self.focus(core::BOUNDS)?,
            105 => self.use_tool(ViewerTool::Pan)?,
            106 => self.use_tool(ViewerTool::EditVertices)?,
            107 | 108 => {
                let mut viewer = self.window.pane(1)?;
                let index = Self::layer_index(&mut viewer, "Selection")?;
                if self.overlays && index >= 0 {
                    if id == 107 {
                        viewer.undo_edit_layer(index)?;
                    } else {
                        viewer.redo_edit_layer(index)?;
                    }
                    self.capture()?;
                    self.controls()?;
                }
            }
            109 => {
                self.window.set_status_text(core::ATTRIBUTION)?;
            }
            _ => (),
        }
        Ok(())
    }
    pub fn begin_close(&mut self) {
        self.closing = true;
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn tick(&mut self) -> Result<()> {
        self.window.process_events();
        if !self.window.is_visible()? {
            self.begin_close();
        }
        while let Ok((percent, message)) = self.progress_rx.try_recv() {
            if !self.closing {
                self.window
                    .set_control_value(PROGRESS, percent as f64, "%p%")?;
                self.window.set_status_text(&message)?;
                if self.inputs.is_none() {
                    self.window.set_control_value(SUMMARY, 0., &message)?;
                }
            }
        }
        if self.worker.as_ref().is_some_and(|w| w.is_finished()) {
            let outcome = self
                .worker
                .take()
                .unwrap()
                .join()
                .map_err(|_| "Background worker panicked")?;
            if !self.closing {
                match outcome {
                    Ok(Done::Prepared(input, displays)) => {
                        if let Err(error) = self.publish_inputs(input, displays) {
                            for side in 0..2 {
                                self.window.pane(side)?.clear_layers()?;
                            }
                            self.window.set_status_text(&error.to_string())?;
                            self.window.set_control_value(
                                SUMMARY,
                                0.,
                                "Loading failed; click Run analysis to retry.",
                            )?;
                        }
                    }
                    Ok(Done::Analyzed(result)) => {
                        let options: Vec<String> = result["features"]
                            .as_array()
                            .ok_or("Missing features")?
                            .iter()
                            .map(|f| f["id"].as_str().unwrap_or("").into())
                            .collect();
                        self.end_edit()?;
                        self.selected = if options.is_empty() { None } else { Some(0) };
                        self.result = Some(result);
                        self.candidates_dirty = true;
                        self.window
                            .set_control_options_json(CANDIDATE, &json!(options).to_string())?;
                        if let Some(first) = options.first() {
                            self.window.set_control_value(CANDIDATE, 0., first)?;
                        }
                        self.redraw()?;
                        self.window
                            .set_status_text("Complete · predictions require review")?;
                    }
                    Err(error) => {
                        self.window.set_status_text(&error)?;
                        if self.inputs.is_none() {
                            self.window.set_control_value(
                                SUMMARY,
                                0.,
                                "Loading failed; click Run analysis to retry.",
                            )?;
                        }
                    }
                }
                self.controls()?;
            }
        }
        if self.closing {
            if self.worker.is_none() {
                for side in 0..2 {
                    self.window.pane(side)?.clear_layers()?;
                }
                self.closed = true;
            }
            return Ok(());
        }
        while let Ok(command) = self.commands.try_recv() {
            if let Err(e) = self.command(command) {
                self.window.set_status_text(&e.to_string())?;
            }
        }
        while let Ok(event) = self.controls.try_recv() {
            if let Err(e) = self.control(event) {
                self.window.set_status_text(&e.to_string())?;
            }
        }
        self.sync()?;
        if !self.busy() {
            match self.capture() {
                Ok(true) => self.controls()?,
                Err(e) => {
                    self.window.set_status_text(&e.to_string())?;
                }
                _ => (),
            }
        }
        Ok(())
    }
}
