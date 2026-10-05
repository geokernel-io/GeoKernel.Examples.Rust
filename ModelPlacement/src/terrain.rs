//! Main-thread adapter for Viewer3D using the published GeoKernel 1.5.32 SDK.
use libloading::Library;
use std::{
    ffi::{c_char, c_void, CStr, CString},
    marker::PhantomData,
    path::Path,
    rc::Rc,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Handle = *mut c_void;

#[repr(C)]
#[derive(Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[link(name = "user32")]
extern "system" {
    fn GetClientRect(hwnd: Handle, rect: *mut Rect) -> i32;
}

pub struct Terrain {
    handle: Handle,
    parent: Handle,
    destroy: unsafe extern "C" fn(Handle),
    error: unsafe extern "C" fn() -> *const c_char,
    resize: unsafe extern "C" fn(Handle, i32, i32) -> i32,
    load: unsafe extern "C" fn(
        Handle,
        *const c_char,
        *const c_char,
        *const c_char,
        *const c_char,
        *const f64,
        i32,
    ) -> i32,
    model_visible: unsafe extern "C" fn(Handle, i32) -> i32,
    model_count: unsafe extern "C" fn(Handle, *mut u64) -> i32,
    focus_model: unsafe extern "C" fn(Handle) -> i32,
    poll: unsafe extern "C" fn(Handle) -> i32,
    cancel: unsafe extern "C" fn(Handle),
    imagery: unsafe extern "C" fn(Handle, i32) -> i32,
    height: unsafe extern "C" fn(Handle, f32) -> i32,
    reset: unsafe extern "C" fn(Handle) -> i32,
    size: (i32, i32),
    _library: Library,
    _main_thread: PhantomData<Rc<()>>,
}

impl Terrain {
    /// Safety: main Qt GUI thread only; parent must outlive this adapter.
    pub unsafe fn new(parent: usize, library: &Path) -> Result<Self> {
        let dll = Library::new(library)?;
        let create = *dll.get::<unsafe extern "C" fn(Handle) -> Handle>(b"GeoKernel3D_Create")?;
        // Resolve every symbol before allocating the native object.
        let destroy = *dll.get(b"GeoKernel3D_Destroy")?;
        let error: unsafe extern "C" fn() -> *const c_char = *dll.get(b"GeoKernel3D_LastError")?;
        let resize = *dll.get(b"GeoKernel3D_Resize")?;
        let load = *dll.get(b"GeoKernel3D_LoadModelOnTerrain")?;
        let model_visible = *dll.get(b"GeoKernel3D_SetModelVisible")?;
        let model_count = *dll.get(b"GeoKernel3D_GetModelVertexCount")?;
        let focus_model = *dll.get(b"GeoKernel3D_FocusModel")?;
        let poll = *dll.get(b"GeoKernel3D_PollLoad")?;
        let cancel = *dll.get(b"GeoKernel3D_CancelLoad")?;
        let imagery = *dll.get(b"GeoKernel3D_SetImageryVisible")?;
        let height = *dll.get(b"GeoKernel3D_SetHeightScale")?;
        let reset = *dll.get(b"GeoKernel3D_ResetCamera")?;
        let handle = create(parent as Handle);
        if handle.is_null() {
            let message = error();
            return Err(if message.is_null() {
                "Viewer3D creation failed".into()
            } else {
                CStr::from_ptr(message)
                    .to_string_lossy()
                    .into_owned()
                    .into()
            });
        }
        Ok(Self {
            handle,
            parent: parent as Handle,
            destroy,
            error,
            resize,
            load,
            model_visible,
            model_count,
            focus_model,
            poll,
            cancel,
            imagery,
            height,
            reset,
            size: (0, 0),
            _library: dll,
            _main_thread: PhantomData,
        })
    }

    fn check(&self, code: i32) -> Result<i32> {
        if code < 0 {
            let message = unsafe { (self.error)() };
            return Err(if message.is_null() {
                "Viewer3D failed".into()
            } else {
                unsafe { CStr::from_ptr(message) }
                    .to_string_lossy()
                    .into_owned()
                    .into()
            });
        }
        Ok(code)
    }

    pub fn resize(&mut self) -> Result<()> {
        let mut rect = Rect::default();
        if unsafe { GetClientRect(self.parent, &mut rect) } == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        let size = (rect.right - rect.left, rect.bottom - rect.top);
        if size != self.size {
            self.check(unsafe { (self.resize)(self.handle, size.0.max(1), size.1.max(1)) })?;
            self.size = size;
        }
        Ok(())
    }

    pub fn load(
        &self,
        dem: &Path,
        imagery: &Path,
        model: &Path,
        geoid: &Path,
        placement: &[f64; 7],
        resolution: i32,
    ) -> Result<()> {
        if !placement.iter().all(|v| v.is_finite()) {
            return Err("Placement must be finite".into());
        }
        let paths: Vec<CString> = [dem, imagery, model, geoid]
            .iter()
            .map(|p| Ok(CString::new(p.to_str().ok_or("Path is not UTF-8")?)?))
            .collect::<Result<_>>()?;
        self.check(unsafe {
            (self.load)(
                self.handle,
                paths[0].as_ptr(),
                paths[1].as_ptr(),
                paths[2].as_ptr(),
                paths[3].as_ptr(),
                placement.as_ptr(),
                resolution,
            )
        })?;
        Ok(())
    }
    pub fn model_visible(&self, visible: bool) -> Result<()> {
        self.check(unsafe { (self.model_visible)(self.handle, i32::from(visible)) })?;
        Ok(())
    }
    pub fn focus_model(&self) -> Result<()> {
        self.check(unsafe { (self.focus_model)(self.handle) })?;
        Ok(())
    }
    pub fn model_vertex_count(&self) -> Result<u64> {
        let mut count = 0;
        self.check(unsafe { (self.model_count)(self.handle, &mut count) })?;
        Ok(count)
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
        unsafe { (self.cancel)(self.handle) };
    }
    pub fn imagery(&self, value: i32) -> Result<()> {
        self.check(unsafe { (self.imagery)(self.handle, value) })?;
        Ok(())
    }
    pub fn height(&self, value: f32) -> Result<()> {
        self.check(unsafe { (self.height)(self.handle, value) })?;
        Ok(())
    }
    pub fn reset(&self) -> Result<()> {
        self.check(unsafe { (self.reset)(self.handle) })?;
        Ok(())
    }
}
impl Drop for Terrain {
    fn drop(&mut self) {
        // Native destruction cancels and joins any CPU loader before releasing Vulkan.
        unsafe { (self.destroy)(self.handle) };
    }
}
