use std::os::windows::ffi::OsStrExt;
use std::path::Path;
use windows::core::PCWSTR;
use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED};
use windows::Win32::UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems};

pub fn select(path: &Path) -> bool {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    unsafe {
        if CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_err() { return false; }
        let item = ILCreateFromPathW(PCWSTR(wide.as_ptr()));
        let shown = if item.is_null() {
            false
        } else {
            let result = SHOpenFolderAndSelectItems(item, None, 0).is_ok();
            ILFree(Some(item));
            result
        };
        CoUninitialize();
        shown
    }
}
