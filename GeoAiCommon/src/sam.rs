use crate::{dialog, ControlEvent, GeoAiExample, Result, ViewerTool};
use serde_json::{json, Value};
use std::time::Instant;

pub const MODE: i32 = 20;
pub const TEXT: i32 = 21;
pub const SCORE: i32 = 22;
pub const TOOL: i32 = 23;
pub const CLEAR: i32 = 24;
pub const ACCEPT: i32 = 25;
pub const DISCARD: i32 = 26;
pub const TIFF: i32 = 27;
pub const SELECTION: i32 = 28;

#[derive(Clone)]
pub struct SamState {
    pub text_mode: bool,
    pub text: String,
    pub score: f64,
    pub tool: String,
    /// Longitude, latitude, positive (1) / negative (0).
    pub points: Vec<[f64; 3]>,
    pub bounds: Option<[f64; 4]>,
    pub corner: Option<[f64; 2]>,
    pub accepted: bool,
    layers: Vec<String>,
}
impl Default for SamState {
    fn default() -> Self {
        Self {
            text_mode: true,
            text: "building".into(),
            score: 0.5,
            tool: "Pan map".into(),
            points: vec![],
            bounds: None,
            corner: None,
            accepted: false,
            layers: vec![],
        }
    }
}
pub fn controls() -> Vec<Value> {
    vec![
        json!({"id":MODE,"type":"combo","label":"Task","options":["Text segmentation","Point / box segmentation"],"value":"Text segmentation"}),
        json!({"id":TEXT,"type":"text","label":"Text concept","value":"building"}),
        json!({"id":SCORE,"type":"number","label":"Text score threshold","minimum":0.01,"maximum":0.99,"step":0.05,"decimals":2,"value":0.5}),
        json!({"id":TOOL,"type":"combo","label":"Map tool","options":["Pan map","Positive point","Negative point","Box: click two corners"],"value":"Pan map"}),
        json!({"id":SELECTION,"type":"text","label":"Selection","value":"0 points · no box"}),
        json!({"id":CLEAR,"type":"button","text":"Clear prompts"}),
        json!({"id":ACCEPT,"type":"button","text":"Accept draft"}),
        json!({"id":DISCARD,"type":"button","text":"Discard draft"}),
        json!({"id":TIFF,"type":"button","text":"Save accepted GeoTIFF"}),
    ]
}
impl GeoAiExample {
    pub fn is_sam(&self) -> bool {
        self.profile.task.starts_with("sam3-")
    }
    pub(super) fn sam_controls(&mut self) -> Result<()> {
        let ready = !self.busy() && !self.closing;
        let editable = ready && self.result.is_none();
        let save = ready && self.result.is_some() && self.sam.accepted;
        for (id, enabled) in [
            (crate::RUN, editable),
            (crate::PROVIDER, editable),
            (MODE, editable),
            (TEXT, editable && self.sam.text_mode),
            (SCORE, editable && self.sam.text_mode),
            (TOOL, editable && !self.sam.text_mode),
            (CLEAR, editable),
            (ACCEPT, ready && self.result.is_some() && !self.sam.accepted),
            (DISCARD, ready && self.result.is_some()),
            (crate::SAVE, save),
            (TIFF, save),
        ] {
            self.window.set_control_enabled(id, enabled)?;
        }
        self.window.viewer().use_tool(
            if editable && !self.sam.text_mode && self.sam.tool != "Pan map" {
                ViewerTool::Info
            } else {
                ViewerTool::Pan
            },
        );
        Ok(())
    }
    pub(super) fn sam_valid(&mut self) -> Result<bool> {
        let error = if self.sam.text_mode && self.sam.text.trim().is_empty() {
            Some("Enter a text concept, for example building.")
        } else if !self.sam.text_mode
            && self.sam.bounds.is_none()
            && !self.sam.points.iter().any(|p| p[2] > 0.0)
        {
            Some("Add a positive point or a box around ONE object.")
        } else if self.sam.corner.is_some() {
            Some("Complete the second box corner or clear prompts.")
        } else {
            None
        };
        if let Some(error) = error {
            self.log(error)?;
            return Ok(false);
        }
        self.sam.accepted = false;
        Ok(true)
    }
    pub(super) fn sam_control(&mut self, event: &ControlEvent) -> Result<bool> {
        let editable = !self.busy() && self.result.is_none();
        match event.id {
            MODE if editable => {
                self.sam.text_mode = event.text == "Text segmentation";
                self.sam_controls()?;
            }
            TEXT if editable => self.sam.text = event.text.clone(),
            SCORE if editable => self.sam.score = event.number.clamp(0.01, 0.99),
            TOOL if editable => {
                self.sam.tool = event.text.clone();
                self.sam_controls()?;
            }
            CLEAR if editable => self.clear_prompts()?,
            ACCEPT => self.accept()?,
            DISCARD => self.discard()?,
            TIFF if self.sam.accepted && !self.busy() => {
                let owner = self.window.viewer().get_native_handle()?;
                if let Some(path) = dialog::save_format(owner, "sam3-instances.tif", "geotiff")? {
                    self.export_format(path, "geotiff")?;
                }
            }
            MODE | TEXT | SCORE | TOOL | CLEAR | TIFF => {}
            _ => return Ok(false),
        }
        Ok(true)
    }
    pub fn accept(&mut self) -> Result<()> {
        if self.is_sam() && !self.busy() && !self.closing && self.result.is_some() {
            self.sam.accepted = true;
            self.progress(100, "Draft accepted. Save explicitly to retain it.")?;
            self.controls()?;
        }
        Ok(())
    }
    pub fn discard(&mut self) -> Result<()> {
        if self.is_sam() && !self.busy() && !self.closing && self.result.is_some() {
            self.detach_result()?;
            self.result = None;
            self.sam.accepted = false;
            self.run_after_release = false;
            self.release_since = Some(Instant::now());
            self.progress(0, "Draft discarded.")?;
            self.controls()?;
        }
        Ok(())
    }
    pub fn clear_prompts(&mut self) -> Result<()> {
        if self.busy() || self.result.is_some() || self.closing {
            return Ok(());
        }
        self.sam.points.clear();
        self.sam.bounds = None;
        self.sam.corner = None;
        self.draw_prompts()
    }
    /// Coordinates are WGS84, matching the viewer's explicitly selected CRS.
    pub fn select_point(&mut self, x: f64, y: f64) -> Result<()> {
        if !self.is_sam()
            || self.sam.text_mode
            || self.busy()
            || self.closing
            || self.result.is_some()
            || !x.is_finite()
            || !y.is_finite()
        {
            return Ok(());
        }
        match self.sam.tool.as_str() {
            "Positive point" | "Negative point" => {
                if self.sam.points.len() + if self.sam.bounds.is_some() { 2 } else { 0 } >= 64 {
                    self.progress(0, "Prompt limit reached.")?;
                    return Ok(());
                }
                self.sam.points.push([
                    x,
                    y,
                    if self.sam.tool == "Positive point" {
                        1.0
                    } else {
                        0.0
                    },
                ]);
            }
            "Box: click two corners" => {
                if self.sam.points.len() > 62 {
                    self.progress(0, "A box needs two prompt tokens.")?;
                    return Ok(());
                }
                if let Some([a, b]) = self.sam.corner.take() {
                    if a == x || b == y {
                        self.progress(0, "Choose different box corners.")?;
                    } else {
                        self.sam.bounds = Some([a.min(x), b.min(y), a.max(x), b.max(y)]);
                    }
                } else {
                    self.sam.corner = Some([x, y]);
                    self.sam.bounds = None;
                }
            }
            _ => return Ok(()),
        }
        self.draw_prompts()
    }
    fn draw_prompts(&mut self) -> Result<()> {
        for name in self.sam.layers.drain(..) {
            self.window.viewer().remove_layer_by_name(&name)?;
        }
        let mut points: Vec<([f64; 2], &str)> = self
            .sam
            .points
            .iter()
            .map(|p| ([p[0], p[1]], if p[2] > 0.0 { "#20df80" } else { "#ff4848" }))
            .collect();
        if let Some(p) = self.sam.corner {
            points.push((p, "#ffe45a"));
        }
        for (point, color) in points {
            let name = format!("SAM3 prompt {}", self.sam.layers.len());
            let index = self.window.viewer().add_point_layer(
                &name,
                &[point],
                &json!({"pointColor":color,"pointSize":10}).to_string(),
            )?;
            if index < 0 {
                return Err("Cannot display prompt".into());
            }
            self.window
                .viewer()
                .set_layer_coordinate_system_preset(index, "wgs84")?;
            self.sam.layers.push(name);
        }
        if let Some([a, b, c, d]) = self.sam.bounds {
            let name = "SAM3 prompt box";
            let index=self.window.viewer().add_polygon_layer(name,&[[a,b],[c,b],[c,d],[a,d],[a,b]],&json!({"fillColor":"#ffe45a","fillOpacity":24,"lineColor":"#ffe45a","lineWidth":2}).to_string())?;
            if index < 0 {
                return Err("Cannot display box".into());
            }
            self.window
                .viewer()
                .set_layer_coordinate_system_preset(index, "wgs84")?;
            self.sam.layers.push(name.into());
        }
        self.window.set_control_value(
            SELECTION,
            0.0,
            &format!(
                "{} points · {}",
                self.sam.points.len(),
                if self.sam.bounds.is_some() {
                    "box set"
                } else if self.sam.corner.is_some() {
                    "choose second corner"
                } else {
                    "no box"
                }
            ),
        )?;
        self.window.viewer().refresh_layers()?;
        Ok(())
    }
}
