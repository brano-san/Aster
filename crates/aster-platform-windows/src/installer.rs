mod wizard;

use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::windows::fs::MetadataExt;
use std::os::windows::process::CommandExt;
use std::path::{Component, Path, PathBuf, Prefix};
use std::process::{Child, Command};
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, MOVEFILE_REPLACE_EXISTING, MoveFileExW};
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::Win32::UI::Shell::SHCNE_UPDATEITEM;
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
use windows::core::{HSTRING, w};
use crate::shell_registry::{self, Value};

const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Aster";
const ABOUT: &str = "https://github.com/brano-san/Aster";
const BINARY: &str = "aster.exe";

#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    Install { silent: bool },
    Uninstall { silent: bool },
}

#[derive(Clone)]
struct InstallPaths {
    dir: PathBuf,
    exe: PathBuf,
}

impl InstallPaths {
    fn new(local_appdata: &Path) -> Self {
        let dir = local_appdata.join(r"Programs\Aster");
        let exe = dir.join(BINARY);
        Self { dir, exe }
    }

    fn from_dir(dir: PathBuf) -> io::Result<Self> {
        let mut components = dir.components();
        let local_drive = matches!(components.next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)));
        let rooted = components.next() == Some(Component::RootDir);
        let valid = dir.file_name().is_some() && components.all(|component| match component {
            Component::Normal(name) => {
                let name = name.to_string_lossy();
                !name.ends_with(['.', ' ']) && !name.contains(['<', '>', ':', '"', '|', '?', '*'])
                    && !name.chars().any(char::is_control)
            }
            _ => false,
        });
        if !local_drive || !rooted || !valid {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, aster_core::installation::invalid_directory()));
        }
        let dir: PathBuf = dir.components().collect();
        let exe = dir.join(BINARY);
        Ok(Self { dir, exe })
    }

    fn current() -> io::Result<Self> {
        if let Some(location) = shell_registry::read_text(UNINSTALL_KEY, "InstallLocation")? {
            return Self::from_dir(PathBuf::from(location));
        }
        let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).filter(|path| path.is_absolute());
        Ok(Self::new(&root.ok_or_else(|| missing_environment_variable("LOCALAPPDATA"))?))
    }

    fn contains(&self, exe: &Path) -> bool {
        crate::shortcut::same_path(&self.exe, exe)
    }
}

fn setup_name(executable: &Path) -> bool {
    let Some(name) = executable.file_name().and_then(OsStr::to_str) else { return false };
    let name = name.to_ascii_lowercase();
    name.starts_with("aster-setup") || (name.starts_with("aster-") && name.ends_with("-x86_64-windows-setup.exe"))
}

pub fn parse(args: &[String], executable: Option<&Path>) -> Option<Result<Action, ()>> {
    let (uninstall, options) = match args.first().map(String::as_str) {
        Some("--install") => (false, &args[1..]),
        Some("--uninstall") => (true, &args[1..]),
        _ if executable.is_some_and(setup_name) => (false, args),
        _ => return None,
    };
    let silent = match options {
        [] => false,
        [flag] if flag == "--silent" => true,
        _ => return Some(Err(())),
    };
    Some(Ok(if uninstall { Action::Uninstall { silent } } else { Action::Install { silent } }))
}

