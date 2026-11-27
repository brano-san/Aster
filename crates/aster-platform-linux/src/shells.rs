use aster_core::profile::Profile;
use aster_platform::SpawnRequest;
use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

pub(crate) struct ShellStartup {
    directory: PathBuf,
    file: PathBuf,
}

impl Drop for ShellStartup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

pub(crate) fn startup(req: &SpawnRequest, args: &mut Vec<String>, env: &mut Vec<CString>) -> Option<ShellStartup> {
    req.local_environment.as_ref()?;
    let shell = Path::new(&req.command).file_name()?.to_str()?;
    if matches!(shell, "pwsh" | "powershell") {
        if let Some(integrated) = aster_platform::shell_hooks::interactive_powershell_args(args) {
            *args = integrated;
        }
        return None;
    }
    if !args.iter().all(|arg| matches!(arg.as_str(), "-i" | "-l" | "-il" | "-li" | "--login")) {
        return None;
    }
    let (file, script) = match shell {
        "bash" => ("bashrc", aster_platform::BASH_MARKS),
        "zsh" => (".zshenv", aster_platform::ZSH_MARKS),
        _ => return None,
    };
    let prefix = std::env::temp_dir().join("aster-shell-XXXXXX");
    let mut directory = CString::new(prefix.as_os_str().as_bytes()).ok()?.into_bytes_with_nul();
    if unsafe { libc::mkdtemp(directory.as_mut_ptr().cast()) }.is_null() {
        return None;
    }
    directory.pop();
    let directory = PathBuf::from(std::ffi::OsStr::from_bytes(&directory));
    let startup = ShellStartup { file: directory.join(file), directory };
    if let Err(error) = std::fs::write(&startup.file, script) {
        tracing::warn!("could not prepare shell integration: {error}");
        return None;
    }
    if shell == "bash" {
        let path = startup.file.to_str()?.to_owned();
        let login = args.iter().any(|arg| arg != "-i");
        if login {
            let previous = env.iter().find_map(|entry| entry.as_bytes().strip_prefix(b"PROMPT_COMMAND=")).map(<[u8]>::to_vec);
            set_env(env, b"ASTER_BASH_RC", path.as_bytes());
            set_env(env, b"ASTER_BASH_PREVIOUS_PROMPT", previous.as_deref().unwrap_or_default());
            set_env(env, b"PROMPT_COMMAND", aster_platform::BASH_LOGIN_BRIDGE.as_bytes());
        } else {
            *args = vec!["--rcfile".into(), path, "-i".into()];
        }
    } else {
        let previous = env.iter().find_map(|entry| entry.as_bytes().strip_prefix(b"ZDOTDIR=")).map(<[u8]>::to_vec);
        if let Some(previous) = previous {
            set_env(env, b"ASTER_ZDOTDIR_SET", b"1");
            set_env(env, b"ASTER_ZDOTDIR", &previous);
        } else {
            set_env(env, b"ASTER_ZDOTDIR_SET", b"0");
        }
        set_env(env, b"ZDOTDIR", startup.directory.as_os_str().as_bytes());
    }
    Some(startup)
}

fn set_env(env: &mut Vec<CString>, name: &[u8], value: &[u8]) {
    let entry = [name, b"=", value].concat();
    env.retain(|s| s.as_bytes().strip_prefix(name).is_none_or(|suffix| !suffix.starts_with(b"=")));
    if let Ok(entry) = CString::new(entry) {
        env.push(entry);
    }
}

fn login_shell() -> Option<String> {
    let from_passwd = unsafe {
        let pw = libc::getpwuid(libc::getuid());
        (!pw.is_null() && !(*pw).pw_shell.is_null()).then(|| std::ffi::CStr::from_ptr((*pw).pw_shell).to_string_lossy().into_owned())
    };
    from_passwd.filter(|s| Path::new(s).is_file()).or_else(|| std::env::var("SHELL").ok().filter(|s| Path::new(s).is_file()))
}

pub fn listed_shells(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines().map(str::trim).filter(|l| l.starts_with('/')) {
        let name = Path::new(line).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !out.iter().any(|s| Path::new(s).file_name().map(|n| n.to_string_lossy() == name).unwrap_or(false)) {
            out.push(line.to_owned());
        }
    }
    out
}

pub fn detect() -> Vec<Profile> {
    let mut out = Vec::new();
    let default = login_shell().unwrap_or_else(|| "/bin/sh".into());
    let name = Path::new(&default).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "sh".into());
    out.push(Profile::new("local:default", &format!("User default ({name})"), &default, &["-l"]));
    let listed = std::fs::read_to_string("/etc/shells").or_else(|_| std::fs::read_to_string("/usr/share/defaults/etc/shells")).unwrap_or_default();
    for shell in listed_shells(&listed).into_iter().filter(|s| Path::new(s).is_file()) {
        let base = Path::new(&shell).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if base == name || base == "nologin" || base == "false" {
            continue;
        }
        out.push(Profile::new(&format!("local:{base}"), &base, &shell, &["-l"]));
    }
    out
}
