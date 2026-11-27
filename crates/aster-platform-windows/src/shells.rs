use aster_core::profile::Profile;
use aster_platform::shell_hooks::{POWERSHELL_MARKS, encode_powershell_command, interactive_powershell_args};
use aster_platform::SpawnRequest;
use std::path::{Path, PathBuf};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::System::Registry::{RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, RRF_RT_REG_SZ};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn reg_string(root: HKEY, key: &str, value: &str) -> Option<String> {
    let (key, value) = (wide(key), wide(value));
    let mut size = 0u32;
    unsafe {
        RegGetValueW(root, PCWSTR(key.as_ptr()), PCWSTR(value.as_ptr()), RRF_RT_REG_SZ, None, None, Some(&mut size)).ok().ok()?;
        let mut buf = vec![0u16; size as usize / 2 + 1];
        RegGetValueW(root, PCWSTR(key.as_ptr()), PCWSTR(value.as_ptr()), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr().cast()), Some(&mut size)).ok().ok()?;
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..n]))
    }
}

fn subkeys(root: HKEY, key: &str) -> Vec<String> {
    let key = wide(key);
    let mut out = Vec::new();
    unsafe {
        let mut h = HKEY::default();
        if RegOpenKeyExW(root, PCWSTR(key.as_ptr()), None, KEY_READ, &mut h).is_err() {
            return out;
        }
        for i in 0.. {
            let mut name = [0u16; 256];
            let mut len = name.len() as u32;
            if RegEnumKeyExW(h, i, Some(PWSTR(name.as_mut_ptr())), &mut len, None, None, None, None).is_err() {
                break;
            }
            out.push(String::from_utf16_lossy(&name[..len as usize]));
        }
        let _ = RegCloseKey(h);
    }
    out
}

fn system32() -> PathBuf {
    let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32")
}

fn on_path(exe: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(exe)).find(|p| p.is_file())
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

pub(crate) struct BashStartup {
    directory: PathBuf,
    file: PathBuf,
}

impl Drop for BashStartup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

pub(crate) fn startup(req: &SpawnRequest) -> Option<(SpawnRequest, Option<BashStartup>)> {
    req.local_environment.as_ref()?;
    let shell = Path::new(&req.command).file_stem()?.to_str()?;
    if shell.eq_ignore_ascii_case("bash")
        && req.args.iter().all(|arg| matches!(arg.as_str(), "-i" | "-l" | "-li" | "-il" | "--login"))
    {
        let mut nonce = [0; 16];
        getrandom::fill(&mut nonce).ok()?;
        let directory = std::env::temp_dir().join(format!("aster-shell-{:032x}", u128::from_le_bytes(nonce)));
        std::fs::create_dir(&directory).ok()?;
        let startup = BashStartup { file: directory.join("bashrc"), directory };
        if let Err(error) = std::fs::write(&startup.file, aster_platform::BASH_MARKS) {
            tracing::warn!("could not prepare shell integration: {error}");
            return None;
        }
        let mut req = req.clone();
        let path = startup.file.to_str()?.replace('\\', "/");
        if req.args.iter().any(|arg| arg != "-i") {
            let spec = req.local_environment.as_ref()?;
            let explicit = spec.profile.keys().chain(spec.global.keys()).chain(req.env.iter().map(|(key, _)| key))
                .any(|key| key.eq_ignore_ascii_case("PROMPT_COMMAND"));
            if explicit { return None; }
            let previous = std::env::var("PROMPT_COMMAND").unwrap_or_default();
            req.env.extend([
                ("ASTER_BASH_RC".to_owned(), path),
                ("ASTER_BASH_PREVIOUS_PROMPT".to_owned(), previous),
                ("PROMPT_COMMAND".to_owned(), aster_platform::BASH_LOGIN_BRIDGE.to_owned()),
            ]);
        } else {
            req.args = vec!["--rcfile".into(), path, "-i".into()];
        }
        return Some((req, Some(startup)));
    }
    if shell.eq_ignore_ascii_case("pwsh") || shell.eq_ignore_ascii_case("powershell") {
        let args = interactive_powershell_args(&req.args)?;
        let mut req = req.clone();
        req.args = args;
        return Some((req, None));
    }
    None
}

