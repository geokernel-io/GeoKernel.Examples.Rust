//! Explicit save dialog for the profile CSV.
pub fn save_csv(owner: usize) -> Result<Option<String>, Box<dyn std::error::Error>> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Controls::Dialogs::*;
        let mut file = vec![0u16; 32768];
        for (i, c) in "profile.csv".encode_utf16().enumerate() {
            file[i] = c;
        }
        let filter: Vec<u16> = "CSV (*.csv)\0*.csv\0\0".encode_utf16().collect();
        let ext: Vec<u16> = "csv\0".encode_utf16().collect();
        let title: Vec<u16> = "Export numeric profile\0".encode_utf16().collect();
        let mut options = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: owner as _,
            lpstrFilter: filter.as_ptr(),
            lpstrTitle: title.as_ptr(),
            lpstrDefExt: ext.as_ptr(),
            lpstrFile: file.as_mut_ptr(),
            nMaxFile: file.len() as u32,
            Flags: OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_OVERWRITEPROMPT,
            ..Default::default()
        };
        if unsafe { GetSaveFileNameW(&mut options) } == 0 {
            let e = unsafe { CommDlgExtendedError() };
            return if e == 0 {
                Ok(None)
            } else {
                Err(format!("Save dialog failed: {e}").into())
            };
        }
        let n = file.iter().position(|v| *v == 0).unwrap_or(file.len());
        Ok(Some(String::from_utf16(&file[..n])?))
    }
    #[cfg(not(windows))]
    {
        let _ = owner;
        Err("This example requires Windows".into())
    }
}
