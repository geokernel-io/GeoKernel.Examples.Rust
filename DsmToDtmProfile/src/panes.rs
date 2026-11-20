//! The central area of the native window: two pane headings, two native 2D map hosts and
//! the profile chart, all Win32 child windows of the window's viewer surface. The chart is
//! a small window class of its own so it can report hover positions; it is drawn with GDI.
use crate::map2d::{Event, Library2D, Map2D};
use crate::profile::Profile;
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
#[repr(C)]
struct TrackMouse {
    size: u32,
    flags: u32,
    hwnd: Hwnd,
    hover: u32,
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
    fn TrackMouseEvent(track: *mut TrackMouse) -> i32;
    fn LoadCursorW(instance: *mut c_void, name: *const u16) -> *mut c_void;
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
pub const COLORS: [&str; 4] = ["#66768a", "#087e8b", "#8857bd", "#d98419"];
const CHART_CLASS: &str = "GeoKernelDsmProfileChart";

thread_local! {
    // The chart window's current bitmap and the last mouse position over it (GUI thread only).
    static CHART_BITMAP: Cell<*mut c_void> = const { Cell::new(std::ptr::null_mut()) };
    static HOVER: Cell<Option<(i32, i32)>> = const { Cell::new(None) };
}

unsafe extern "system" fn chart_proc(
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
            let bitmap = CHART_BITMAP.with(Cell::get);
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
        0x0200 => {
            // WM_MOUSEMOVE: remember the position and ask for WM_MOUSELEAVE
            let (x, y) = (
                (lparam & 0xffff) as i16 as i32,
                ((lparam >> 16) & 0xffff) as i16 as i32,
            );
            HOVER.with(|h| h.set(Some((x, y))));
            let mut track = TrackMouse {
                size: std::mem::size_of::<TrackMouse>() as u32,
                flags: 2,
                hwnd,
                hover: 0,
            };
            TrackMouseEvent(&mut track);
            0
        }
        0x02A3 => {
            // WM_MOUSELEAVE
            HOVER.with(|h| h.set(None));
            0
        }
        _ => DefWindowProcW(hwnd, message, wparam, lparam),
    }
}

/// Chart state the drawing depends on; redraw only when it changes.
#[derive(Clone, Copy, PartialEq)]
struct ChartKey {
    revision: u64,
    errors: bool,
    hovered: i64,
    width: i32,
    height: i32,
}

pub struct Panes {
    parent: Hwnd,
    headings: [Hwnd; 2],
    hosts: [Hwnd; 2],
    chart: Hwnd,
    font: *mut c_void,
    rect: Rect,
    chart_key: Option<ChartKey>,
    pub maps: Vec<Map2D>,
    _main_thread: PhantomData<Rc<()>>,
}

