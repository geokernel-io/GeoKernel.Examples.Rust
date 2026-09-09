mod bootstrap;
use geokernel::ViewerWindow;
use runtime::Result;

fn run(window: &mut ViewerWindow, path: &str) -> Result<()>
{
    editing::run(window, path, "SnappingEnabled")
}

fn main() -> Result<()>
{
    bootstrap::prepare()?;
    runtime::main("SnappingEnabled", "world_4326/world_4326.shp", run)
}

mod editing
{
    use crate::runtime::{self, Result};
    use geokernel::{ControlEvent, Extent, Viewer, ViewerEvent, ViewerTool, ViewerWindow};
    use serde_json::{json, Value};
    use std::{collections::BTreeSet, sync::mpsc::Receiver};

    const POLYGON: [[f64; 2]; 7] = [
        [-119.0, 28.0],
        [-109.0, 45.0],
        [-91.0, 42.0],
        [-83.0, 30.0],
        [-99.0, 22.0],
        [-115.0, 23.5],
        [-119.0, 28.0],
    ];
    const POINTS: [[f64; 2]; 5] = [[-122.0, 36.0], [-111.0, 42.0], [-101.0, 34.5], [-91.0, 41.0], [-80.0, 33.0]];
    const GUIDE: [[f64; 2]; 6] = [[-123.0, 31.0], [-116.0, 42.0], [-106.0, 34.0], [-96.0, 43.0], [-86.0, 35.0], [-76.0, 41.0]];

    struct State {
        name: String,
        kind: i32,
        active: i32,
        serial: i32,
        step: f64,
        part: i32,
        vertex: i32,
        x: f64,
        y: f64,
        attributes: Value,
        last_status: String,
        message: String,
        saved: Option<std::path::PathBuf>,
        signals: BTreeSet<i32>,
    }

    fn require(ok: bool, message: &str) -> Result<()>
    {
        if ok {
            Ok(())
        } else {
            Err(message.to_owned().into())
        }
    }

