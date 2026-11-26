use std::ffi::OsString;
use std::io;
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use aster_platform::PlatformError;
use windows::core::{HSTRING, Interface};
use windows::Win32::System::Com::{IPersistFile, STGM_READ};
use windows::Win32::UI::Shell::{FOLDERID_Desktop, IShellLinkW, KF_FLAG_DEFAULT, SHChangeNotify, SHGetKnownFolderPath, SHCNE_CREATE, SHCNE_DELETE, SHCNE_ID, SHCNE_UPDATEITEM, SHCNF_PATHW, SLGP_RAWPATH, PropertiesSystem::IPropertyStore};
use crate::shell_link::{self, Apartment};

fn io_error(error: io::Error) -> PlatformError {
    PlatformError::new(error.raw_os_error().unwrap_or_default().into(), error.to_string())
}

pub(crate) fn notify(path: &Path, event: SHCNE_ID) {
    let path = HSTRING::from(path);
    unsafe { SHChangeNotify(event, SHCNF_PATHW, Some(path.as_ptr().cast()), None) };
}

fn load(path: &Path) -> windows::core::Result<IShellLinkW> {
    let link = shell_link::new()?;
    let file: IPersistFile = link.cast()?;
    unsafe { file.Load(&HSTRING::from(path), STGM_READ)? };
    Ok(link)
}

fn target(link: &IShellLinkW) -> windows::core::Result<PathBuf> {
    let mut buffer = vec![0; 32768];
    unsafe { link.GetPath(&mut buffer, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)? };
    let end = buffer.iter().position(|&v| v == 0).unwrap_or(buffer.len());
    Ok(PathBuf::from(OsString::from_wide(&buffer[..end])))
}

pub(crate) fn same_path(a: &Path, b: &Path) -> bool {
    a == b || matches!((std::fs::canonicalize(a), std::fs::canonicalize(b)), (Ok(a), Ok(b)) if a == b)
}

fn save(path: &Path, exe: &Path) -> windows::core::Result<()> {
    let link = shell_link::for_exe(exe)?;
    let store: IPropertyStore = link.cast()?;
    shell_link::set_app_id(&store)?;
    unsafe {
        store.Commit()?;
        link.cast::<IPersistFile>()?.Save(&HSTRING::from(path), true)
    }
}

fn update(path: &Path, exe: &Path, change: Option<bool>) -> Result<bool, PlatformError> {
    if change == Some(false) {
        match std::fs::remove_file(path) {
            Ok(()) => notify(path, SHCNE_DELETE),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(io_error(error)),
        }
        return Ok(false);
    }
    let exists = path.try_exists().map_err(io_error)?;
    if change.is_none() && (!exists || load(path).and_then(|link| target(&link)).is_ok_and(|target| same_path(&target, exe))) {
        return Ok(exists);
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(io_error)?;
    }
    save(path, exe).map_err(crate::err)?;
    notify(path, if exists { SHCNE_UPDATEITEM } else { SHCNE_CREATE });
    Ok(true)
}

pub fn apply(change: Option<bool>) -> Result<bool, PlatformError> {
    let exe = std::env::current_exe().map_err(io_error)?;
    apply_for(change, &exe)
}

pub(crate) fn apply_for(change: Option<bool>, exe: &Path) -> Result<bool, PlatformError> {
    let path = start_menu_path()?;
    let _com = Apartment::enter().map_err(crate::err)?;
    update(&path, exe, change)
}

fn start_menu_path() -> Result<PathBuf, PlatformError> {
    let appdata = std::env::var_os("APPDATA").ok_or_else(|| PlatformError::new(0, "APPDATA unavailable"))?;
    Ok(PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs\Aster.lnk"))
}

fn desktop_path() -> Result<PathBuf, PlatformError> {
    unsafe {
        let value = SHGetKnownFolderPath(&FOLDERID_Desktop, KF_FLAG_DEFAULT, None).map_err(crate::err)?;
        let folder = OsString::from_wide(value.as_wide());
        windows::Win32::System::Com::CoTaskMemFree(Some(value.0.cast()));
        Ok(PathBuf::from(folder).join("Aster.lnk"))
    }
}

pub(crate) fn installed_shortcuts(exe: &Path) -> (bool, bool) {
    let Ok(_com) = Apartment::enter() else { return (false, false) };
    let matches = |path: PathBuf| points_to(&path, exe);
    (start_menu_path().is_ok_and(matches), desktop_path().is_ok_and(matches))
}

pub(crate) fn apply_desktop_for(change: bool, exe: &Path) -> Result<(), PlatformError> {
    apply_owned_shortcut(desktop_path(), change, exe)
}

pub(crate) fn apply_start_menu_for(change: bool, exe: &Path) -> Result<(), PlatformError> {
    apply_owned_shortcut(start_menu_path(), change, exe)
}

fn points_to(path: &Path, exe: &Path) -> bool {
    load(path).and_then(|link| target(&link)).is_ok_and(|target| same_path(&target, exe))
}

fn apply_owned_shortcut(path: Result<PathBuf, PlatformError>, change: bool, exe: &Path) -> Result<(), PlatformError> {
    let path = match path {
        Ok(path) => path,
        Err(_) if !change => return Ok(()),
        Err(error) => return Err(error),
    };
    let _com = Apartment::enter().map_err(crate::err)?;
    if path.try_exists().map_err(io_error)? && !points_to(&path, exe) {
        if change {
            return Err(io_error(io::Error::new(io::ErrorKind::AlreadyExists, path.display().to_string())));
        }
        return Ok(());
    }
    update(&path, exe, Some(change)).map(|_| ())
}

#[cfg(test)]
#[path = "../test/shortcut.rs"]
mod tests;
