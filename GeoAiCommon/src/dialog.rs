pub fn save_format(
    owner: usize,
    default_name: &str,
    format: &str,
) -> crate::Result<Option<String>> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::Controls::Dialogs::*;
        let mut file = vec![0u16; 32768];
        let initial: Vec<u16> = default_name.encode_utf16().collect();
        if initial.len() >= file.len() {
            return Err("Export filename is too long".into());
        }
        file[..initial.len()].copy_from_slice(&initial);
        let filter: Vec<u16> = if format == "geotiff" {
            "GeoTIFF (*.tif)\0*.tif\0\0"
        } else {
            "Shapefile (*.shp)\0*.shp\0\0"
        }
        .encode_utf16()
        .collect();
        let title: Vec<u16> = "Save result\0".encode_utf16().collect();
        let extension: Vec<u16> = if format == "geotiff" {
            "tif\0"
        } else {
            "shp\0"
        }
        .encode_utf16()
        .collect();
        let mut options = OPENFILENAMEW::default();
        options.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
        options.hwndOwner = owner as _;
        options.lpstrFilter = filter.as_ptr();
        options.lpstrTitle = title.as_ptr();
        options.lpstrDefExt = extension.as_ptr();
        options.lpstrFile = file.as_mut_ptr();
        options.nMaxFile = file.len() as u32;
        options.Flags = OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;
        if unsafe { GetSaveFileNameW(&mut options) } == 0 {
            let error = unsafe { CommDlgExtendedError() };
            return if error == 0 {
                Ok(None)
            } else {
                Err(format!("Save dialog failed: {error}").into())
            };
        }
        let length = file.iter().position(|v| *v == 0).unwrap_or(file.len());
        return Ok(Some(String::from_utf16(&file[..length])?));
    }
    #[cfg(not(windows))]
    {
        let _ = (owner, default_name, format);
        Err("The save dialog currently requires Windows".into())
    }
}
