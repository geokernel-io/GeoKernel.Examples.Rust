//! The central area of the native window: two pane headings, two native Viewer3D hosts and
//! a GDI-drawn profile chart, all Win32 child windows of the window's viewer surface.
use crate::gdal::Grids;
use libloading::Library;
use std::{
    ffi::{c_char, c_void, CStr, CString},
    marker::PhantomData,
    path::Path,
    rc::Rc,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Hwnd = *mut c_void;

#[repr(C)]
#[derive(Default, Clone, Copy, PartialEq)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}
#[repr(C)]
struct Point {
    x: i32,
    y: i32,
}
#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_ppm: i32,
    y_ppm: i32,
    used: u32,
    important: u32,
}
#[repr(C)]
struct BitmapInfo {
    header: BitmapInfoHeader,
    colors: [u32; 1],
}

#[link(name = "user32")]
extern "system" {
    fn CreateWindowExW(
        ex: u32,
        class: *const u16,
        name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        parent: Hwnd,
        menu: *mut c_void,
        instance: *mut c_void,
        param: *mut c_void,
    ) -> Hwnd;
    fn DestroyWindow(hwnd: Hwnd) -> i32;
    fn MoveWindow(hwnd: Hwnd, x: i32, y: i32, w: i32, h: i32, repaint: i32) -> i32;
    fn SetWindowTextW(hwnd: Hwnd, text: *const u16) -> i32;
    fn SendMessageW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    fn GetClientRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn ShowWindow(hwnd: Hwnd, command: i32) -> i32;
    fn GetDpiForWindow(hwnd: Hwnd) -> u32;
    fn FillRect(dc: *mut c_void, rect: *const Rect, brush: *mut c_void) -> i32;
}
#[link(name = "gdi32")]
extern "system" {
    fn CreateCompatibleDC(dc: *mut c_void) -> *mut c_void;
    fn DeleteDC(dc: *mut c_void) -> i32;
    fn CreateDIBSection(
        dc: *mut c_void,
        info: *const BitmapInfo,
        usage: u32,
        bits: *mut *mut c_void,
        section: *mut c_void,
        offset: u32,
    ) -> *mut c_void;
    fn SelectObject(dc: *mut c_void, object: *mut c_void) -> *mut c_void;
    fn DeleteObject(object: *mut c_void) -> i32;
    fn CreatePen(style: i32, width: i32, color: u32) -> *mut c_void;
    fn CreateSolidBrush(color: u32) -> *mut c_void;
    fn Polyline(dc: *mut c_void, points: *const Point, count: i32) -> i32;
    fn TextOutW(dc: *mut c_void, x: i32, y: i32, text: *const u16, count: i32) -> i32;
    fn SetTextColor(dc: *mut c_void, color: u32) -> u32;
    fn SetBkMode(dc: *mut c_void, mode: i32) -> i32;
    fn SetTextAlign(dc: *mut c_void, align: u32) -> u32;
    fn CreateFontW(
        h: i32,
        w: i32,
        e: i32,
        o: i32,
        weight: i32,
        i: u32,
        u: u32,
        s: u32,
        charset: u32,
        op: u32,
        cp: u32,
        q: u32,
        pf: u32,
        face: *const u16,
    ) -> *mut c_void;
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}
/// "#rrggbb" to a GDI COLORREF (0x00bbggrr).
fn rgb(hex: &str) -> u32 {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0);
    ((v & 0xff) << 16) | (v & 0xff00) | (v >> 16)
}

const WS_CHILD_VISIBLE_CLIP: u32 = 0x4000_0000 | 0x1000_0000 | 0x0400_0000 | 0x0200_0000;
const SS_CENTERIMAGE: u32 = 0x200;
const SS_BITMAP: u32 = 0x0E;
pub const COLORS: [&str; 4] = ["#66768a", "#087e8b", "#8857bd", "#d98419"];

/// Main-thread adapter for one native Viewer3D (published GeoKernel 1.5.40 C API).
pub struct Viewer3D {
    handle: *mut c_void,
    destroy: unsafe extern "C" fn(*mut c_void),
    error: unsafe extern "C" fn() -> *const c_char,
    resize: unsafe extern "C" fn(*mut c_void, i32, i32) -> i32,
    load: unsafe extern "C" fn(*mut c_void, *const c_char, *const c_char, i32) -> i32,
    poll: unsafe extern "C" fn(*mut c_void) -> i32,
    cancel: unsafe extern "C" fn(*mut c_void),
    height: unsafe extern "C" fn(*mut c_void, f32) -> i32,
    reset: unsafe extern "C" fn(*mut c_void) -> i32,
    get_camera: unsafe extern "C" fn(*mut c_void, *mut f32) -> i32,
    move_camera: unsafe extern "C" fn(*mut c_void, *const f32, i32) -> i32,
    _library: Rc<Library>,
}

