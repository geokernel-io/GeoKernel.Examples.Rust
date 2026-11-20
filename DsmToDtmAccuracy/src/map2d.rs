//! One native GeoKernel 2D map hosted in a Win32 child window, through the published
//! GeoKernelViewer C ABI (the same calls the WinForms control makes). The crate's
//! `ViewerWindow` panes report mouse events only for the primary pane, so each map here is
//! a standalone viewer with its own event callback. Events are copied into a channel and
//! handled after the Qt event pump returns; user code never runs inside the callback.
use libloading::Library;
use std::{
    ffi::c_void,
    marker::PhantomData,
    path::Path,
    rc::Rc,
    sync::mpsc::{Receiver, Sender},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type P = *mut c_void;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Extent {
    pub x_min: f64,
    pub y_min: f64,
    pub x_max: f64,
    pub y_max: f64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct EventData {
    event_type: i32,
    int_value: i32,
    int_value2: i32,
    double_value: f64,
    extent: Extent,
    screen: [i32; 4],
}

#[derive(Clone, Copy)]
pub struct Event {
    pub side: usize,
    pub kind: i32,
    pub button: i32,
    pub world: (f64, f64),
    /// World extent of a finished selection box (the event's full extent).
    pub extent: Extent,
    pub screen: (i32, i32),
}

pub const VISIBLE_EXTENT_CHANGED: i32 = 12;
pub const MAP_SELECTION_BOX_FINISHED: i32 = 18;
pub const MAP_MOUSE_DOWN: i32 = 20;
pub const MAP_MOUSE_UP: i32 = 22;

type Callback = unsafe extern "C" fn(*const EventData, *const u16, P);

struct Api {
    create: unsafe extern "C" fn(i32, i32) -> P,
    destroy: unsafe extern "C" fn(P),
    native_handle: unsafe extern "C" fn(P) -> usize,
    set_callback: unsafe extern "C" fn(P, Option<Callback>, P),
    show: unsafe extern "C" fn(P),
    resize: unsafe extern "C" fn(P, i32, i32),
    invalidate: unsafe extern "C" fn(P, bool, bool),
    add_layer: unsafe extern "C" fn(P, *const u16) -> bool,
    set_layer_name: unsafe extern "C" fn(P, i32, *const u16) -> bool,
    layer_count: unsafe extern "C" fn(P) -> i32,
    set_layer_visible: unsafe extern "C" fn(P, i32, bool) -> bool,
    clear_layers: unsafe extern "C" fn(P),
    refresh: unsafe extern "C" fn(P),
    set_tool: unsafe extern "C" fn(P, i32),
    zoom_in: unsafe extern "C" fn(P, f64),
    zoom_out: unsafe extern "C" fn(P, f64),
    get_extent: unsafe extern "C" fn(P, *mut Extent) -> bool,
    set_extent: unsafe extern "C" fn(P, Extent),
    clear_shapes: unsafe extern "C" fn(P),
    add_line: unsafe extern "C" fn(P, *const f64, i32, *const u16) -> bool,
    add_polygon: unsafe extern "C" fn(P, *const f64, i32, *const u16) -> bool,
    set_overlay_point: unsafe extern "C" fn(P, *const u16, f64, f64, *const u16) -> bool,
    _library: Library,
}

#[link(name = "user32")]
extern "system" {
    fn SetParent(child: P, parent: P) -> P;
    fn SetWindowLongPtrW(hwnd: P, index: i32, value: isize) -> isize;
    fn SetWindowPos(hwnd: P, after: P, x: i32, y: i32, w: i32, h: i32, flags: u32) -> i32;
    fn MoveWindow(hwnd: P, x: i32, y: i32, w: i32, h: i32, repaint: i32) -> i32;
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

unsafe extern "C" fn on_event(data: *const EventData, _text: *const u16, context: P) {
    if data.is_null() || context.is_null() {
        return;
    }
    let (side, tx) = &*context.cast::<(usize, Sender<Event>)>();
    let d = *data;
    let _ = tx.send(Event {
        side: *side,
        kind: d.event_type,
        button: d.int_value2,
        world: (d.extent.x_min, d.extent.y_min),
        extent: d.extent,
        screen: (d.screen[0], d.screen[1]),
    });
}

/// Loads the viewer entry points once per process (the library is already loaded by the runtime).
pub struct Library2D(Rc<Api>);

impl Library2D {
    /// Safety: `dll` is the trusted GeoKernel.Viewer.dll of the configured SDK.
    pub unsafe fn new(dll: &Path) -> Result<Self> {
        let library = Library::new(dll)?;
        macro_rules! f {
            ($name:literal) => {
                *library.get(concat!("GeoKernelViewer_", $name, "\0").as_bytes())?
            };
        }
        Ok(Self(Rc::new(Api {
            create: f!("Create"),
            destroy: f!("Destroy"),
            native_handle: f!("GetNativeHandle"),
            set_callback: f!("SetEventCallback"),
            show: f!("Show"),
            resize: f!("Resize"),
            invalidate: f!("InvalidateRenderCache"),
            add_layer: f!("AddLayerFile"),
            set_layer_name: f!("SetLayerName"),
            layer_count: f!("GetLayerCount"),
            set_layer_visible: f!("SetLayerVisible"),
            clear_layers: f!("ClearLayers"),
            refresh: f!("RefreshLayers"),
            set_tool: f!("SetTool"),
            zoom_in: f!("ZoomIn"),
            zoom_out: f!("ZoomOut"),
            get_extent: f!("GetViewExtent"),
            set_extent: f!("SetViewExtent"),
            clear_shapes: f!("ClearShapes"),
            add_line: f!("AddPolylineShape"),
            add_polygon: f!("AddPolygonShape"),
            set_overlay_point: f!("SetOverlayPoint"),
            _library: library,
        })))
    }
}

/// Main-thread adapter for one standalone native 2D map.
pub struct Map2D {
    api: Rc<Api>,
    handle: P,
    window: P,
    context: *mut (usize, Sender<Event>),
    _main_thread: PhantomData<Rc<()>>,
}

const WS_CHILD_VISIBLE_CLIP: isize = 0x5600_0000;

impl Map2D {
    /// Safety: main Qt GUI thread; `host` outlives the map.
    pub unsafe fn new(
        library: &Library2D,
        host: P,
        side: usize,
        events: Sender<Event>,
    ) -> Result<Self> {
        let api = library.0.clone();
        let handle = (api.create)(1, 1);
        if handle.is_null() {
            return Err("GeoKernelViewer_Create failed.".into());
        }
        let context = Box::into_raw(Box::new((side, events)));
        let map = Self {
            window: (api.native_handle)(handle) as P,
            api,
            handle,
            context,
            _main_thread: PhantomData,
        };
        if map.window.is_null() {
            return Err("The native map returned no window handle.".into());
        }
        SetWindowLongPtrW(map.window, -16, WS_CHILD_VISIBLE_CLIP); // GWL_STYLE
        SetParent(map.window, host);
        SetWindowPos(map.window, std::ptr::null_mut(), 0, 0, 0, 0, 0x0037); // NOMOVE|NOSIZE|NOZORDER|NOACTIVATE|FRAMECHANGED
        (map.api.set_callback)(handle, Some(on_event), context.cast());
        (map.api.show)(handle);
        Ok(map)
    }

    /// Size the map to its host's client area (physical pixels).
    pub fn resize(&self, width: i32, height: i32) {
        let (w, h) = (width.max(1), height.max(1));
        unsafe {
            MoveWindow(self.window, 0, 0, w, h, 1);
            (self.api.resize)(self.handle, w, h);
            (self.api.invalidate)(self.handle, false, false);
        }
    }

    pub fn add_layer(&self, path: &Path, name: &str) -> Result<()> {
        let file = wide(path.to_str().ok_or("Path is not UTF-8")?);
        let name = wide(name);
        unsafe {
            if !(self.api.add_layer)(self.handle, file.as_ptr()) {
                return Err(
                    format!("The relief layer could not be added: {}", path.display()).into(),
                );
            }
            (self.api.set_layer_name)(self.handle, 0, name.as_ptr());
        }
        Ok(())
    }
    pub fn layer_count(&self) -> i32 {
        unsafe { (self.api.layer_count)(self.handle) }
    }
    pub fn set_layer_visible(&self, index: i32, visible: bool) {
        unsafe { (self.api.set_layer_visible)(self.handle, index, visible) };
    }
    pub fn clear_layers(&self) {
        unsafe { (self.api.clear_layers)(self.handle) }
    }
    pub fn refresh(&self) {
        unsafe { (self.api.refresh)(self.handle) }
    }
    pub fn set_tool(&self, tool: i32) {
        unsafe { (self.api.set_tool)(self.handle, tool) }
    }
    pub fn zoom(&self, zoom_in: bool) {
        unsafe {
            if zoom_in {
                (self.api.zoom_in)(self.handle, 1.5)
            } else {
                (self.api.zoom_out)(self.handle, 1.5)
            }
        }
    }
    pub fn extent(&self) -> Option<Extent> {
        let mut extent = Extent::default();
        unsafe { (self.api.get_extent)(self.handle, &mut extent) }.then_some(extent)
    }
    pub fn set_extent(&self, extent: Extent) {
        unsafe { (self.api.set_extent)(self.handle, extent) }
    }

    pub fn clear_shapes(&self) {
        unsafe { (self.api.clear_shapes)(self.handle) }
    }
    pub fn add_polygon(&self, ring: &[[f64; 2]], style: &str) {
        let xy: Vec<f64> = ring.iter().flatten().copied().collect();
        let style = wide(style);
        unsafe {
            (self.api.add_polygon)(self.handle, xy.as_ptr(), ring.len() as i32, style.as_ptr())
        };
    }
    pub fn add_line(&self, points: &[[f64; 2]], style: &str) {
        let xy: Vec<f64> = points.iter().flatten().copied().collect();
        let style = wide(style);
        unsafe {
            (self.api.add_line)(
                self.handle,
                xy.as_ptr(),
                points.len() as i32,
                style.as_ptr(),
            )
        };
    }
    /// A lightweight overlay point that follows the map without re-rendering it.
    pub fn set_overlay_point(&self, id: &str, p: [f64; 2], style: &str) {
        let (id, style) = (wide(id), wide(style));
        unsafe {
            (self.api.set_overlay_point)(self.handle, id.as_ptr(), p[0], p[1], style.as_ptr())
        };
    }
}

impl Drop for Map2D {
    fn drop(&mut self) {
        unsafe {
            (self.api.set_callback)(self.handle, None, std::ptr::null_mut());
            (self.api.destroy)(self.handle);
            drop(Box::from_raw(self.context));
        }
    }
}

/// Drain the queued events of both maps.
pub fn drain(events: &Receiver<Event>) -> Vec<Event> {
    events.try_iter().collect()
}
