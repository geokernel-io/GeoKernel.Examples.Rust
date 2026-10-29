pub fn save(owner: usize) -> Result<Option<String>, Box<dyn std::error::Error>> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Controls::Dialogs::*;
        let mut file = vec![0u16; 32768];
        for (i, c) in "building_changes.geojson".encode_utf16().enumerate() {
            file[i] = c;
        }
        let filter: Vec<u16> = "GeoJSON (*.geojson)\0*.geojson\0\0"
            .encode_utf16()
            .collect();
        let ext: Vec<u16> = "geojson\0".encode_utf16().collect();
        let title: Vec<u16> = "Export approved candidates\0".encode_utf16().collect();
        let mut options = OPENFILENAMEW::default();
        options.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        options.hwndOwner = owner as _;
        options.lpstrFilter = filter.as_ptr();
        options.lpstrTitle = title.as_ptr();
        options.lpstrDefExt = ext.as_ptr();
        options.lpstrFile = file.as_mut_ptr();
        options.nMaxFile = file.len() as u32;
        options.Flags = OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_OVERWRITEPROMPT;
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
pub fn confirm(owner: usize) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let text: Vec<u16> = "Replace this session’s candidates and review decisions?\0"
            .encode_utf16()
            .collect();
        let title: Vec<u16> = "Run analysis\0".encode_utf16().collect();
        unsafe {
            MessageBoxW(
                owner as _,
                text.as_ptr(),
                title.as_ptr(),
                MB_YESNO | MB_DEFBUTTON2,
            ) == IDYES
        }
    }
    #[cfg(not(windows))]
    {
        let _ = owner;
        false
    }
}
