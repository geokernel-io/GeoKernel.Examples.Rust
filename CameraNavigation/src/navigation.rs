use crate::terrain::Terrain;
use serde_json::{json, Value};
use std::{fs, path::PathBuf};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Navigation {
    pub home: Option<[f32; 6]>,
    pub preset: String,
    pub name: String,
    pub smooth: bool,
    views: Vec<(String, [f32; 6])>,
    file: PathBuf,
}
impl Navigation {
    pub fn new() -> Result<Self> {
        let folder = std::env::var_os("LOCALAPPDATA").ok_or("LOCALAPPDATA is unavailable")?;
        Self::from_file(
            PathBuf::from(folder)
                .join("GeoKernel/CameraNavigation.Rust/sagrada-camera-views-v1.json"),
        )
    }
    pub fn from_file(file: PathBuf) -> Result<Self> {
        let mut views = Vec::new();
        if file.is_file() {
            let contents = fs::read_to_string(&file)?;
            if let Ok(Value::Array(entries)) = serde_json::from_str::<Value>(&contents) {
                for entry in entries {
                    let Some(name) = entry["name"].as_str().filter(|n| !n.trim().is_empty()) else {
                        continue;
                    };
                    let Ok(c) = serde_json::from_value::<[f32; 6]>(entry["camera"].clone()) else {
                        continue;
                    };
                    if views.len() < 20
                        && c.iter().all(|v| v.is_finite())
                        && (-85.0..=85.0).contains(&c[1])
                        && (0.005..=20.0).contains(&c[2])
                    {
                        views.push((name.to_owned(), c));
                    }
                }
            }
        }
        Ok(Self {
            home: None,
            preset: "Overview".into(),
            name: String::new(),
            smooth: true,
            views,
            file,
        })
    }
    pub fn action(&mut self, terrain: &Terrain, id: i32) -> Result<()> {
        let duration = if self.smooth { 1200 } else { 0 };
        let mut c = terrain.camera()?;
        match id {
            11 => {
                c = self.home.ok_or("No terrain loaded")?;
                match self.preset.as_str() {
                    "Top view" => {
                        c[0] = 0.0;
                        c[1] = 85.0;
                    }
                    "East view" => {
                        c[0] = 90.0;
                        c[1] = 35.0;
                    }
                    "West view" => {
                        c[0] = -90.0;
                        c[1] = 35.0;
                    }
                    "Close oblique" => {
                        c[1] = 30.0;
                        c[2] = (c[2] * 0.55).max(0.005);
                    }
                    _ => {}
                }
                terrain.move_camera(&c, duration)?;
            }
            13 => terrain.stop_camera()?,
            14 | 15 => {
                c[2] = (c[2] * if id == 14 { 0.7 } else { 1.0 / 0.7 }).clamp(0.005, 20.0);
                terrain.move_camera(&c, duration)?;
            }
            16 => {
                c[0] = 0.0;
                terrain.move_camera(&c, duration)?;
            }
            18..=20 => {
                let name = self.name.trim();
                if name.is_empty() {
                    return Err("Enter a saved view name.".into());
                }
                let position = self.views.iter().position(|v| v.0 == name);
                match id {
                    18 => {
                        terrain.stop_camera()?;
                        let entry = (name.to_owned(), terrain.camera()?);
                        if let Some(i) = position {
                            self.views[i] = entry;
                        } else if self.views.len() < 20 {
                            self.views.push(entry);
                        } else {
                            return Err("At most 20 views can be saved.".into());
                        }
                        self.persist()?;
                    }
                    19 => {
                        let i = position.ok_or("Saved view not found.")?;
                        terrain.move_camera(&self.views[i].1, duration)?;
                    }
                    20 => {
                        let i = position.ok_or("Saved view not found.")?;
                        self.views.remove(i);
                        self.persist()?;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn persist(&self) -> Result<()> {
        fs::create_dir_all(self.file.parent().ok_or("Invalid bookmark path")?)?;
        let entries: Vec<_> = self
            .views
            .iter()
            .map(|(name, camera)| json!({"name":name,"camera":camera}))
            .collect();
        fs::write(&self.file, serde_json::to_vec_pretty(&entries)?)?;
        Ok(())
    }
    pub fn list_html(&self) -> String {
        let names: Vec<_> = self
            .views
            .iter()
            .map(|v| {
                v.0.replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;")
            })
            .collect();
        format!("Saved views: {}<br>Enter a name to restore or delete it. Mouse or keyboard input stops transitions.", if names.is_empty() { "None".into() } else { names.join(", ") })
    }
}
