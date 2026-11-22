use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::windows::fs::MetadataExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_REPARSE_POINT, MOVEFILE_REPLACE_EXISTING, MoveFileExW};
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::System::Threading::CREATE_NO_WINDOW;
use windows::Win32::UI::WindowsAndMessaging::{IDYES, MB_DEFBUTTON2, MB_ICONERROR, MB_ICONINFORMATION, MB_ICONQUESTION, MB_OK, MB_YESNO, MESSAGEBOX_STYLE, MessageBoxW};
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

    fn current() -> io::Result<Self> {
        let root = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).filter(|path| path.is_absolute());
        Ok(Self::new(&root.ok_or_else(|| missing_environment_variable("LOCALAPPDATA"))?))
    }

    fn contains(&self, exe: &Path) -> bool {
        exe.parent().is_some_and(|dir| crate::shortcut::same_path(&self.dir, dir))
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

fn yes(message: &str) -> bool {
    message_box(message, MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2) == IDYES
}

fn message_box(message: &str, style: MESSAGEBOX_STYLE) -> windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_RESULT {
    unsafe { MessageBoxW(None, &HSTRING::from(message), w!("Aster"), style) }
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

fn install(paths: &InstallPaths, source: &Path, version: &str, silent: bool) -> io::Result<()> {
    check_directory(&paths.dir)?;
    crate::instance::stop_for_installation(&paths.exe, (!silent).then_some(source))?;
    if !crate::shortcut::same_path(source, &paths.exe) {
        fs::create_dir_all(&paths.dir)?;
        let stage = paths.dir.join(format!("aster-{}.exe.tmp", std::process::id()));
        let result = fs::copy(source, &stage).and_then(|_| replace_executable(&stage, &paths.exe));
        if result.is_err() { let _ = fs::remove_file(&stage); }
        result?;
    }
    crate::shortcut::apply_for(Some(true), &paths.exe).map_err(io_platform)?;
    shell_registry::write(UNINSTALL_KEY, &uninstall_values(paths, version, fs::metadata(&paths.exe)?.len()))?;
    Ok(())
}

fn system_dir() -> io::Result<PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    let mut buffer = [0u16; 32768];
    let len = unsafe { GetSystemDirectoryW(Some(&mut buffer)) } as usize;
    if len == 0 || len >= buffer.len() { return Err(io::Error::last_os_error()) }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..len])))
}

fn delete_after_exit(dir: &Path, settings: Option<&Path>) -> io::Result<Child> {
    let system = system_dir()?;
    let mut command = Command::new(system.join("cmd.exe"));
    let script = if let Some(settings) = settings {
        command.env("ASTER_DELETE_SETTINGS", settings);
        r#"for /l %i in (1,1,20) do @(ping -n 3 127.0.0.1 >nul & rmdir /s /q "%ASTER_DELETE_INSTALL%" 2>nul & if not exist "%ASTER_DELETE_INSTALL%" (rmdir /s /q "%ASTER_DELETE_SETTINGS%" & exit /b))"#
    } else {
        r#"for /l %i in (1,1,20) do @(ping -n 3 127.0.0.1 >nul & rmdir /s /q "%ASTER_DELETE_INSTALL%" 2>nul & if not exist "%ASTER_DELETE_INSTALL%" exit /b)"#
    };
    command.current_dir(&system).creation_flags(CREATE_NO_WINDOW.0)
        .env("ASTER_DELETE_INSTALL", dir).args(["/d", "/v:off", "/c"]).raw_arg(script).spawn()
}

fn uninstall(paths: &InstallPaths, source: &Path, delete_settings: bool, close_sessions: impl FnOnce() -> io::Result<()>) -> io::Result<Option<PathBuf>> {
    check_directory(&paths.dir)?;
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
    crate::shortcut::apply_for(Some(false), &paths.exe).map_err(io_platform)?;
    crate::jump_list::remove().map_err(io_platform)?;
    let explorer = crate::explorer::set("", false);
    let paste = crate::explorer::set_paste("", false);
    let url = crate::url_scheme::set(false);
    if !(explorer && paste && url) {
        return Err(io::Error::other(aster_core::installation::integration_error()));
    }
    crate::autostart::set(false, &[])?;
    shell_registry::delete(UNINSTALL_KEY)?;
    Ok(settings)
}

pub fn run(action: Action, executable: &Path, version: &str, close_sessions: impl FnOnce() -> io::Result<()>) -> i32 {
    use aster_core::installation;
    let (silent, installing) = match action {
        Action::Install { silent } => (silent, true),
        Action::Uninstall { silent } => (silent, false),
    };
    if !silent && !yes(installation::confirmation(installing)) { return 0 }
    let delete_settings = !silent && !installing && yes(installation::delete_settings());
    let result = InstallPaths::current().and_then(|paths| {
        if installing {
            install(&paths, executable, version, silent)?;
            if !silent {
                message_box(installation::result(true), MB_OK | MB_ICONINFORMATION);
                Command::new(&paths.exe).spawn()?;
            }
        } else {
            let settings = uninstall(&paths, executable, delete_settings, close_sessions)?;
            if !silent { message_box(installation::result(false), MB_OK | MB_ICONINFORMATION); }
            delete_after_exit(&paths.dir, settings.as_deref())?;
        }
        Ok(())
    });
    match result {
        Ok(()) => 0,
        Err(error) => {
            if !silent { message_box(&installation::failed(installing, &error.to_string()), MB_OK | MB_ICONERROR); }
            1
        }
    }
}

#[cfg(test)]
#[path = "../test/installer.rs"]
mod tests;