impl Viewer3D {
    /// Safety: main Qt GUI thread only; `host` must outlive this adapter.
    unsafe fn new(host: Hwnd, library: Rc<Library>) -> Result<Self> {
        let create = *library
            .get::<unsafe extern "C" fn(*mut c_void) -> *mut c_void>(b"GeoKernel3D_Create")?;
        let error: unsafe extern "C" fn() -> *const c_char =
            *library.get(b"GeoKernel3D_LastError")?;
        let mut viewer = Self {
            handle: std::ptr::null_mut(),
            destroy: *library.get(b"GeoKernel3D_Destroy")?,
            error,
            resize: *library.get(b"GeoKernel3D_Resize")?,
            load: *library.get(b"GeoKernel3D_LoadTerrainAndImagery")?,
            poll: *library.get(b"GeoKernel3D_PollLoad")?,
            cancel: *library.get(b"GeoKernel3D_CancelLoad")?,
            height: *library.get(b"GeoKernel3D_SetHeightScale")?,
            reset: *library.get(b"GeoKernel3D_ResetCamera")?,
            get_camera: *library.get(b"GeoKernel3D_GetCamera")?,
            move_camera: *library.get(b"GeoKernel3D_MoveCamera")?,
            _library: library,
        };
        let handle = create(host);
        if handle.is_null() {
            return Err(viewer.message().into());
        }
        viewer.handle = handle;
        Ok(viewer)
    }
    fn message(&self) -> String {
        let p = unsafe { (self.error)() };
        if p.is_null() {
            "Viewer3D failed".into()
        } else {
            unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
        }
    }
    fn check(&self, code: i32) -> Result<i32> {
        if code < 0 {
            Err(self.message().into())
        } else {
            Ok(code)
        }
    }
    pub fn load(&self, dem: &Path, image: &Path, resolution: i32) -> Result<()> {
        let (dem, image) = (
            CString::new(dem.to_str().ok_or("Path is not UTF-8")?)?,
            CString::new(image.to_str().ok_or("Path is not UTF-8")?)?,
        );
        self.check(unsafe { (self.load)(self.handle, dem.as_ptr(), image.as_ptr(), resolution) })
            .map(drop)
    }
    pub fn poll(&self) -> Result<i32> {
        let code = unsafe { (self.poll)(self.handle) };
        if code == -2 {
            Ok(code)
        } else {
            self.check(code)
        }
    }
    pub fn cancel(&self) {
        unsafe { (self.cancel)(self.handle) }
    }
    pub fn height(&self, value: f32) -> Result<()> {
        self.check(unsafe { (self.height)(self.handle, value) })
            .map(drop)
    }
    pub fn reset(&self) -> Result<()> {
        self.check(unsafe { (self.reset)(self.handle) }).map(drop)
    }
    pub fn camera(&self) -> Result<[f32; 6]> {
        let mut values = [0f32; 6];
        self.check(unsafe { (self.get_camera)(self.handle, values.as_mut_ptr()) })?;
        Ok(values)
    }
    pub fn move_camera(&self, values: &[f32; 6]) -> Result<()> {
        self.check(unsafe { (self.move_camera)(self.handle, values.as_ptr(), 0) })
            .map(drop)
    }
}
impl Drop for Viewer3D {
    fn drop(&mut self) {
        // Native destruction cancels and joins any CPU loader before releasing Vulkan.
        if !self.handle.is_null() {
            unsafe { (self.destroy)(self.handle) };
        }
    }
}

pub struct Panes {
    parent: Hwnd,
    headings: [Hwnd; 2],
    hosts: [Hwnd; 2],
    chart: Hwnd,
    font: *mut c_void,
    bitmap: *mut c_void,
    rect: Rect,
    pub viewers: Vec<Viewer3D>,
    pub chart_visible: bool,
    chart_state: Option<(usize, i32, i32)>,
    _main_thread: PhantomData<Rc<()>>,
}

