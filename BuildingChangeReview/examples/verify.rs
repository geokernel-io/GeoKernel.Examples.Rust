//! Opt-in native/model verification, separate from the interactive executable.
use building_change_review::{core, gdal::Gdal, ui::App};
use geokernel::ViewerTool;
use geokernel_geoai_examples::{runtime, Result};
use serde_json::{json, Value};
use std::{
    fs, thread,
    time::{Duration, Instant},
};

fn main() -> Result<()> {
    let runtime = runtime::load()?;
    {
        let out = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/validation");
        fs::create_dir_all(&out)?;
        let blocked = out.join("blocked-cache-file");
        fs::write(&blocked, "Injected preparation failure")?;
        let original_cache = std::env::var_os("GEOKERNEL_EXAMPLES_DATA_DIR");
        std::env::set_var("GEOKERNEL_EXAMPLES_DATA_DIR", &blocked);
        let mut app = App::new(&runtime)?;
        let deadline = Instant::now() + Duration::from_secs(180);
        while app.busy() && Instant::now() < deadline {
            app.tick()?;
            thread::sleep(Duration::from_millis(16));
        }
        assert!(!app.is_ready());
        if let Some(cache) = original_cache {
            std::env::set_var("GEOKERNEL_EXAMPLES_DATA_DIR", cache);
        } else {
            std::env::remove_var("GEOKERNEL_EXAMPLES_DATA_DIR");
        }
        app.run()?; // Retry preparation using the same Run action.
        while app.busy() && Instant::now() < deadline {
            app.tick()?;
            thread::sleep(Duration::from_millis(16));
        }
        assert!(app.is_ready());
        let bin = std::path::PathBuf::from(
            std::env::var_os("GEOKERNEL_BIN").ok_or("Runtime path missing")?,
        );
        let g = Gdal::load(&bin)?;
        g.initialize();
        let (tx, _rx) = std::sync::mpsc::channel();
        let input = core::prepare(&std::sync::atomic::AtomicBool::new(false), &tx)?;
        let previews = core::Workspace::new("geokernel-change-verify-")?;
        for side in 0..2 {
            assert_eq!(core::file_hash(&input.paths[side])?, core::HASHES[side]);
            let file = previews.0.join(format!("preview-{side}.tif"));
            g.display_raster(&input.paths[side], &file)?;
            let ds = g.open(&file)?;
            let grid = g.metadata(ds.handle)?;
            assert!(grid.width > 1000 && grid.height > 1000);
            for band in 1..=3 {
                let pixels = g.read_float(ds.handle, band)?;
                let mean = pixels.iter().map(|v| *v as f64).sum::<f64>() / pixels.len() as f64;
                let variance = pixels
                    .iter()
                    .map(|v| (*v as f64 - mean).powi(2))
                    .sum::<f64>()
                    / pixels.len() as f64;
                assert!(variance.sqrt() > 10.);
            }
            let viewer = app.window.pane(side)?;
            let e = viewer.layer_projected_extent(0)?;
            let view = viewer.view_extent()?;
            assert!(e.x_min > 5. && e.x_max < 6. && e.y_min > 52. && e.y_max < 53.);
            assert!(
                e.x_min < view.x_max
                    && e.x_max > view.x_min
                    && e.y_min < view.y_max
                    && e.y_max > view.y_min
            );
            println!(
                "PASS: {} RGB display {}x{}, WGS84 layer and unchanged source hash",
                if side == 0 { 2016 } else { 2025 },
                grid.width,
                grid.height
            );
        }
        println!("PASS: preparation failure and successful retry");
        app.run()?;
        while app.busy() && Instant::now() < deadline {
            app.tick()?;
            thread::sleep(Duration::from_millis(16));
        }
        if app.result.is_none() {
            return Err("Real inference did not publish".into());
        }
        let selection: Value = serde_json::from_str(
            &app.window
                .pane(1)?
                .get_layer_info_by_name_json("Selection")?,
        )?;
        assert_ne!(selection["isValid"], true);
        if app.capture()? {
            return Err("Geometry changed on publication".into());
        }
        fs::write(
            out.join("result.json"),
            serde_json::to_vec_pretty(app.result.as_ref().unwrap())?,
        )?;
        let layers = [
            app.window.pane(0)?.get_layers_info_json()?,
            app.window.pane(1)?.get_layers_info_json()?,
        ];
        let mut shape_ids = std::collections::HashSet::new();
        let count = app.result.as_ref().unwrap()["features"]
            .as_array()
            .unwrap()
            .len();
        for i in 1..=80 {
            app.select(i % count)?;
            for side in 0..2 {
                let mut viewer = app.window.pane(side)?;
                assert_eq!(viewer.get_layers_info_json()?, layers[side]);
                let selected: Value = serde_json::from_str(&viewer.get_selected_features_json()?)?;
                assert_eq!(selected.as_array().unwrap().len(), 1);
                let target: Value = serde_json::from_str(
                    &viewer.get_layer_info_by_name_json("Selection targets")?,
                )?;
                assert_eq!(selected[0]["layerIndex"], target["index"]);
                if side == 1 {
                    shape_ids.insert(selected[0]["shapeId"].as_i64().unwrap());
                }
            }
        }
        assert!(shape_ids.len() > 20);
        println!(
            "PASS: 80 selections preserve native layer identities and use selection highlights"
        );
        if app.export(&out.join("pending.geojson")).is_ok() {
            return Err("Pending geometry was exported".into());
        }
        app.review("approved")?;
        app.export(&out.join("approved.geojson"))?;
        assert_eq!(app.window.pane(0)?.get_layers_info_json()?, layers[0]);
        app.use_tool(ViewerTool::EditVertices)?;
        let index: Value = serde_json::from_str(
            &app.window
                .pane(1)?
                .get_layer_info_by_name_json("Selection")?,
        )?;
        let index = index["index"].as_i64().ok_or("Missing edit layer")? as i32;
        assert!(app.window.pane(1)?.is_layer_editing(index)?);
        assert!(!app.capture()?);
        let selected = app.selected.unwrap();
        let ring =
            &app.result.as_ref().unwrap()["features"][selected]["geometry"]["coordinates"][0];
        let x = (ring[0][0].as_f64().unwrap() + ring[1][0].as_f64().unwrap()) / 2.;
        let y = (ring[0][1].as_f64().unwrap() + ring[1][1].as_f64().unwrap()) / 2.;
        assert!(app
            .window
            .pane(1)?
            .insert_selected_feature_vertex_in_edit_layer(0, 1, x, y)?);
        assert!(app.capture()?);
        assert_eq!(
            app.result.as_ref().unwrap()["features"][selected]["properties"]["review_status"],
            "pending"
        );
        assert!(app.export(&out.join("edited-pending.geojson")).is_err());
        assert!(app.window.pane(1)?.undo_edit_layer(index)?);
        assert!(app.capture()?);
        assert!(app.window.pane(1)?.redo_edit_layer(index)?);
        assert!(app.capture()?);
        let edited = app.result.as_ref().unwrap()["features"][selected]["geometry"].clone();
        app.select((selected + 1) % count)?;
        assert_eq!(
            app.result.as_ref().unwrap()["features"][selected]["geometry"],
            edited
        );
        app.select(selected)?;
        app.review("approved")?;
        app.export(&out.join("edited-approved.geojson"))?;
        println!("PASS: lazy editing, real vertex insert, approval reset, undo/redo, selection after editing and export");
        let mut feature = app.result.as_ref().unwrap()["features"][0].clone();
        let mut geometry = feature["geometry"].clone();
        let ring = geometry["coordinates"][0]
            .as_array_mut()
            .ok_or("Missing ring")?;
        ring[0][0] = json!(ring[0][0].as_f64().ok_or("Invalid X")? + 0.0000001);
        let last = ring.len() - 1;
        ring[last] = ring[0].clone();
        core::update(&mut feature, geometry, &g)?;
        if feature["properties"]["review_status"] != "pending" {
            return Err("Approval was not invalidated".into());
        }
        println!("PASSED Rust inference, dual-view publication, approval/export gating and edit invalidation: {} candidates",app.result.as_ref().unwrap()["features"].as_array().unwrap().len());
        app.begin_close();
        while !app.closed {
            app.tick()?;
            thread::sleep(Duration::from_millis(16));
        }
    }
    unsafe {
        runtime.shutdown_viewer_application()?;
    }
    Ok(())
}
