//! Explicit open/save dialogs and a replacement confirmation (Win32 common dialogs).
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(windows)]
fn run(owner: usize, title: &str, default_name: &str, save: bool) -> Result<Option<String>> {
    use windows_sys::Win32::UI::Controls::Dialogs::*;
    let mut file = vec![0u16; 32768];
    for (i, c) in default_name.encode_utf16().enumerate() {
        file[i] = c;
    }
    let filter: Vec<u16> = "GeoTIFF (*.tif;*.tiff)\0*.tif;*.tiff\0\0"
        .encode_utf16()
        .collect();
    let ext: Vec<u16> = "tif\0".encode_utf16().collect();
    let title: Vec<u16> = title.encode_utf16().chain(Some(0)).collect();
    let mut options = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner as _,
        lpstrFilter: filter.as_ptr(),
        lpstrTitle: title.as_ptr(),
        lpstrDefExt: ext.as_ptr(),
        lpstrFile: file.as_mut_ptr(),
        nMaxFile: file.len() as u32,
        Flags: OFN_EXPLORER
            | OFN_PATHMUSTEXIST
            | OFN_NOCHANGEDIR
            | if save {
                OFN_OVERWRITEPROMPT
            } else {
                OFN_FILEMUSTEXIST
            },
        ..Default::default()
    };
    let accepted = unsafe {
        if save {
            GetSaveFileNameW(&mut options)
        } else {
            GetOpenFileNameW(&mut options)
        }
    };
    if accepted == 0 {
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
fn run(_owner: usize, _title: &str, _default_name: &str, _save: bool) -> Result<Option<String>> {
    Err("This example requires Windows".into())
}

pub fn open_tif(owner: usize, title: &str) -> Result<Option<String>> {
    run(owner, title, "", false)
}

pub fn save_tif(owner: usize) -> Result<Option<String>> {
    run(
        owner,
        "Save DTM, removed heights, QA and JSON",
        "terrain-result.tif",
        true,
    )
}

/// A modal question; true when the user chooses Yes.
pub fn confirm(owner: usize, title: &str, text: &str) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            MessageBoxW, IDYES, MB_ICONQUESTION, MB_YESNO,
        };
        let (title, text): (Vec<u16>, Vec<u16>) = (
            title.encode_utf16().chain(Some(0)).collect(),
            text.encode_utf16().chain(Some(0)).collect(),
        );
        unsafe {
            MessageBoxW(
                owner as _,
                text.as_ptr(),
                title.as_ptr(),
                MB_YESNO | MB_ICONQUESTION,
            ) == IDYES
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (owner, title, text);
        false
    }
}

/// A modal warning.
pub fn warn(owner: usize, title: &str, text: &str) {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};
        let (title, text): (Vec<u16>, Vec<u16>) = (
            title.encode_utf16().chain(Some(0)).collect(),
            text.encode_utf16().chain(Some(0)).collect(),
        );
        unsafe {
            MessageBoxW(
                owner as _,
                text.as_ptr(),
                title.as_ptr(),
                MB_OK | MB_ICONWARNING,
            )
        };
    }
    #[cfg(not(windows))]
    let _ = (owner, title, text);
}