impl Panes {
    /// Safety: main Qt GUI thread; `parent` (the window's viewer surface) outlives the panes.
    pub unsafe fn new(parent: usize, library: &Path) -> Result<Self> {
        let parent = parent as Hwnd;
        let library = Rc::new(Library::new(library)?);
        let class = wide("STATIC");
        let child = |text: &str, style: u32| {
            let text = wide(text);
            CreateWindowExW(
                0,
                class.as_ptr(),
                text.as_ptr(),
                WS_CHILD_VISIBLE_CLIP | style,
                0,
                0,
                1,
                1,
                parent,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        let headings = [
            child("ORIGINAL DSM · roofs + trees + ground", SS_CENTERIMAGE),
            child("CLASSICAL DTM · prepared terrain estimate", SS_CENTERIMAGE),
        ];
        let hosts = [child("", 0), child("", 0)];
        let chart = child("", SS_BITMAP);
        if headings
            .iter()
            .chain(&hosts)
            .chain([&chart])
            .any(|h| h.is_null())
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let scale = GetDpiForWindow(parent).max(96) as i32;
        let face = wide("Segoe UI");
        let font = CreateFontW(
            -(9 * scale / 72),
            0,
            0,
            0,
            600,
            0,
            0,
            0,
            1,
            0,
            0,
            5,
            0,
            face.as_ptr(),
        );
        for heading in headings {
            SendMessageW(heading, 0x30, font as usize, 1); // WM_SETFONT
        }
        let mut panes = Self {
            parent,
            headings,
            hosts,
            chart,
            font,
            bitmap: std::ptr::null_mut(),
            rect: Rect::default(),
            viewers: Vec::new(),
            chart_visible: true,
            chart_state: None,
            _main_thread: PhantomData,
        };
        for host in hosts {
            panes.viewers.push(Viewer3D::new(host, library.clone())?);
        }
        Ok(panes)
    }

    fn scale(&self) -> f64 {
        f64::from(unsafe { GetDpiForWindow(self.parent) }.max(96)) / 96.0
    }

    /// Lay out headings, viewers and chart when the viewer surface size changes.
    pub fn layout(&mut self, grids: Option<&Grids>, selected: usize) -> Result<()> {
        let mut rect = Rect::default();
        if unsafe { GetClientRect(self.parent, &mut rect) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let s = self.scale();
        let px = |v: f64| (v * s).round() as i32;
        let (w, h) = (rect.right.max(2), rect.bottom.max(2));
        let chart_h = if self.chart_visible { px(200.0) } else { 0 };
        let (gap, title) = (px(8.0), px(34.0));
        let half = (w - gap) / 2;
        let view_h = (h - chart_h - title - if self.chart_visible { gap } else { 0 }).max(1);
        if rect != self.rect {
            unsafe {
                for i in 0..2 {
                    let x = i as i32 * (half + gap);
                    MoveWindow(self.headings[i], x, 0, half, title, 1);
                    MoveWindow(self.hosts[i], x, title, half, view_h, 1);
                    self.viewers[i].check((self.viewers[i].resize)(
                        self.viewers[i].handle,
                        half.max(1),
                        view_h,
                    ))?;
                }
                MoveWindow(self.chart, 0, h - chart_h, w, chart_h.max(1), 1);
                ShowWindow(self.chart, if self.chart_visible { 5 } else { 0 });
            }
            self.rect = rect;
        }
        if self.chart_visible && self.chart_state != Some((selected, w, chart_h)) {
            if let Some(grids) = grids {
                self.draw_chart(grids, selected, w, chart_h);
                self.chart_state = Some((selected, w, chart_h));
            }
        }
        Ok(())
    }

    pub fn set_chart_visible(&mut self, visible: bool) {
        self.chart_visible = visible;
        self.rect = Rect::default();
        self.chart_state = None;
    }

    pub fn set_heading(&self, index: usize, text: &str) {
        let text = wide(text);
        unsafe { SetWindowTextW(self.headings[index], text.as_ptr()) };
    }

    /// Render the profile with GDI into a DIB and show it in the SS_BITMAP child.
    fn draw_chart(&mut self, grids: &Grids, selected: usize, width: i32, height: i32) {
        let s = self.scale();
        let px = |v: f64| (v * s).round() as i32;
        unsafe {
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            let info = BitmapInfo {
                header: BitmapInfoHeader {
                    size: 40,
                    width,
                    height: -height,
                    planes: 1,
                    bit_count: 32,
                    compression: 0,
                    size_image: 0,
                    x_ppm: 0,
                    y_ppm: 0,
                    used: 0,
                    important: 0,
                },
                colors: [0],
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(dc, &info, 0, &mut bits, std::ptr::null_mut(), 0);
            let previous = SelectObject(dc, bitmap);
            let white = CreateSolidBrush(0x00ff_ffff);
            FillRect(
                dc,
                &Rect {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                },
                white,
            );
            DeleteObject(white);
            let face = wide("Segoe UI");
            let font = CreateFontW(
                -px(12.0),
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                face.as_ptr(),
            );
            let old_font = SelectObject(dc, font);
            SetBkMode(dc, 1);
            SetTextColor(dc, rgb("#476174"));
            let text = |x: i32, y: i32, align: u32, value: &str| {
                let value: Vec<u16> = value.encode_utf16().collect();
                SetTextAlign(dc, align);
                TextOutW(dc, x, y, value.as_ptr(), value.len() as i32);
            };
            text(
                px(8.0),
                px(6.0),
                0,
                "Fixed building transect · original pixel heights (LN02 metres)",
            );
            let rows = grids.profile();
            let low = rows
                .iter()
                .flat_map(|r| r.iter())
                .fold(f64::MAX, |a, v| a.min(f64::from(*v)))
                - 2.0;
            let high = rows
                .iter()
                .flat_map(|r| r.iter())
                .fold(f64::MIN, |a, v| a.max(f64::from(*v)))
                + 2.0;
            let (left, top) = (px(64.0), px(30.0));
            let (pw, ph) = ((width - px(88.0)).max(1), (height - px(64.0)).max(1));
            let bottom = top + ph;
            let grid_pen = CreatePen(0, 1, rgb("#e2e9ee"));
            for i in 0..=4 {
                let t = f64::from(i) / 4.0;
                let y = bottom - (t * f64::from(ph)).round() as i32;
                let old = SelectObject(dc, grid_pen);
                Polyline(
                    dc,
                    [Point { x: left, y }, Point { x: left + pw, y }].as_ptr(),
                    2,
                );
                SelectObject(dc, old);
                text(
                    px(56.0),
                    y - px(8.0),
                    2,
                    &format!("{:.0}", low + t * (high - low)),
                ); // TA_RIGHT
                text(
                    left + (t * f64::from(pw)).round() as i32,
                    bottom + px(5.0),
                    6,
                    &format!("{:.0} m", t * 249.5),
                ); // TA_CENTER
            }
            DeleteObject(grid_pen);
            for index in [0usize, 3, 1, 2] {
                let row = rows[index];
                let points: Vec<Point> = row
                    .iter()
                    .enumerate()
                    .map(|(x, v)| Point {
                        x: left
                            + (x as f64 / (row.len() - 1) as f64 * f64::from(pw)).round() as i32,
                        y: bottom
                            - ((f64::from(*v) - low) / (high - low) * f64::from(ph)).round() as i32,
                    })
                    .collect();
                // GDI draws dashes only at width 1; the reference stays thin and dashed.
                let pen = if index == 3 {
                    CreatePen(1, 1, rgb(COLORS[3]))
                } else {
                    CreatePen(
                        0,
                        px(if index == selected { 2.5 } else { 1.4 }).max(1),
                        rgb(COLORS[index]),
                    )
                };
                let old = SelectObject(dc, pen);
                Polyline(dc, points.as_ptr(), points.len() as i32);
                SelectObject(dc, old);
                DeleteObject(pen);
            }
            SelectObject(dc, old_font);
            DeleteObject(font);
            SelectObject(dc, previous);
            DeleteDC(dc);
            let replaced = SendMessageW(self.chart, 0x0172, 0, bitmap as isize) as *mut c_void; // STM_SETIMAGE
            if !replaced.is_null() {
                DeleteObject(replaced);
            }
            if !self.bitmap.is_null() && self.bitmap != replaced {
                DeleteObject(self.bitmap);
            }
            self.bitmap = bitmap;
        }
    }
}

impl Drop for Panes {
    fn drop(&mut self) {
        self.viewers.clear();
        unsafe {
            for hwnd in self.headings.iter().chain(&self.hosts).chain([&self.chart]) {
                DestroyWindow(*hwnd);
            }
            if !self.bitmap.is_null() {
                DeleteObject(self.bitmap);
            }
            DeleteObject(self.font);
        }
    }
}
