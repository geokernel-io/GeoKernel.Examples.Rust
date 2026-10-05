//! Render the viewshed into a BMP for the SDK's rich-text results panel.
use serde_json::Value;
use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const WIDTH: usize = 320;
const HEIGHT: usize = 320;

pub struct VisibilityMap {
    folder: PathBuf,
    previous: Option<PathBuf>,
    revision: u64,
}

impl VisibilityMap {
    pub fn new() -> Result<Self> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let folder = std::env::temp_dir().join(format!("geokernel-viewshed-{}-{stamp}", std::process::id()));
        fs::create_dir(&folder)?;
        Ok(Self { folder, previous: None, revision: 0 })
    }

    pub fn render(&mut self, value: &Value) -> Result<String> {
        let cells = value["cells"].as_array().ok_or("Missing viewshed cells")?;
        let width = value["width"].as_u64().unwrap_or(0) as usize;
        if width == 0 || cells.is_empty() {
            return Ok("<p>Select an observer on terrain, then calculate visibility.</p>".into());
        }
        if cells.len() != width * width { return Err("Invalid viewshed grid dimensions".into()); }
        let radius = value["radius"].as_f64().unwrap_or(0.0);
        let mut pixels = vec![255u8; WIDTH * HEIGHT * 3];
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let cell = &cells[(y * width / HEIGHT) * width + x * width / WIDTH];
                let color = match cell.as_u64() {
                    Some(1) => [220, 227, 235],
                    Some(2) => [35, 155, 86],
                    Some(3) => [211, 84, 69],
                    Some(4) => [133, 133, 133],
                    _ => [255, 255, 255],
                };
                pixel(&mut pixels, x as i32, y as i32, color);
            }
        }
        for offset in -6..=6 {
            pixel(&mut pixels, WIDTH as i32 / 2 + offset, HEIGHT as i32 / 2, [0, 0, 0]);
            pixel(&mut pixels, WIDTH as i32 / 2, HEIGHT as i32 / 2 + offset, [0, 0, 0]);
        }
        // WIDTH*3 is divisible by four, so BMP scanlines require no extra padding.
        let size = 54 + pixels.len();
        let mut bmp = vec![0u8; 54];
        bmp[0..2].copy_from_slice(b"BM");
        bmp[2..6].copy_from_slice(&(size as u32).to_le_bytes());
        bmp[10..14].copy_from_slice(&54u32.to_le_bytes());
        bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&(WIDTH as i32).to_le_bytes());
        bmp[22..26].copy_from_slice(&(-(HEIGHT as i32)).to_le_bytes());
        bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
        bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
        bmp.extend_from_slice(&pixels);
        self.revision += 1;
        let path = self.folder.join(format!("{}.bmp", self.revision));
        fs::write(&path, bmp)?;
        if let Some(old) = self.previous.replace(path.clone()) { let _ = fs::remove_file(old); }
        let source = path.to_string_lossy().replace('\\', "/")
            .replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;");
        Ok(format!("<table><tr><td></td><td align=\"center\">N ↑</td><td></td></tr><tr><td>W</td><td><img src=\"{source}\" width=\"320\" height=\"320\"></td><td>E</td></tr><tr><td></td><td align=\"center\">S ↓ | Radius: {radius:.0} m</td><td></td></tr></table>"))
    }
}

fn pixel(pixels: &mut [u8], x: i32, y: i32, rgb: [u8; 3]) {
    if x < 0 || y < 0 || x >= WIDTH as i32 || y >= HEIGHT as i32 { return; }
    let offset = (y as usize * WIDTH + x as usize) * 3;
    pixels[offset..offset + 3].copy_from_slice(&[rgb[2], rgb[1], rgb[0]]);
}

impl Drop for VisibilityMap {
    fn drop(&mut self) {
        if let Some(path) = &self.previous { let _ = fs::remove_file(path); }
        let _ = fs::remove_dir(&self.folder);
    }
}