    fn begin(viewer: &mut Viewer<'_>, index: i32) -> Result<()>
    {
        if !viewer.is_layer_editing(index)? {
            require(viewer.begin_edit_layer(index)?, "Cannot begin edit session")?;
        }
        require(viewer.set_active_edit_layer_index(index)?, "Cannot activate edit layer")
    }

    fn point(index: i32) -> [f64; 2]
    {
        let cell = index % (29 * 13);
        [-124.0 + ((cell * 7) % 29) as f64 * 1.9, 26.0 + ((cell / 29 + cell * 11) % 13) as f64 * 1.8]
    }

    fn add(window: &mut ViewerWindow, state: &mut State) -> Result<()>
    {
        let i = state.active;
        let mut viewer = window.viewer();
        if !viewer.is_layer_editing(i)? {
            return Err("Choose Begin Edit first".into());
        }
        let n = state.serial;
        let ok = match state.kind {
            3 => {
                let x = -124.0 + (n % 7) as f64 * 7.0;
                let y = 29.0 + (n / 7) as f64 * 3.0;
                viewer.add_polyline_to_edit_layer(i, &[[x, y], [x + 2.2, y + 1.4], [x + 4.8, y + 0.4], [x + 6.4, y + 2.2]])?
            }
            5 => {
                let x = -124.0 + (n % 7) as f64 * 7.5;
                let y = 27.0 + (n / 7) as f64 * 4.2;
                viewer.add_polygon_to_edit_layer(
                    i,
                    &[[x, y], [x + 4.4, y + 0.2], [x + 5.6, y + 2.4], [x + 2.3, y + 3.4], [x - 0.4, y + 2.0], [x, y]],
                )?
            }
            _ => {
                let p = if matches!(state.name.as_str(), "EditSession" | "EditDirtyState" | "EditSessionSignals") {
                    [
                        -124.0 + (n % 11) as f64 * 5.6 + (n / 66) as f64 * 0.35,
                        25.0 + ((n / 11) % 6) as f64 * 4.2 + (n / 66) as f64 * 0.35,
                    ]
                } else {
                    point(n)
                };
                let attributes = json!({"Name":format!("Site {}",n+1),"Category":if n%2==0 {"Odd"} else {"Even"},"Score":(n+1)*10,"Source":"Rust","Status":"Planned","Priority":n+1});
                viewer.add_point_to_edit_layer_with_attributes(i, p[0], p[1], &attributes.to_string())?
            }
        };
        require(ok, "Cannot add geometry")?;
        state.serial += 1;
        Ok(())
    }

    fn create_layer(viewer: &mut Viewer<'_>, name: &str, kind: i32, blue: bool) -> Result<()>
    {
        let style = json!({"pointColor":if blue {"#3984D9"} else {"#D95D39"},"pointSize":11.0,"lineColor":if kind==1 {"#8C321D"} else {"#D95D39"},"lineWidth":if kind==3 {2.6} else {2.0},"fillColor":"#F2D27A","fillOpacity":160,"selectedLineColor":"#F59E0B","selectedLineWidth":4.0,"showLabels":kind==1,"labelField":"Name","labelFontSize":10.0,"labelColor":"#263238","labelHaloEnabled":true,"labelHaloColor":"#FFFFFF","labelHaloWidth":2.0,"labelOffsetY":-12.0,"labelAllowOverlap":true});
        require(viewer.add_empty_vector_layer(name, kind, &style.to_string())? >= 0, "Cannot create edit layer")?;
        for (field, kind) in [("Name", 0), ("Category", 0), ("Score", 1), ("Source", 0), ("Status", 0), ("Priority", 1)] {
            require(viewer.add_layer_attribute_definition(0, field, kind, 64, 0)?, "Cannot create attribute field")?;
        }
        Ok(())
    }

    fn reset(window: &mut ViewerWindow, state: &mut State) -> Result<()>
    {
        let mut v = window.viewer();
        v.cancel_edit_sketch()?;
        v.clear_selected_features()?;
        for name in ["Editable", "Red Points", "Blue Points", "Editable Lines"] {
            let info: Value = serde_json::from_str(&v.get_layer_info_by_name_json(name)?)?;
            if info["isValid"] == true {
                require(v.remove_layer_by_name(name)?, "Cannot reset edit layer")?;
            }
        }
        if state.name == "MultiLayerEdit" {
            create_layer(&mut v, "Red Points", 1, false)?;
            create_layer(&mut v, "Blue Points", 1, true)?;
            begin(&mut v, 0)?;
            begin(&mut v, 1)?;
            state.active = 1;
        } else {
            if state.name == "EditVerticesTool" {
                create_layer(&mut v, "Editable Lines", 3, false)?;
                begin(&mut v, 0)?;
                require(
                    v.add_polyline_to_edit_layer(0, &[[-127.0, 31.0], [-118.0, 40.0], [-107.0, 34.0], [-96.0, 43.0], [-86.0, 37.0]])?,
                    "Cannot seed line",
                )?;
                require(
                    v.add_polyline_to_edit_layer(0, &[[-113.0, 24.0], [-101.0, 29.0], [-90.0, 27.0], [-80.0, 33.0]])?,
                    "Cannot seed line",
                )?;
                require(v.commit_edit_layer(0)?, "Cannot commit line seed")?;
                begin(&mut v, 0)?;
            }
            create_layer(&mut v, "Editable", state.kind, false)?;
            begin(&mut v, 0)?;
            state.active = 0;
            if matches!(state.name.as_str(), "EditSession" | "EditDirtyState" | "EditSessionSignals") {
                for p in [[-122.4194, 37.7749], [-118.2437, 34.0522], [-112.0740, 33.4484]] {
                    require(v.add_point_to_edit_layer(0, p[0], p[1])?, "Cannot seed session")?;
                }
            } else if matches!(
                state.name.as_str(),
                "DeleteFeature" | "MoveFeatureTool" | "MoveFeatureProgrammatic" | "SetAttributes" | "CanEditCheck"
            ) {
                for (i, p) in POINTS.iter().enumerate() {
                    require(
                        v.add_point_to_edit_layer_with_attributes(
                            0,
                            p[0],
                            p[1],
                            &json!({"Name":format!("Site {}",i+1),"Status":if i%2==0 {"Planned"} else {"Active"},"Priority":i+1}).to_string(),
                        )?,
                        "Cannot seed points",
                    )?;
                }
            } else if matches!(state.name.as_str(), "InsertVertex" | "DeleteVertex" | "EditVerticesTool") {
                require(v.add_polygon_to_edit_layer(0, &POLYGON)?, "Cannot seed polygon")?;
                if state.name == "EditVerticesTool" {
                    require(
                        v.add_polygon_to_edit_layer(0, &[[-83.0, 24.0], [-73.0, 31.0], [-65.0, 25.0], [-72.0, 18.0], [-83.0, 24.0]])?,
                        "Cannot seed polygon",
                    )?;
                }
            } else if state.name == "SnappingEnabled" {
                require(v.add_polyline_to_edit_layer(0, &GUIDE)?, "Cannot seed snapping guide")?;
            }
            require(v.commit_edit_layer(0)?, "Cannot commit seed")?;
            if !matches!(state.name.as_str(), "EditSession" | "EditDirtyState" | "EditSessionSignals" | "CanEditCheck") {
                begin(&mut v, 0)?;
            }
        }
        state.serial = 0;
        let tool = if state.name == "EditAndSave" || state.name == "UndoRedo" {
            ViewerTool::AddPoint
        } else if state.name == "AddPolylineInteractive" || state.name == "SnappingEnabled" {
            ViewerTool::AddPolyline
        } else if state.name == "AddPolygonInteractive" {
            ViewerTool::AddPolygon
        } else if matches!(state.name.as_str(), "EditVerticesTool" | "DeleteVertex") {
            ViewerTool::EditVertices
        } else {
            ViewerTool::Pan
        };
        v.use_tool(tool);
        Ok(())
    }

    fn selected(window: &mut ViewerWindow) -> Result<Value>
    {
        let values: Value = serde_json::from_str(&window.viewer().get_selected_features_json()?)?;
        values
            .as_array()
            .and_then(|a| a.first())
            .cloned()
            .ok_or("Select an editable feature first".into())
    }

    fn command(window: &mut ViewerWindow, state: &mut State, id: i32) -> Result<()>
    {
        let i = state.active;
        match id {
            1 => begin(&mut window.viewer(), i)?,
            2 => add(window, state)?,
            3 => require(window.viewer().commit_edit_layer(i)?, "Cannot commit edit")?,
            4 => require(window.viewer().rollback_edit_layer(i)?, "Cannot roll back edit")?,
            5 => reset(window, state)?,
            6 => window.viewer().full_extent()?,
            7 => window.viewer().use_tool(ViewerTool::Pan),
            8 => window.viewer().use_tool(ViewerTool::Select),
            9 => {
                begin(&mut window.viewer(), i)?;
                window.viewer().use_tool(match state.kind {
                    3 => ViewerTool::AddPolyline,
                    5 => ViewerTool::AddPolygon,
                    _ => ViewerTool::AddPoint,
                });
            }
            10 => window.viewer().use_tool(ViewerTool::MoveFeature),
            11 => window.viewer().use_tool(ViewerTool::EditVertices),
            12 => {
                let hit = selected(window)?;
                require(
                    window
                        .viewer()
                        .delete_shape_from_edit_layer(hit["layerIndex"].as_i64().unwrap_or(-1) as i32, hit["shapeId"].as_i64().unwrap_or(-1) as i32)?,
                    "Cannot delete feature",
                )?;
            }
            13 => require(
                window.viewer().delete_selected_features_from_edit_layer()?,
                "Select editable features to delete",
            )?,
            14..=17 => {
                let (x, y) = match id {
                    14 => (-state.step, 0.0),
                    15 => (state.step, 0.0),
                    16 => (0.0, state.step),
                    _ => (0.0, -state.step),
                };
                require(window.viewer().move_selected_features_in_edit_layer(x, y)?, "Select editable features to move")?;
            }
            18 => require(
                window
                    .viewer()
                    .insert_selected_feature_vertex_in_edit_layer(state.part, state.vertex, state.x, state.y)?,
                "Select a polygon and a valid insertion index",
            )?,
            19 => require(
                window.viewer().delete_selected_feature_vertex_in_edit_layer(state.part, state.vertex)?,
                "Select a polygon and a removable vertex index",
            )?,
            20 => require(window.viewer().delete_selected_vertex_from_edit_layer()?, "Select a vertex with Edit Vertices")?,
            21..=24 => {
                for _ in 0..if id >= 23 { 5 } else { 1 } {
                    let ok = if id % 2 == 1 {
                        window.viewer().undo_edit_layer(i)?
                    } else {
                        window.viewer().redo_edit_layer(i)?
                    };
                    if !ok {
                        break;
                    }
                }
            }
            25 => {
                let hit = selected(window)?;
                require(
                    window.viewer().set_shape_attributes_in_edit_layer_json(
                        hit["layerIndex"].as_i64().unwrap_or(-1) as i32,
                        hit["shapeId"].as_i64().unwrap_or(-1) as i32,
                        &state.attributes.to_string(),
                    )?,
                    "Cannot update attributes",
                )?;
            }
            26 => window.viewer().clear_selected_features()?,
            27 | 28 => {
                state.active = if id == 27 { 1 } else { 0 };
                begin(&mut window.viewer(), state.active)?;
            }
            29 | 30 => {
                for index in 0..2 {
                    let mut v = window.viewer();
                    if v.is_layer_editing(index)? {
                        require(
                            if id == 29 {
                                v.commit_edit_layer(index)?
                            } else {
                                v.rollback_edit_layer(index)?
                            },
                            "Cannot finish layer edit",
                        )?;
                    }
                    begin(&mut v, index)?;
                }
                begin(&mut window.viewer(), state.active)?;
            }
            31 => {
                if window.viewer().get_layer_feature_count(i)? == 0 {
                    return Err("Add points before saving".into());
                }
                let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_nanos();
                let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("output/EditAndSave/{stamp}"));
                std::fs::create_dir_all(&dir)?;
                let path = dir.join("clicked_points.shp");
                require(
                    window.viewer().save_layer_as_shapefile(i, path.to_str().ok_or("Invalid output path")?)?,
                    "Shapefile save failed",
                )?;
                state.message = format!("Saved: {}", path.display());
                state.saved = Some(path);
            }
            _ => return Err("Unknown edit command".into()),
        }
        runtime::refresh(window)?;
        Ok(())
    }

    fn controls(name: &str) -> Vec<Value>
    {
        let actions: Vec<(i32, &str)> = match name {
            "EditSession" | "EditDirtyState" | "EditSessionSignals" => vec![(1, "Begin Edit"), (2, "Add Feature"), (3, "Commit Edit"), (4, "Rollback Edit")],
            "EditAndSave" => vec![(9, "Add Point"), (31, "Save Shapefile"), (5, "Clear Points")],
            "AddPointInteractive" | "AddPolylineInteractive" | "AddPolygonInteractive" => vec![(9, "Draw Feature"), (5, "Clear")],
            "AddPointProgrammatic" | "AddPolylineProgrammatic" | "AddPolygonProgrammatic" | "AddWithAttributes" => {
                vec![(2, "Add Feature"), (8, "Select / Inspect"), (5, "Clear")]
            }
            "DeleteFeature" => vec![(8, "Select"), (12, "Delete Feature"), (13, "Delete Selected"), (5, "Reset Points")],
            "MoveFeatureTool" => vec![(8, "Select"), (10, "Move Feature"), (5, "Reset Points")],
            "MoveFeatureProgrammatic" => vec![
                (8, "Select"),
                (14, "Move West"),
                (15, "Move East"),
                (16, "Move North"),
                (17, "Move South"),
                (5, "Reset Points"),
            ],
            "EditVerticesTool" => vec![(11, "Edit Vertices"), (20, "Delete Selected Vertex"), (5, "Reset Shapes")],
            "InsertVertex" => vec![(8, "Select Polygon"), (18, "Insert Vertex"), (5, "Reset Shape")],
            "DeleteVertex" => vec![
                (8, "Select Polygon"),
                (11, "Edit Vertices"),
                (20, "Delete Selected Vertex"),
                (19, "Delete By Index"),
                (5, "Reset Shape"),
            ],
            "UndoRedo" => vec![
                (9, "Add Point"),
                (10, "Move Feature"),
                (21, "Undo"),
                (22, "Redo"),
                (23, "Undo 5"),
                (24, "Redo 5"),
                (5, "Reset"),
            ],
            "SetAttributes" => vec![(8, "Select"), (25, "Apply Attributes"), (21, "Undo"), (22, "Redo"), (5, "Reset")],
            "SnappingEnabled" => vec![(9, "Draw Polyline"), (11, "Edit Vertices"), (5, "Reset Guide")],
            "MultiLayerEdit" => vec![
                (27, "Active: Red Points"),
                (28, "Active: Blue Points"),
                (2, "Add To Active Layer"),
                (29, "Commit Both"),
                (30, "Rollback Both"),
                (5, "Reset"),
            ],
            _ => vec![
                (1, "Begin Edit"),
                (3, "Commit Edit"),
                (4, "Rollback Edit"),
                (8, "Select"),
                (26, "Clear Selection"),
                (5, "Reset Points"),
            ],
        };
        let mut result: Vec<Value> = actions
            .into_iter()
            .chain([(7, "Pan"), (6, "Full Extent")])
            .map(|(id, text)| json!({"id":id,"type":"button","text":text}))
            .collect();
        if name == "MoveFeatureProgrammatic" {
            result.push(json!({"id":101,"type":"number","label":"Step (map units)","minimum":0.1,"maximum":20,"value":1.0}));
        }
        if matches!(name, "InsertVertex" | "DeleteVertex") {
            result.extend([
                json!({"id":102,"type":"number","label":"Part index","minimum":0,"maximum":0,"decimals":0,"value":0}),
                json!({"id":103,"type":"number","label":"Vertex index","minimum":0,"maximum":100,"decimals":0,"value":2}),
            ]);
            if name == "InsertVertex" {
                result.extend([
                    json!({"id":104,"type":"number","label":"New vertex X","minimum":-180,"maximum":180,"value":-100.66}),
                    json!({"id":105,"type":"number","label":"New vertex Y","minimum":-90,"maximum":90,"value":47.46}),
                ]);
            }
        }
        if name == "SetAttributes" {
            result.extend([
                json!({"id":106,"type":"text","label":"Name","value":"Updated site"}),
                json!({"id":107,"type":"combo","label":"Status","options":["Planned","Active","Closed"],"value":"Active"}),
                json!({"id":108,"type":"number","label":"Priority","minimum":1,"maximum":10,"decimals":0,"value":1}),
            ]);
        }
        if name == "SnappingEnabled" {
            result.extend([
                json!({"id":109,"type":"combo","label":"Snapping","options":["On","Off"],"value":"On"}),
                json!({"id":110,"type":"number","label":"Tolerance (pixels)","minimum":1,"maximum":64,"decimals":0,"value":14}),
            ]);
        }
        result
    }

    fn event(window: &mut ViewerWindow, state: &mut State, event: ControlEvent) -> Result<()>
    {
        match event.id {
            101 => state.step = event.number,
            102 => state.part = event.number as i32,
            103 => state.vertex = event.number as i32,
            104 => state.x = event.number,
            105 => state.y = event.number,
            106 => state.attributes["Name"] = json!(event.text),
            107 => state.attributes["Status"] = json!(event.text),
            108 => state.attributes["Priority"] = json!(event.number as i32),
            109 => window.viewer().set_edit_snapping_enabled(event.text == "On")?,
            110 => window.viewer().set_edit_snapping_tolerance_pixels(event.number)?,
            id => command(window, state, id)?,
        }
        Ok(())
    }

    fn update(window: &mut ViewerWindow, state: &mut State, events: &Receiver<ViewerEvent>) -> Result<()>
    {
        for ev in events.try_iter() {
            if (6..=9).contains(&ev.data.event_type) {
                state.signals.insert(ev.data.event_type);
                if state.name == "EditSessionSignals" {
                    let label = match ev.data.event_type {
                        7 => "layerEditSessionStarted",
                        8 => "layerEditSessionCommitted",
                        9 => "layerEditSessionRolledBack",
                        _ => "layerEditStateChanged",
                    };
                    window.append_log(&format!("{label} | layer {}", ev.data.int_value))?;
                }
            }
        }
        let mut v = window.viewer();
        let i = state.active;
        let editing = v.is_layer_editing(i)?;
        let dirty = v.is_layer_dirty(i)?;
        let count = v.get_layer_feature_count(i)?;
        let selection = v.get_selected_feature_count()?;
        let undo = v.can_undo_edit_layer(i)?;
        let redo = v.can_redo_edit_layer(i)?;
        let mut status = format!("Layer {i} | Editing: {editing} | Dirty: {dirty} | Features: {count} | Selected: {selection} | Undo: {undo} | Redo: {redo}");
        if state.name == "CanEditCheck" {
            status += &format!(
                " | CanEditLayer: {} | CanEditSelected: {} | CanMoveSelected: {}",
                v.can_edit_layer(i)?,
                v.can_edit_selected_features()?,
                v.can_move_selected_features()?
            );
        }
        if state.name == "MultiLayerEdit" {
            status += &format!(" | Red: {} | Blue: {}", v.get_layer_feature_count(1)?, v.get_layer_feature_count(0)?);
        }
        if selection > 0 {
            let hits: Value = serde_json::from_str(&v.get_selected_features_json()?)?;
            status += &format!(" | {}", hits[0]["attributes"]);
        }
        if !state.message.is_empty() {
            status += &format!(" | {}", state.message);
        }
        if status == state.last_status {
            return Ok(());
        }
        if matches!(state.name.as_str(), "AddWithAttributes" | "SetAttributes" | "CanEditCheck") {
            window.append_log(&status.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;"))?;
        }
        window.set_status_text(&status)?;
        state.last_status = status;
        if editing {
            window
                .viewer()
                .set_new_feature_attributes_json(&json!({"Name":format!("Site {}",count+1),"Status":"Planned","Priority":count+1}).to_string())?;
        }
        let available_controls = controls(&state.name);
        for (id, enabled) in [
            (1, !editing),
            (2, editing),
            (3, editing),
            (4, editing),
            (21, undo),
            (22, redo),
            (23, undo),
            (24, redo),
        ] {
            if available_controls.iter().any(|c| c["id"] == id) {
                window.set_control_enabled(id, enabled)?;
            }
        }
        Ok(())
    }

    pub fn run(window: &mut ViewerWindow, path: &str, name: &str) -> Result<()>
    {
        runtime::show(window)?;
        runtime::load(
            &mut window.viewer(),
            path,
            "World",
            &json!({"fillColor":"#D8E5E1","fillOpacity":210,"lineColor":"#6F8883","lineWidth":0.7}),
        )?;
        let kind = if name.contains("Polyline") || name == "SnappingEnabled" {
            3
        } else if name.contains("Polygon") || matches!(name, "InsertVertex" | "DeleteVertex" | "EditVerticesTool") {
            5
        } else {
            1
        };
        let mut state = State {
            name: name.into(),
            kind,
            active: 0,
            serial: 0,
            step: 1.0,
            part: 0,
            vertex: 2,
            x: -100.66,
            y: 47.46,
            attributes: json!({"Name":"Updated site","Status":"Active","Priority":1}),
            last_status: String::new(),
            message: String::new(),
            saved: None,
            signals: BTreeSet::new(),
        };
        let events = window.subscribe_events();
        if name == "EditSessionSignals" {
            window.add_log_panel("Edit session events")?;
        } else if matches!(name, "AddWithAttributes" | "SetAttributes" | "CanEditCheck") {
            window.add_log_panel("State and selected attributes")?;
        }
        reset(window, &mut state)?;
        let rx = window.add_control_panel(&json!({"title":name,"area":"left","width":245,"controls":controls(name)}).to_string())?;
        if name == "SnappingEnabled" {
            window.viewer().set_edit_snapping_enabled(true)?;
            window.viewer().set_edit_snapping_tolerance_pixels(14.0)?;
        }
        window.process_events();
        let extent = if matches!(name, "InsertVertex" | "DeleteVertex" | "EditVerticesTool") {
            Extent {
                x_min: -132.0,
                y_min: 15.0,
                x_max: -55.0,
                y_max: 55.0,
            }
        } else if matches!(name, "UndoRedo" | "SetAttributes" | "SnappingEnabled") {
            Extent {
                x_min: -132.0,
                y_min: 18.0,
                x_max: -60.0,
                y_max: 55.0,
            }
        } else {
            Extent {
                x_min: -130.0,
                y_min: 20.0,
                x_max: -65.0,
                y_max: if matches!(
                    name,
                    "AddWithAttributes" | "DeleteFeature" | "MoveFeatureTool" | "MoveFeatureProgrammatic" | "MultiLayerEdit" | "CanEditCheck"
                ) {
                    55.0
                } else {
                    52.0
                },
            }
        };
        if name == "AddPointInteractive" {
            window.viewer().full_extent()?;
        } else {
            window.viewer().set_view_extent(extent)?;
        }
        update(window, &mut state, &events)?;
        state.signals.clear();
        while window.is_visible()? {
            window.process_events();
            for ev in rx.try_iter() {
                if let Err(error) = event(window, &mut state, ev) {
                    state.message = error.to_string();
                }
            }
            update(window, &mut state, &events)?;

        }

        Ok(())
    }
}

mod runtime
{
    use geokernel::{Runtime, Viewer, ViewerWindow};
    use serde_json::Value;
    use std::{
        error::Error,
        path::PathBuf,
    };
    pub type Result<T> = std::result::Result<T, Box<dyn Error>>;

    pub fn main(name: &str, default_data: &str, run: fn(&mut ViewerWindow, &str) -> Result<()>) -> Result<()>
    {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data").join(default_data);

        let path = if default_data.is_empty() {
            String::new()
        } else {
            path.canonicalize()?.to_str().ok_or("Invalid data path")?.to_owned()
        };
        // This standalone process owns the main thread and loads the trusted matching SDK.
        let runtime = unsafe { Runtime::from_env()? };
        let outcome = (|| {
            let mut window = unsafe {
                if matches!(name, "MultiWindowSync" | "LabelCollisionOff") {
                    ViewerWindow::new_dual(&runtime, name, 1280, 760)?
                } else {
                    ViewerWindow::new(&runtime, name, 1200, 800)?
                }
            };
            run(&mut window, &path)
        })();
        let shutdown = unsafe { runtime.shutdown_viewer_application() };
        outcome?;
        shutdown?;
        Ok(())
    }

    pub fn show(window: &mut ViewerWindow) -> Result<()>
    {
        window.show()?;
        window.process_events();
        Ok(())
    }

    pub fn load(viewer: &mut Viewer<'_>, path: &str, name: &str, style: &Value) -> Result<()>
    {
        if !viewer.add_layer_file(path)? || !viewer.set_layer_name(0, name)? || !viewer.set_layer_style_json(0, &style.to_string())? {
            return Err(format!("Could not load {name}").into());
        }
        Ok(())
    }

    pub fn refresh(window: &mut ViewerWindow) -> Result<()>
    {
        window.viewer().invalidate_render_cache(true, true)?;
        window.viewer().refresh_layers()?;
        Ok(())
    }
}