fn pwsh() -> Option<PathBuf> {
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    let candidates = [program_files.join(r"PowerShell\7\pwsh.exe"), program_files.join(r"PowerShell\7-preview\pwsh.exe")];
    reg_string(HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\pwsh.exe", "")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| reg_string(HKEY_CURRENT_USER, r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\pwsh.exe", "").map(PathBuf::from).filter(|p| p.is_file()))
        .or_else(|| env_path("LOCALAPPDATA", r"Microsoft\WindowsApps\pwsh.exe"))
        .or_else(|| candidates.into_iter().find(|p| p.is_file()))
        .or_else(|| env_path("ProgramFiles(x86)", r"PowerShell\7\pwsh.exe"))
        .or_else(|| on_path("pwsh.exe"))
}

fn env_path(variable: &str, path: &str) -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os(variable)?).join(path)).filter(|p| p.is_file())
}

fn git_bash() -> Option<PathBuf> {
    let install = reg_string(HKEY_LOCAL_MACHINE, r"SOFTWARE\GitForWindows", "InstallPath").or_else(|| reg_string(HKEY_CURRENT_USER, r"SOFTWARE\GitForWindows", "InstallPath"));
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    install.map(|p| PathBuf::from(p).join(r"bin\bash.exe")).filter(|p| p.is_file()).or_else(|| Some(program_files.join(r"Git\bin\bash.exe")).filter(|p| p.is_file()))
}

fn wsl_distros() -> Vec<String> {
    const LXSS: &str = r"Software\Microsoft\Windows\CurrentVersion\Lxss";
    let mut names: Vec<String> = subkeys(HKEY_CURRENT_USER, LXSS)
        .into_iter()
        .filter_map(|id| reg_string(HKEY_CURRENT_USER, &format!(r"{LXSS}\{id}"), "DistributionName"))
        .filter(|n| !n.starts_with("docker-desktop"))
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names
}

fn powershell(id: &str, name: &str, exe: &Path) -> Profile {
    let hook = encode_powershell_command(POWERSHELL_MARKS);
    let mut profile = Profile::new(id, name, &path_str(exe), &["-NoLogo", "-NoExit", "-EncodedCommand", &hook]);
    profile.icon = Some("powershell".into());
    profile
}

pub fn detect() -> Vec<Profile> {
    let sys = system32();
    let mut out = Vec::new();
    if let Some(p) = pwsh() {
        out.push(powershell("local:pwsh", "PowerShell", &p));
    }
    let windows_ps = [sys.join(r"WindowsPowerShell\v1.0\powershell.exe"), sys.join("powershell.exe")]
        .into_iter().find(|p| p.is_file()).or_else(|| on_path("powershell.exe"));
    if let Some(windows_ps) = &windows_ps {
        out.push(powershell("local:powershell", "Windows PowerShell", windows_ps));
    }
    let mut cmd = Profile::new("local:cmd", "CMD", &path_str(&sys.join("cmd.exe")), &[]);
    cmd.icon = Some("cmd".into());
    cmd.env.insert("PROMPT".into(), r"$$e]9;9;$$P$$e\$$P$$G".into());
    out.push(cmd);
    let wsl = sys.join("wsl.exe");
    if wsl.is_file() {
        for name in wsl_distros() {
            let mut profile = Profile::new(&format!("local:wsl:{name}"), &format!("WSL / {name}"), &path_str(&wsl), &["-d", &name, "--cd", "~"]);
            profile.icon = Some("linux".into());
            out.push(profile);
        }
    }
    if let Some(bash) = git_bash() {
        let mut profile = Profile::new("local:git-bash", "Git Bash", &path_str(&bash), &["--login", "-i"]);
        profile.icon = Some("git".into());
        out.push(profile);
    }
    out.extend(cygwin());
    out.extend(msys2(&sys));
    let vs_roots: Vec<_> = ["ProgramFiles", "ProgramFiles(x86)"].into_iter().filter_map(std::env::var_os)
        .map(|root| PathBuf::from(root).join("Microsoft Visual Studio")).collect();
    out.extend(visual_studio(&sys, &vs_roots));
    if let Some(root) = std::env::var_os("CMDER_ROOT") {
        out.extend(cmder(&sys, Path::new(&root), windows_ps.as_deref()));
    }
    if let Some(profile) = clink(&sys) {
        out.push(profile);
    }
    out
}