fn uninstall_values(paths: &InstallPaths, version: &str, size: u64) -> [(&'static str, Value); 11] {
    let exe = paths.exe.as_os_str();
    let mut icon = OsString::from(exe);
    icon.push(",0");
    let mut uninstall = OsString::from("\"");
    uninstall.push(exe);
    uninstall.push("\" --uninstall");
    let mut quiet = uninstall.clone();
    quiet.push(" --silent");
    [
        ("DisplayName", Value::text("Aster")),
        ("Publisher", Value::text("Aster")),
        ("DisplayIcon", Value::text(icon)),
        ("InstallLocation", Value::text(&paths.dir)),
        ("URLInfoAbout", Value::text(ABOUT)),
        ("UninstallString", Value::text(uninstall)),
        ("QuietUninstallString", Value::text(quiet)),
        ("NoModify", Value::Dword(1)),
        ("NoRepair", Value::Dword(1)),
        ("DisplayVersion", Value::text(version)),
        ("EstimatedSize", Value::Dword(size.div_ceil(1024).min(u64::from(u32::MAX)) as u32)),
    ]
}

pub fn state() -> aster_core::installation::State {
    use aster_core::installation::State;
    let (Ok(exe), Ok(paths)) = (std::env::current_exe(), InstallPaths::current()) else { return State::Unavailable };
    if paths.contains(&exe) { return State::Installed }
    if exe.parent().is_some_and(|dir| dir.join("data").is_dir()) { return State::Unavailable }
    State::Available
}

pub fn refresh(exe: &Path, version: &str) -> io::Result<()> {
    crate::shortcut::notify(exe, SHCNE_UPDATEITEM);
    let Ok(paths) = InstallPaths::current() else { return Ok(()) };
    if !paths.contains(exe) { return Ok(()) }
    let values = uninstall_values(&paths, version, fs::metadata(exe)?.len());
    shell_registry::write(UNINSTALL_KEY, &values[9..])
}

fn io_platform(error: aster_platform::PlatformError) -> io::Error {
    io::Error::other(error.localized())
}

fn missing_environment_variable(name: &str) -> io::Error {
    io::Error::other(aster_core::installation::missing_environment_variable(name))
}

fn show_error(message: &str) {
    unsafe { MessageBoxW(None, &HSTRING::from(message), w!("Aster"), MB_OK | MB_ICONERROR); }
}

fn check_directory(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0 => Ok(()),
        Ok(_) => Err(io::Error::new(io::ErrorKind::InvalidInput, path.display().to_string())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn replace_executable(stage: &Path, live: &Path) -> io::Result<()> {
    let result = unsafe { MoveFileExW(&HSTRING::from(stage), &HSTRING::from(live), MOVEFILE_REPLACE_EXISTING) };
    let Err(error) = result else { return Ok(()) };
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_SHARING_VIOLATION};
    let code = error.code();
    if code != ERROR_ACCESS_DENIED.to_hresult() && code != ERROR_SHARING_VIOLATION.to_hresult() {
        return Err(io::Error::other(error));
    }
    let old = live.with_extension("exe.old");
    fs::rename(live, &old)?;
    if let Err(error) = fs::rename(stage, live) {
        return match fs::rename(&old, live) {
            Ok(()) => Err(error),
            Err(restore) => Err(io::Error::other(format!("{error}\n{}: {restore}", old.display()))),
        };
    }
    let _ = fs::remove_file(old);
    Ok(())
}

#[derive(Clone, Copy)]
struct Shortcuts {
    start_menu: bool,
    desktop: bool,
}

fn install(paths: &InstallPaths, source: &Path, version: &str, shortcuts: Shortcuts, interactive: bool) -> io::Result<()> {
    let _lock = operation_lock()?;
    check_directory(&paths.dir)?;
    let registered = InstallPaths::current()?;
    if registered.exe.is_file() && !registered.contains(&paths.exe) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, aster_core::installation::existing_installation()));
    }
    if !registered.contains(&paths.exe) && paths.exe.try_exists()? && !paths.contains(source) {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, paths.exe.display().to_string()));
    }
    use aster_platform::Platform;
    let config = crate::Windows::config_dir();
    if paths.dir.join("data").is_dir() || config.as_ref().is_some_and(|config| {
        paths.dir.ancestors().any(|parent| crate::shortcut::same_path(parent, config))
    }) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, aster_core::installation::portable_directory()));
    }
    crate::instance::stop_for_installation(&paths.exe, interactive.then_some(source))?;
    if !crate::shortcut::same_path(source, &paths.exe) {
        fs::create_dir_all(&paths.dir)?;
        let stage = paths.dir.join(format!("aster-{}.exe.tmp", std::process::id()));
        let result = fs::copy(source, &stage).and_then(|_| replace_executable(&stage, &paths.exe));
        if result.is_err() { let _ = fs::remove_file(&stage); }
        result?;
        crate::shortcut::notify(&paths.exe, SHCNE_UPDATEITEM);
    }
    shell_registry::write(UNINSTALL_KEY, &uninstall_values(paths, version, fs::metadata(&paths.exe)?.len()))?;
    crate::shortcut::apply_start_menu_for(shortcuts.start_menu, &paths.exe).map_err(io_platform)?;
    crate::shortcut::apply_desktop_for(shortcuts.desktop, &paths.exe).map_err(io_platform)?;
    Ok(())
}

fn operation_lock() -> io::Result<fs::File> {
    let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).filter(|root| root.is_absolute())
        .ok_or_else(|| missing_environment_variable("LOCALAPPDATA"))?;
    let dir = root.join("Programs");
    fs::create_dir_all(&dir)?;
    crate::config_lock::lock(&dir.join("aster-install.lock"))
}

