//! The central area of the native window: three pane headings, three native 2D map hosts and
//! a GDI-drawn panel with the shared colour legend and the absolute-error histogram, all Win32
//! child windows of the window's viewer surface.
use crate::accuracy::{error_color, Metric};
use crate::map2d::{Event, Library2D, Map2D};
use std::{cell::Cell, ffi::c_void, marker::PhantomData, path::Path, rc::Rc, sync::mpsc::Sender};

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
#[repr(C)]
struct WndClass {
    style: u32,
    proc_: unsafe extern "system" fn(Hwnd, u32, usize, isize) -> isize,
    cls_extra: i32,
    wnd_extra: i32,
    instance: *mut c_void,
    icon: *mut c_void,
    cursor: *mut c_void,
    background: *mut c_void,
    menu: *const u16,
    class: *const u16,
}
#[repr(C)]
struct PaintStruct {
    dc: *mut c_void,
    erase: i32,
    rect: Rect,
    restore: i32,
    update: i32,
    reserved: [u8; 32],
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
    fn GetDpiForWindow(hwnd: Hwnd) -> u32;
    fn FillRect(dc: *mut c_void, rect: *const Rect, brush: *mut c_void) -> i32;
    fn RegisterClassW(class: *const WndClass) -> u16;
    fn DefWindowProcW(hwnd: Hwnd, message: u32, wparam: usize, lparam: isize) -> isize;
    fn BeginPaint(hwnd: Hwnd, paint: *mut PaintStruct) -> *mut c_void;
    fn EndPaint(hwnd: Hwnd, paint: *const PaintStruct) -> i32;
    fn InvalidateRect(hwnd: Hwnd, rect: *const Rect, erase: i32) -> i32;
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
    fn BitBlt(
        dc: *mut c_void,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        source: *mut c_void,
        sx: i32,
        sy: i32,
        op: u32,
    ) -> i32;
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

const WS_CHILD_VISIBLE_CLIP: u32 = 0x5600_0000;
const SS_CENTERIMAGE: u32 = 0x200;
const PANEL_CLASS: &str = "GeoKernelDsmAccuracyPanel";
pub const METHOD_COLORS: [&str; 2] = ["#087e8b", "#8857bd"];
const BIN_LABELS: [&str; 6] = ["≤0.25", "0.25–0.5", "0.5–1", "1–2", "2–5", ">5 m"];

thread_local! {
    // The panel's current bitmap (GUI thread only).
    static PANEL_BITMAP: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
}

unsafe extern "system" fn panel_proc(
    hwnd: Hwnd,
    message: u32,
    wparam: usize,
    lparam: isize,
) -> isize {
    match message {
        0x0014 => 1, // WM_ERASEBKGND: the bitmap covers the window
        0x000F => {
            // WM_PAINT
            let mut paint: PaintStruct = std::mem::zeroed();
            let dc = BeginPaint(hwnd, &mut paint);
            let bitmap = PANEL_BITMAP.with(Cell::get);
            let mut rect = Rect::default();
            GetClientRect(hwnd, &mut rect);
            if bitmap.is_null() {
                let white = CreateSolidBrush(0x00ff_ffff);
                FillRect(dc, &rect, white);
                DeleteObject(white);
            } else {
                let memory = CreateCompatibleDC(dc);
                let previous = SelectObject(memory, bitmap);
                BitBlt(dc, 0, 0, rect.right, rect.bottom, memory, 0, 0, 0x00CC_0020); // SRCCOPY
                SelectObject(memory, previous);
                DeleteDC(memory);
            }
            EndPaint(hwnd, &paint);
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// Panel state the drawing depends on; redraw only when it changes.
#[derive(Clone, Copy, PartialEq)]
struct PanelKey {
    revision: u64,
    mode: usize,
    limit: u64,
    width: i32,
    height: i32,
}

pub struct Panes {
    parent: Hwnd,
    headings: [Hwnd; 3],
    hosts: [Hwnd; 3],
    panel: Hwnd,
    font: *mut c_void,
    rect: Rect,
    panel_key: Option<PanelKey>,
    pub maps: Vec<Map2D>,
    _main_thread: PhantomData<Rc<()>>,
}

impl Panes {
    /// Safety: main Qt GUI thread; `parent` (the window's viewer surface) outlives the panes.
    pub unsafe fn new(
        parent: usize,
        viewer_dll: &Path,
        events: Sender<Event>,
        titles: [&str; 3],
    ) -> Result<Self> {
        let parent = parent as Hwnd;
        let library = Library2D::new(viewer_dll)?;
        let class_name = wide(PANEL_CLASS);
        let class = WndClass {
            style: 0x0003, // CS_HREDRAW | CS_VREDRAW
            proc_: panel_proc,
            cls_extra: 0,
            wnd_extra: 0,
            instance: std::ptr::null_mut(),
            icon: std::ptr::null_mut(),
            cursor: std::ptr::null_mut(),
            background: std::ptr::null_mut(),
            menu: std::ptr::null(),
            class: class_name.as_ptr(),
        };
        RegisterClassW(&class); // a second registration in the same process fails harmlessly
        let child = |class: &str, text: &str, style: u32| {
            let (class, text) = (wide(class), wide(text));
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
        let headings = titles.map(|t| child("STATIC", t, SS_CENTERIMAGE));
        let hosts = [
            child("STATIC", "", 0),
            child("STATIC", "", 0),
            child("STATIC", "", 0),
        ];
        let panel = child(PANEL_CLASS, "", 0);
        if headings
            .iter()
            .chain(&hosts)
            .chain([&panel])
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
            panel,
            font,
            rect: Rect::default(),
            panel_key: None,
            maps: Vec::new(),
            _main_thread: PhantomData,
        };
        for (side, host) in hosts.into_iter().enumerate() {
            panes
                .maps
                .push(Map2D::new(&library, host, side, events.clone())?);
        }
        Ok(panes)
    }

    fn scale(&self) -> f64 {
        f64::from(unsafe { GetDpiForWindow(self.parent) }.max(96)) / 96.0
    }

    /// Lay out headings, maps and the legend/histogram panel when the surface size changes.
    pub fn layout(&mut self) -> Result<()> {
        let mut rect = Rect::default();
        if unsafe { GetClientRect(self.parent, &mut rect) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if rect == self.rect {
            return Ok(());
        }
        let s = self.scale();
        let px = |v: f64| (v * s).round() as i32;
        let (w, h) = (rect.right.max(3), rect.bottom.max(2));
        let (gap, title) = (px(8.0), px(34.0));
        let panel_h = px(230.0).min(h / 2);
        let third = (w - 2 * gap) / 3;
        let map_h = (h - panel_h - title - gap).max(1);
        unsafe {
            for i in 0..3 {
                let x = i as i32 * (third + gap);
                MoveWindow(self.headings[i], x, 0, third, title, 1);
                MoveWindow(self.hosts[i], x, title, third, map_h, 1);
                self.maps[i].resize(third, map_h);
            }
            MoveWindow(self.panel, 0, h - panel_h, w, panel_h, 1);
        }
        self.rect = rect;
        self.panel_key = None;
        Ok(())
    }

    pub fn set_heading(&self, index: usize, text: &str) {
        let text = wide(text);
        unsafe { SetWindowTextW(self.headings[index], text.as_ptr()) };
    }

    /// Redraw the legend and histogram into a DIB when their inputs changed and show it.
    pub fn draw_panel(
        &mut self,
        mode: usize,
        limit: f64,
        metrics: Option<&[Metric; 2]>,
        revision: u64,
    ) {
        let mut client = Rect::default();
        unsafe { GetClientRect(self.panel, &mut client) };
        let (width, height) = (client.right.max(1), client.bottom.max(1));
        let key = PanelKey {
            revision,
            mode,
            limit: limit.to_bits(),
            width,
            height,
        };
        if self.panel_key == Some(key) {
            return;
        }
        self.panel_key = Some(key);
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
            let fill = |r: Rect, color: u32| {
                let brush = CreateSolidBrush(color);
                FillRect(dc, &r, brush);
                DeleteObject(brush);
            };
            fill(
                Rect {
                    left: 0,
                    top: 0,
                    right: width,
                    bottom: height,
                },
                0x00ff_ffff,
            );
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
            // Legend: the exact colour of each column's value on the shared scale.
            let legend_h = px(62.0);
            if mode == 2 {
                text(width / 2, px(22.0), 6, "Neutral relief: same light and contrast for both terrain estimates. Switch to an error view to compare reference differences.");
            } else {
                text(
                    width / 2,
                    px(3.0),
                    6,
                    if mode == 0 {
                        "Estimated DTM − reference: blue below · white zero · red above (metres)"
                    } else {
                        "Absolute reference error: white zero · orange/red larger errors (metres)"
                    },
                );
                let (x0, x1) = (px(70.0), (width - px(70.0)).max(px(71.0)));
                for x in x0..x1 {
                    let t = f64::from(x - x0) / f64::from((x1 - x0 - 1).max(1));
                    let value = if mode == 0 {
                        (t * 2.0 - 1.0) * limit
                    } else {
                        t * limit
                    };
                    let [r, g, b] = error_color(value, mode == 1, limit);
                    fill(
                        Rect {
                            left: x,
                            top: px(25.0),
                            right: x + 1,
                            bottom: px(38.0),
                        },
                        u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16,
                    );
                }
                text(
                    px(8.0),
                    px(42.0),
                    0,
                    &if mode == 0 {
                        format!("≤ −{limit} m")
                    } else {
                        "0 m".into()
                    },
                );
                text(width - px(8.0), px(42.0), 2, &format!("≥ {limit} m"));
                text(
                    width / 2,
                    px(42.0),
                    6,
                    "One shared scale · display saturation only; numeric errors are unchanged",
                );
            }
            // Histogram of absolute errors in the evaluated area.
            if let Some(metrics) = metrics {
                let top0 = legend_h + px(6.0);
                text(
                    width / 2,
                    top0,
                    6,
                    "Absolute error distribution · evaluated area · teal classical / purple AI",
                );
                let (left, top) = (px(50.0), top0 + px(28.0));
                let (pw, ph) = ((width - px(70.0)).max(1), (height - top - px(26.0)).max(1));
                let bottom = top + ph;
                let group = f64::from(pw) / 6.0;
                let mut maximum = 10.0f64;
                for m in metrics {
                    for count in m.bins {
                        maximum = maximum.max(100.0 * count as f64 / m.count as f64);
                    }
                }
                maximum = (maximum / 10.0).ceil() * 10.0;
                for tick in 0..=2 {
                    let y = bottom - (f64::from(tick) * 0.5 * f64::from(ph)).round() as i32;
                    let pen = CreatePen(0, 1, rgb("#e2e9ee"));
                    let old = SelectObject(dc, pen);
                    Polyline(
                        dc,
                        [Point { x: left, y }, Point { x: left + pw, y }].as_ptr(),
                        2,
                    );
                    SelectObject(dc, old);
                    DeleteObject(pen);
                    text(
                        px(43.0),
                        y - px(8.0),
                        2,
                        &format!("{:.0}%", f64::from(tick) * 0.5 * maximum),
                    );
                }
                for (b, label) in BIN_LABELS.iter().enumerate() {
                    for (method, m) in metrics.iter().enumerate() {
                        let h = (100.0 * m.bins[b] as f64 / m.count as f64 / maximum
                            * f64::from(ph))
                        .round() as i32;
                        let x = left
                            + (b as f64 * group + group * (0.18 + method as f64 * 0.32)).round()
                                as i32;
                        fill(
                            Rect {
                                left: x,
                                top: bottom - h,
                                right: x + (group * 0.28).round() as i32,
                                bottom,
                            },
                            rgb(METHOD_COLORS[method]),
                        );
                    }
                    text(
                        left + (b as f64 * group + group / 2.0).round() as i32,
                        bottom + px(4.0),
                        6,
                        label,
                    );
                }
            }
            SelectObject(dc, old_font);
            DeleteObject(font);
            SelectObject(dc, previous);
            DeleteDC(dc);
            let old = PANEL_BITMAP.with(|b| b.replace(bitmap));
            if !old.is_null() {
                DeleteObject(old);
            }
            InvalidateRect(self.panel, std::ptr::null(), 0);
        }
    }
}

impl Drop for Panes {
    fn drop(&mut self) {
        self.maps.clear();
        unsafe {
            for hwnd in self.headings.iter().chain(&self.hosts).chain([&self.panel]) {
                DestroyWindow(*hwnd);
            }
            let bitmap = PANEL_BITMAP.with(|b| b.replace(std::ptr::null_mut()));
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            DeleteObject(self.font);
        }
    }
}
