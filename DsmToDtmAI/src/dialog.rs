//! Explicit open dialogs and a parent-folder picker (Win32 common dialogs and shell browser).
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(windows)]
fn open(owner: usize, title: &str, filter: &str, ext: &str) -> Result<Option<String>> {
    use windows_sys::Win32::UI::Controls::Dialogs::*;
    let mut file = vec![0u16; 32768];
    let filter: Vec<u16> = filter.encode_utf16().collect();
    let ext: Vec<u16> = ext.encode_utf16().chain(Some(0)).collect();
    let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let mut options = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner as _,
        lpstrFilter: filter.as_ptr(),
        lpstrTitle: title.as_ptr(),
        lpstrDefExt: ext.as_ptr(),
        lpstrFile: file.as_mut_ptr(),
        nMaxFile: file.len() as u32,
        Flags: OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_FILEMUSTEXIST,
        ..Default::default()
    };
    if unsafe { GetOpenFileNameW(&mut options) } == 0 {
        let e = unsafe { CommDlgExtendedError() };
        return if e == 0 {
            Ok(None)
        } else {
            Err(format!("File dialog failed: {e}").into())
        };
    }
    let n = file.iter().position(|v| *v == 0).unwrap_or(file.len());
    Ok(Some(String::from_utf16(&file[..n])?))
}

#[cfg(not(windows))]
fn open(_owner: usize, _title: &str, _filter: &str, _ext: &str) -> Result<Option<String>> {
    Err("This example requires Windows".into())
}

pub fn open_tif(owner: usize, title: &str) -> Result<Option<String>> {
    open(
        owner,
        title,
        "GeoTIFF (*.tif;*.tiff)\0*.tif;*.tiff\0\0",
        "tif",
    )
}

pub fn open_zip(owner: usize, title: &str) -> Result<Option<String>> {
    open(owner, title, "ZIP (*.zip)\0*.zip\0\0", "zip")
}

/// The parent folder for a new result bundle (shell folder browser).
pub fn choose_folder(owner: usize, title: &str) -> Result<Option<String>> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Com::CoTaskMemFree;
        use windows_sys::Win32::UI::Shell::{
            SHBrowseForFolderW, SHGetPathFromIDListW, BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS,
            BROWSEINFOW,
        };
        let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
        // SAFETY: an all-zero BROWSEINFOW is valid (no root, callback or display buffer).
        let mut info: BROWSEINFOW = unsafe { std::mem::zeroed() };
        info.hwndOwner = owner as _;
        info.lpszTitle = title.as_ptr();
        info.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE;
        let list = unsafe { SHBrowseForFolderW(&info) };
        if list.is_null() {
            return Ok(None);
        }
        let mut path = vec![0u16; 32768];
        let ok = unsafe { SHGetPathFromIDListW(list, path.as_mut_ptr()) };
        unsafe { CoTaskMemFree(list as _) };
        if ok == 0 {
            return Err("Choose a file-system folder.".into());
        }
        let n = path.iter().position(|v| *v == 0).unwrap_or(path.len());
        Ok(Some(String::from_utf16(&path[..n])?))
    }
    #[cfg(not(windows))]
    {
        let _ = (owner, title);
        Err("This example requires Windows".into())
    }
}