fn system_dir() -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let mut buffer = [0u16; 32768];
    let len = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
    if len == 0 || len >= buffer.len() { return Err(io::Error::last_os_error()) }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..len])))
}

fn delete_after_exit(exe: &Path) -> io::Result<Child> {
    let system = system_dir()?;
    let mut command = Command::new(system.join("cmd.exe"));
    let dir = exe.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let script = r#"for /l %i in (1,1,20) do @(del /f /q "%ASTER_DELETE_EXE%" 2>nul & if not exist "%ASTER_DELETE_EXE%" (rmdir "%ASTER_DELETE_INSTALL%" 2>nul & exit /b 0) & ping -n 3 127.0.0.1 >nul) & exit /b 1"#;
    command.current_dir(&system).creation_flags(CREATE_NO_WINDOW.0)
        .env("ASTER_DELETE_EXE", exe).env("ASTER_DELETE_INSTALL", dir).args(["/d", "/v:off", "/c"]).raw_arg(script).spawn()
}

fn uninstall(paths: &InstallPaths, source: &Path, delete_settings: bool, close_sessions: impl FnOnce() -> io::Result<()>) -> io::Result<()> {
    let _lock = operation_lock()?;
    check_directory(&paths.dir)?;
    if !paths.exe.is_file() || !InstallPaths::current()?.contains(&paths.exe) {
        return Err(io::Error::new(io::ErrorKind::NotFound, aster_core::installation::not_installed()));
    }
    let settings = if delete_settings {
        use aster_platform::Platform;
        let dir = crate::Windows::config_dir().filter(|dir| dir.is_absolute()).ok_or_else(|| missing_environment_variable("APPDATA"))?;
        check_directory(&dir)?;
        Some(dir)
    } else {
        None
    };
    crate::instance::stop_for_installation(&paths.exe, Some(source))?;
    close_sessions()?;
    crate::defterm_registration::remove(&paths.exe)?;
    crate::shortcut::apply_start_menu_for(false, &paths.exe).map_err(io_platform)?;
    crate::shortcut::apply_desktop_for(false, &paths.exe).map_err(io_platform)?;
    crate::jump_list::remove().map_err(io_platform)?;
    let explorer = crate::explorer::set("", false);
    let paste = crate::explorer::set_paste("", false);
    let url = crate::url_scheme::set(false);
    if !(explorer && paste && url) {
        return Err(io::Error::other(aster_core::installation::integration_error()));
    }
    crate::autostart::set(false, &[])?;
    if let Some(dir) = settings {
        match fs::remove_dir_all(dir) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    let retired = paths.dir.join(format!("aster-uninstall-{}.exe", std::process::id()));
    fs::rename(&paths.exe, &retired)?;
    let _ = fs::remove_file(paths.exe.with_extension("exe.old"));
    for slot in 0..=8 {
        let _ = fs::remove_file(paths.exe.with_extension(format!("exe.old{slot}")));
    }
    if let Err(error) = delete_after_exit(&retired) {
        fs::rename(&retired, &paths.exe)?;
        return Err(error);
    }
    shell_registry::delete(UNINSTALL_KEY)?;
    Ok(())
}

pub fn run(action: Action, executable: &Path, version: &str, close_sessions: impl Fn() -> io::Result<()> + Send + Sync + 'static) -> i32 {
    use aster_core::installation;
    let (silent, installing) = match action {
        Action::Install { silent } => (silent, true),
        Action::Uninstall { silent } => (silent, false),
    };
    let result = InstallPaths::current().and_then(|paths| {
        if !silent {
            return wizard::run(installing, paths, executable, version, close_sessions);
        }
        if installing {
            let shortcuts = if paths.exe.is_file() {
                let (start_menu, desktop) = crate::shortcut::installed_shortcuts(&paths.exe);
                Shortcuts { start_menu, desktop }
            } else {
                Shortcuts { start_menu: true, desktop: false }
            };
            install(&paths, executable, version, shortcuts, false)?;
        } else {
            uninstall(&paths, executable, false, close_sessions)?;
        }
        Ok(0)
    });
    match result {
        Ok(code) => code,
        Err(error) => {
            if !silent { show_error(&installation::failed(installing, &error.to_string())); }
            1
        }
    }
}

#[cfg(test)]
#[path = "../test/installer.rs"]
mod tests;
