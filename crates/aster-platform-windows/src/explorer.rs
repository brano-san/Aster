use crate::shell_registry;

const PLACES: [(&str, &str); 3] = [(r"Software\Classes\Directory\Background\shell\Aster", "%V"), (r"Software\Classes\Directory\shell\Aster", "%1"), (r"Software\Classes\Drive\shell\Aster", "%1")];

const PASTE_PLACES: [(&str, &str); 2] = [(r"Software\Classes\*\shell\AsterPastePath", "%1"), (r"Software\Classes\Directory\shell\AsterPastePath", "%1")];

pub fn set(label: &str, enabled: bool) -> bool {
    set_verbs(&PLACES, "open", label, enabled)
}

pub fn set_paste(label: &str, enabled: bool) -> bool {
    set_verbs(&PASTE_PLACES, "paste-path", label, enabled)
}

fn set_verbs(places: &[(&str, &str)], verb: &str, label: &str, enabled: bool) -> bool {
    if !enabled {
        let mut ok = true;
        for &(path, _) in places {
            ok &= shell_registry::remove(path);
        }
        return ok;
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let exe = exe.to_string_lossy();
    let icon = format!("\"{exe}\",0");
    places.iter().all(|(path, arg)| {
        shell_registry::set(path, &[(None, label), (Some("Icon"), &icon)])
            && shell_registry::set(&format!(r"{path}\command"), &[(None, &format!("\"{exe}\" {verb} \"{arg}\""))])
    })
}

#[cfg(test)]
#[path = "../test/explorer.rs"]
mod tests;