fn cmder(sys: &Path, root: &Path, windows_ps: Option<&Path>) -> Vec<Profile> {
    let vendor = root.join("vendor");
    let init = vendor.join("init.bat");
    let mut profiles = Vec::new();
    if init.is_file() {
        let mut profile = Profile::new("local:cmder", "Cmder", &path_str(&sys.join("cmd.exe")), &["/k", &path_str(&init)]);
        profile.icon = Some("cmd".into());
        profile.env.insert("TERM".into(), "cygwin".into());
        profiles.push(profile);
    }
    let script = vendor.join("profile.ps1");
    if let Some(exe) = windows_ps.filter(|_| script.is_file()) {
        let command = encode_powershell_command(&format!(". '{}'\n{POWERSHELL_MARKS}", path_str(&script).replace('\'', "''")));
        let mut profile = Profile::new("local:cmder-powershell", "Cmder PowerShell", &path_str(exe), &["-NoLogo", "-NoProfile", "-NoExit", "-EncodedCommand", &command]);
        profile.icon = Some("powershell".into());
        profiles.push(profile);
    }
    profiles
}

fn clink(sys: &Path) -> Option<Profile> {
    let arch_exe = match std::env::consts::ARCH { "aarch64" => "clink_arm64.exe", "x86" => "clink_x86.exe", _ => "clink_x64.exe" };
    let exe = ["clink.exe", arch_exe].into_iter().find_map(|name| on_path(name).or_else(|| {
        let path = format!(r"clink\{name}");
        env_path("ProgramFiles(x86)", &path).or_else(|| env_path("ProgramFiles", &path))
    }))?;
    let mut profile = Profile::new("local:clink", "CMD (Clink)", &path_str(&sys.join("cmd.exe")), &["/k", &path_str(&exe), "inject"]);
    profile.icon = Some("cmd".into());
    profile.env.insert("WT_SESSION".into(), "0".into());
    Some(profile)
}

fn cygwin() -> Vec<Profile> {
    [(r"SOFTWARE\Cygwin\setup", "local:cygwin", "Cygwin"), (r"SOFTWARE\WOW6432Node\Cygwin\setup", "local:cygwin32", "Cygwin (32 bit)")]
        .into_iter()
        .filter_map(|(key, id, name)| {
            let bash = PathBuf::from(reg_string(HKEY_LOCAL_MACHINE, key, "rootdir")?).join(r"bin\bash.exe");
            bash.is_file().then(|| {
                let mut p = Profile::new(id, name, &path_str(&bash), &["--login", "-i"]);
                p.icon = Some("cygwin".into());
                p
            })
        })
        .collect()
}

fn msys2(sys: &Path) -> Vec<Profile> {
    let drive = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let root = PathBuf::from(format!(r"{drive}\msys64"));
    let script = root.join("msys2_shell.cmd");
    if !script.is_file() {
        return Vec::new();
    }
    let cmd = path_str(&sys.join("cmd.exe"));
    ["msys", "mingw64", "clang64", "ucrt64"]
        .into_iter()
        .map(|env| {
            let flag = format!("-{env}");
            let mut p = Profile::new(&format!("local:msys2:{env}"), &format!("MSYS2 ({})", env.to_uppercase()), &cmd, &["/c", &path_str(&script), "-defterm", "-here", "-no-start", &flag]);
            p.icon = Some("msys2".into());
            p
        })
        .collect()
}

fn visual_studio(sys: &Path, roots: &[PathBuf]) -> Vec<Profile> {
    let mut versions: Vec<_> = roots.iter().filter_map(|root| std::fs::read_dir(root).ok()).flatten().filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_digit())).collect();
    versions.sort_unstable_by(|a, b| b.cmp(a));
    versions.dedup();
    let cmd = path_str(&sys.join("cmd.exe"));
    let mut out = Vec::new();
    for version in versions {
        for edition in ["Enterprise", "Professional", "Community", "BuildTools"] {
            let Some(bat) = roots.iter().map(|r| r.join(&version).join(edition).join(r"Common7\Tools\VsDevCmd.bat")).find(|p| p.is_file()) else { continue };
            let mut p = Profile::new(&format!("local:vs{version}:{}", edition.to_lowercase()), &format!("Developer Prompt for VS {version}"), &cmd, &["/k", &path_str(&bat)]);
            p.icon = Some("vs".into());
            out.push(p);
            break;
        }
    }
    out
}

#[cfg(test)]
#[path = "../test/shells.rs"]
mod tests;