impl Panes {
    /// Safety: main Qt GUI thread; `parent` (the window's viewer surface) outlives the panes.
    pub unsafe fn new(parent: usize, viewer_dll: &Path, events: Sender<Event>) -> Result<Self> {
        let parent = parent as Hwnd;
        let library = Library2D::new(viewer_dll)?;
        let class_name = wide(CHART_CLASS);
        let class = WndClass {
            style: 0x0003, // CS_HREDRAW | CS_VREDRAW
            proc_: chart_proc,
            cls_extra: 0,
            wnd_extra: 0,
            instance: std::ptr::null_mut(),
            icon: std::ptr::null_mut(),
            cursor: LoadCursorW(std::ptr::null_mut(), 32515 as *const u16), // IDC_CROSS
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
        let headings = [
            child(
                "STATIC",
                "ORIGINAL DSM · roofs + trees + ground",
                SS_CENTERIMAGE,
            ),
            child(
                "STATIC",
                "CLASSICAL DTM · prepared estimate",
                SS_CENTERIMAGE,
            ),
        ];
        let hosts = [child("STATIC", "", 0), child("STATIC", "", 0)];
        let chart = child(CHART_CLASS, "", 0);
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
            rect: Rect::default(),
            chart_key: None,
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

    /// Lay out headings, maps and chart when the viewer surface size changes.
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
        let (w, h) = (rect.right.max(2), rect.bottom.max(2));
        let (gap, title) = (px(8.0), px(34.0));
        let chart_h = (h / 3).max(px(200.0)).min(h / 2);
        let half = (w - gap) / 2;
        let map_h = (h - chart_h - title - gap).max(1);
        unsafe {
            for i in 0..2 {
                let x = i as i32 * (half + gap);
                MoveWindow(self.headings[i], x, 0, half, title, 1);
                MoveWindow(self.hosts[i], x, title, half, map_h, 1);
                self.maps[i].resize(half, map_h);
            }
            MoveWindow(self.chart, 0, h - chart_h, w, chart_h, 1);
        }
        self.rect = rect;
        self.chart_key = None;
        Ok(())
    }

    pub fn set_heading(&self, index: usize, text: &str) {
        let text = wide(text);
        unsafe { SetWindowTextW(self.headings[index], text.as_ptr()) };
    }

    fn chart_size(&self) -> (i32, i32) {
        let mut rect = Rect::default();
        unsafe { GetClientRect(self.chart, &mut rect) };
        (rect.right.max(1), rect.bottom.max(1))
    }

    fn plot(&self, width: i32, height: i32) -> (i32, i32, i32, i32) {
        let s = self.scale();
        let px = |v: f64| (v * s).round() as i32;
        (
            px(65.0),
            px(30.0),
            (width - px(90.0)).max(1),
            (height - px(72.0)).max(1),
        )
    }

    /// The sample index under the mouse, or -1 outside the plot.
    pub fn hovered(&self, profile: Option<&Profile>) -> i64 {
        let (Some(profile), Some((x, y))) = (profile, HOVER.with(Cell::get)) else {
            return -1;
        };
        let (width, height) = self.chart_size();
        let (left, top, pw, ph) = self.plot(width, height);
        if x < left || x > left + pw || y < top || y > top + ph {
            return -1;
        }
        let count = profile.samples.len() as f64;
        ((f64::from(x - left) / f64::from(pw) * (count - 1.0)).round() as i64)
            .clamp(0, count as i64 - 1)
    }

    /// Redraw the chart into a DIB when its inputs changed and show it.
    pub fn draw_chart(
        &mut self,
        profile: Option<&Profile>,
        revision: u64,
        errors: bool,
        hovered: i64,
    ) {
        let (width, height) = self.chart_size();
        let key = ChartKey {
            revision,
            errors,
            hovered,
            width,
            height,
        };
        if self.chart_key == Some(key) {
            return;
        }
        self.chart_key = Some(key);
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
            let line = |points: &[Point], pen: *mut c_void| {
                let old = SelectObject(dc, pen);
                Polyline(dc, points.as_ptr(), points.len() as i32);
                SelectObject(dc, old);
                DeleteObject(pen);
            };
            match profile {
                None => text(
                    width / 2,
                    height / 2 - px(8.0),
                    6,
                    "Draw a transect: choose A, then B on either map.",
                ),
                Some(profile) => {
                    let bands: &[usize] = if errors { &[1, 2] } else { &[0, 1, 2, 3] };
                    let value = |sample: &crate::profile::Sample, band: usize| {
                        sample.heights[band] - if errors { sample.heights[3] } else { 0.0 }
                    };
                    let (mut low, mut high) = if errors {
                        (0.0, 0.0)
                    } else {
                        (f64::MAX, f64::MIN)
                    };
                    for sample in &profile.samples {
                        for &band in bands {
                            low = low.min(value(sample, band));
                            high = high.max(value(sample, band));
                        }
                    }
                    let pad = (0.25f64).max((high - low) * 0.12);
                    let (low, high) = (low - pad, high + pad);
                    let (left, top, pw, ph) = self.plot(width, height);
                    let bottom = top + ph;
                    let y_of =
                        |v: f64| bottom - ((v - low) / (high - low) * f64::from(ph)).round() as i32;
                    let x_of = |d: f64| left + (d / profile.length * f64::from(pw)).round() as i32;
                    text(
                        px(8.0),
                        px(6.0),
                        0,
                        if errors {
                            "Estimate minus reference (m) · zero is closest"
                        } else {
                            "Source elevation (LN02 metres) · all four rasters on one scale"
                        },
                    );
                    for i in 0..=4 {
                        let t = f64::from(i) / 4.0;
                        let y = bottom - (t * f64::from(ph)).round() as i32;
                        line(
                            &[Point { x: left, y }, Point { x: left + pw, y }],
                            CreatePen(0, 1, rgb("#e2e9ee")),
                        );
                        text(
                            px(58.0),
                            y - px(8.0),
                            2,
                            &format!("{:.1}", low + t * (high - low)),
                        ); // TA_RIGHT
                        text(
                            left + (t * f64::from(pw)).round() as i32,
                            bottom + px(5.0),
                            6,
                            &format!("{:.1}", t * profile.length),
                        ); // TA_CENTER
                    }
                    text(
                        width / 2,
                        height - px(22.0),
                        6,
                        "Distance along A → B (projected metres)",
                    );
                    if errors {
                        line(
                            &[
                                Point {
                                    x: left,
                                    y: y_of(0.0),
                                },
                                Point {
                                    x: left + pw,
                                    y: y_of(0.0),
                                },
                            ],
                            CreatePen(1, 1, rgb("#b4bfca")),
                        );
                    }
                    for band in [0usize, 3, 1, 2] {
                        if !bands.contains(&band) {
                            continue;
                        }
                        let points: Vec<Point> = profile
                            .samples
                            .iter()
                            .map(|s| Point {
                                x: x_of(s.distance),
                                y: y_of(value(s, band)),
                            })
                            .collect();
                        // GDI draws dashes only at width 1; the reference stays thin and dashed.
                        let pen = if band == 3 {
                            CreatePen(1, 1, rgb(COLORS[3]))
                        } else {
                            CreatePen(
                                0,
                                px(if band == 0 { 1.5 } else { 2.0 }).max(1),
                                rgb(COLORS[band]),
                            )
                        };
                        line(&points, pen);
                    }
                    if hovered >= 0 && (hovered as usize) < profile.samples.len() {
                        let x = x_of(profile.samples[hovered as usize].distance);
                        line(
                            &[Point { x, y: top }, Point { x, y: bottom }],
                            CreatePen(1, 1, rgb("#dd3470")),
                        );
                    }
                }
            }
            SelectObject(dc, old_font);
            DeleteObject(font);
            SelectObject(dc, previous);
            DeleteDC(dc);
            let old = CHART_BITMAP.with(|b| b.replace(bitmap));
            if !old.is_null() {
                DeleteObject(old);
            }
            InvalidateRect(self.chart, std::ptr::null(), 0);
        }
    }
}

impl Drop for Panes {
    fn drop(&mut self) {
        self.maps.clear();
        unsafe {
            for hwnd in self.headings.iter().chain(&self.hosts).chain([&self.chart]) {
                DestroyWindow(*hwnd);
            }
            let bitmap = CHART_BITMAP.with(|b| b.replace(std::ptr::null_mut()));
            if !bitmap.is_null() {
                DeleteObject(bitmap);
            }
            DeleteObject(self.font);
        }
    }
}
