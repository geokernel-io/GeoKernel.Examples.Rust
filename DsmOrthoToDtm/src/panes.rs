//! The central area of the native window: pane headings, native 2D map hosts and a
//! text strip with the legend and run summary, all Win32 child windows of the viewer surface.
use crate::map2d::{Event, Library2D, Map2D};
use std::{ffi::c_void, marker::PhantomData, path::Path, rc::Rc, sync::mpsc::Sender};

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
}
#[link(name = "gdi32")]
extern "system" {
    fn DeleteObject(object: *mut c_void) -> i32;
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

const WS_CHILD_VISIBLE_CLIP: u32 = 0x5600_0000;
const SS_CENTERIMAGE: u32 = 0x200;

pub struct Panes {
    parent: Hwnd,
    headings: Vec<Hwnd>,
    hosts: Vec<Hwnd>,
    footer: Hwnd,
    fonts: [*mut c_void; 2],
    rect: Rect,
    footer_text: String,
    pub maps: Vec<Map2D>,
    _main_thread: PhantomData<Rc<()>>,
}

impl Panes {
    /// Safety: main Qt GUI thread; `parent` (the window's viewer surface) outlives the panes.
    pub unsafe fn new(
        parent: usize,
        viewer_dll: &Path,
        events: Sender<Event>,
        titles: &[&str],
    ) -> Result<Self> {
        let parent = parent as Hwnd;
        let library = Library2D::new(viewer_dll)?;
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
        let headings: Vec<Hwnd> = titles
            .iter()
            .map(|t| child("STATIC", t, SS_CENTERIMAGE))
            .collect();
        let hosts: Vec<Hwnd> = titles.iter().map(|_| child("STATIC", "", 0)).collect();
        let footer = child("STATIC", "", 0);
        if headings
            .iter()
            .chain(&hosts)
            .chain([&footer])
            .any(|h| h.is_null())
        {
            return Err(std::io::Error::last_os_error().into());
        }
        let scale = GetDpiForWindow(parent).max(96) as i32;
        let face = wide("Segoe UI");
        let font = |weight: i32| {
            CreateFontW(
                -(9 * scale / 72),
                0,
                0,
                0,
                weight,
                0,
                0,
                0,
                1,
                0,
                0,
                5,
                0,
                face.as_ptr(),
            )
        };
        let fonts = [font(600), font(400)];
        for &heading in &headings {
            SendMessageW(heading, 0x30, fonts[0] as usize, 1); // WM_SETFONT
        }
        SendMessageW(footer, 0x30, fonts[1] as usize, 1);
        let mut panes = Self {
            parent,
            headings,
            hosts,
            footer,
            fonts,
            rect: Rect::default(),
            footer_text: String::new(),
            maps: Vec::new(),
            _main_thread: PhantomData,
        };
        for (side, &host) in panes.hosts.iter().enumerate() {
            panes
                .maps
                .push(Map2D::new(&library, host, side, events.clone())?);
        }
        Ok(panes)
    }

    /// Lay out headings, maps and the text strip when the surface size changes.
    pub fn layout(&mut self) -> Result<()> {
        let mut rect = Rect::default();
        if unsafe { GetClientRect(self.parent, &mut rect) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if rect == self.rect {
            return Ok(());
        }
        let s = f64::from(unsafe { GetDpiForWindow(self.parent) }.max(96)) / 96.0;
        let px = |v: f64| (v * s).round() as i32;
        let (w, h) = (rect.right.max(3), rect.bottom.max(2));
        let (gap, title) = (px(8.0), px(34.0));
        let footer_h = px(118.0).min(h / 3);
        let n = self.hosts.len() as i32;
        let column = (w - (n - 1) * gap) / n;
        let map_h = (h - footer_h - title - gap).max(1);
        unsafe {
            for i in 0..self.hosts.len() {
                let x = i as i32 * (column + gap);
                MoveWindow(self.headings[i], x, 0, column, title, 1);
                MoveWindow(self.hosts[i], x, title, column, map_h, 1);
                self.maps[i].resize(column, map_h);
            }
            MoveWindow(
                self.footer,
                px(4.0),
                h - footer_h + px(4.0),
                w - px(8.0),
                footer_h - px(4.0),
                1,
            );
        }
        self.rect = rect;
        Ok(())
    }

    pub fn set_heading(&self, index: usize, text: &str) {
        let text = wide(text);
        unsafe { SetWindowTextW(self.headings[index], text.as_ptr()) };
    }

    /// Legend, run summary and notes below the maps; rewritten only when they change.
    pub fn set_footer(&mut self, text: &str) {
        if text == self.footer_text {
            return;
        }
        // STATIC controls need CRLF line breaks.
        let crlf = wide(&text.replace('\n', "\r\n"));
        unsafe { SetWindowTextW(self.footer, crlf.as_ptr()) };
        self.footer_text = text.to_owned();
    }
}

impl Drop for Panes {
    fn drop(&mut self) {
        self.maps.clear();
        unsafe {
            for hwnd in self
                .headings
                .iter()
                .chain(&self.hosts)
                .chain([&self.footer])
            {
                DestroyWindow(*hwnd);
            }
            for font in self.fonts {
                DeleteObject(font);
            }
        }
    }
}
