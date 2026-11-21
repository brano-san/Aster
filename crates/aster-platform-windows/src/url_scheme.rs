use crate::shell_registry;
use windows::Win32::UI::Shell::{SHChangeNotify, SHCNE_ASSOCCHANGED, SHCNF_IDLIST};

const PATH: &str = r"Software\Classes\aster";

pub fn set(enabled: bool) -> bool {
    let ok = if enabled {
        let Ok(exe) = std::env::current_exe() else { return false };
        let command = format!("\"{}\" url \"%1\"", exe.display());
        shell_registry::set(&format!(r"{PATH}\shell\open\command"), &[(None, &command)])
            && shell_registry::set(PATH, &[(None, "URL:Aster Protocol"), (Some("URL Protocol"), "")])
    } else {
        shell_registry::remove(PATH)
    };
    if ok {
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
    }
    ok
}
