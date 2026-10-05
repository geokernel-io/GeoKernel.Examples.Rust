//! Windows file dialogs, owned by the existing Qt top-level window.
use std::{ffi::c_void, os::windows::ffi::{OsStrExt, OsStringExt}, path::{Path, PathBuf}};

#[repr(C)]
struct OpenFileName {
    size: u32,
    owner: *mut c_void,
    instance: *mut c_void,
    filter: *const u16,
    custom_filter: *mut u16,
    max_custom_filter: u32,
    filter_index: u32,
    file: *mut u16,
    max_file: u32,
    file_title: *mut u16,
    max_file_title: u32,
    initial_dir: *const u16,
    title: *const u16,
    flags: u32,
    file_offset: u16,
    file_extension: u16,
    default_extension: *const u16,
    custom_data: isize,
    hook: *mut c_void,
    template_name: *const u16,
    reserved: *mut c_void,
    reserved_size: u32,
    flags_ex: u32,
}
#[link(name = "comdlg32")]
extern "system" {
    fn GetOpenFileNameW(value: *mut OpenFileName) -> i32;
    fn GetSaveFileNameW(value: *mut OpenFileName) -> i32;
    fn CommDlgExtendedError() -> u32;
}
#[link(name = "user32")]
extern "system" {
    fn GetAncestor(window: *mut c_void, flags: u32) -> *mut c_void;
}

pub fn choose(parent: usize, save: bool, current: Option<&Path>) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let mut file = vec![0u16; 32768];
    let initial = current.unwrap_or_else(|| Path::new(if save { "SagradaFamilia.gk3d" } else { "" }));
    let encoded: Vec<u16> = initial.as_os_str().encode_wide().collect();
    if encoded.len() >= file.len() { return Err("Project path is too long".into()); }
    file[..encoded.len()].copy_from_slice(&encoded);
    let filter: Vec<u16> = if save {
        "GeoKernel 3D project (*.gk3d)\0*.gk3d\0\0"
    } else {
        "GeoKernel 3D project (*.gk3d;*.json)\0*.gk3d;*.json\0\0"
    }.encode_utf16().collect();
    let title: Vec<u16> = if save { "Save project\0" } else { "Open project\0" }.encode_utf16().collect();
    let extension: Vec<u16> = "gk3d\0".encode_utf16().collect();
    // All fields are scalars or raw pointers; zero is a valid initial representation.
    let mut dialog: OpenFileName = unsafe { std::mem::zeroed() };
    dialog.size = std::mem::size_of::<OpenFileName>() as u32;
    dialog.owner = unsafe { GetAncestor(parent as *mut c_void, 2) };
    dialog.filter = filter.as_ptr();
    dialog.filter_index = 1;
    dialog.file = file.as_mut_ptr();
    dialog.max_file = file.len() as u32;
    dialog.title = title.as_ptr();
    dialog.default_extension = extension.as_ptr();
    // Explorer, no change to working directory, existing parent, overwrite/file checks.
    dialog.flags = 0x00080000 | 0x00000008 | 0x00000800 | if save { 0x00000002 } else { 0x00001000 };
    let accepted = unsafe { if save { GetSaveFileNameW(&mut dialog) } else { GetOpenFileNameW(&mut dialog) } };
    if accepted == 0 {
        let error = unsafe { CommDlgExtendedError() };
        if error != 0 { return Err(format!("Project file dialog failed: 0x{error:08X}").into()); }
        return Ok(None);
    }
    let end = file.iter().position(|v| *v == 0).ok_or("Invalid dialog path")?;
    Ok(Some(PathBuf::from(std::ffi::OsString::from_wide(&file[..end]))))
}
