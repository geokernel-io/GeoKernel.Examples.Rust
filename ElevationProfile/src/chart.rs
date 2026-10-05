//! Render the profile into a BMP for the SDK's rich-text results panel.
use serde_json::Value;
use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const WIDTH: usize = 640;
const HEIGHT: usize = 200;

pub struct Chart {
    folder: PathBuf,
    previous: Option<PathBuf>,
    revision: u64,
}

impl Chart {
    pub fn new() -> Result<Self> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let folder = std::env::temp_dir().join(format!("geokernel-profile-{}-{stamp}", std::process::id()));
        fs::create_dir(&folder)?;
        Ok(Self { folder, previous: None, revision: 0 })
    }

    pub fn render(&mut self, value: &Value) -> Result<String> {
        let samples = value["samples"].as_array().ok_or("Missing profile samples")?;
        let processed = value["processed"].as_u64().unwrap_or(0) as usize;
        let distance = value["horizontal"].as_f64().unwrap_or(0.0);
        let heights: Vec<f64> = samples.iter().take(processed)
            .filter_map(|s| s["height"].as_f64()).collect();
        if heights.is_empty() || distance <= 0.0 {
            return Ok("<p>Click two or more terrain points. No elevation samples available yet.</p>".into());
        }
        let low = heights.iter().copied().fold(f64::INFINITY, f64::min);
        let high = heights.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let padding = ((high - low) * 0.1).max(0.5);
        let (bottom, top) = (low - padding, high + padding);
        let mut pixels = vec![255u8; WIDTH * HEIGHT * 3];
        for i in 0..=4 {
            let y = i * (HEIGHT - 1) / 4;
            for x in 0..WIDTH { pixel(&mut pixels, x as i32, y as i32, [220, 220, 220]); }
            let x = i * (WIDTH - 1) / 4;
            for y in 0..HEIGHT { pixel(&mut pixels, x as i32, y as i32, [220, 220, 220]); }
        }
        let mut previous = None;
        for sample in samples.iter().take(processed) {
            let Some(height) = sample["height"].as_f64() else { previous = None; continue; };
            let x = (sample["distance"].as_f64().unwrap_or(0.0) / distance * (WIDTH - 1) as f64).round() as i32;
            let y = ((top - height) / (top - bottom) * (HEIGHT - 1) as f64).round() as i32;
            if let Some((px, py)) = previous { line(&mut pixels, px, py, x, y); }
            else { pixel(&mut pixels, x, y, [70, 130, 180]); }
            previous = Some((x, y));
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
        Ok(format!("<p>Elevation (m; vertical datum unverified)</p><table><tr><td>{top:.1}<br><br><br><br><br>{bottom:.1}</td><td><img src=\"{source}\" width=\"640\" height=\"200\"></td></tr></table><p>Horizontal distance (m): 0 — {:.1} — {:.1} — {:.1} — {distance:.1}</p>", distance / 4.0, distance / 2.0, distance * 0.75))
    }
}

fn pixel(pixels: &mut [u8], x: i32, y: i32, rgb: [u8; 3]) {
    if x < 0 || y < 0 || x >= WIDTH as i32 || y >= HEIGHT as i32 { return; }
    let offset = (y as usize * WIDTH + x as usize) * 3;
    pixels[offset..offset + 3].copy_from_slice(&[rgb[2], rgb[1], rgb[0]]);
}

fn line(pixels: &mut [u8], mut x: i32, mut y: i32, end_x: i32, end_y: i32) {
    let dx = (end_x - x).abs();
    let dy = -(end_y - y).abs();
    let sx = if x < end_x { 1 } else { -1 };
    let sy = if y < end_y { 1 } else { -1 };
    let mut error = dx + dy;
    loop {
        pixel(pixels, x, y, [70, 130, 180]);
        pixel(pixels, x, y + 1, [70, 130, 180]);
        if x == end_x && y == end_y { break; }
        let twice = error * 2;
        if twice >= dy { error += dy; x += sx; }
        if twice <= dx { error += dx; y += sy; }
    }
}

impl Drop for Chart {
    fn drop(&mut self) {
        if let Some(path) = &self.previous { let _ = fs::remove_file(path); }
        let _ = fs::remove_dir(&self.folder);
    }
}
