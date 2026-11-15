use aster_core::profile::Profile;
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

fn pwsh() -> Option<PathBuf> {
    let program_files = std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
    let candidates = [program_files.join(r"PowerShell\7\pwsh.exe"), program_files.join(r"PowerShell\7-preview\pwsh.exe")];
    reg_string(HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\pwsh.exe", "")
        .map(PathBuf::from)
        .filter(|p| p.is_file())
        .or_else(|| candidates.into_iter().find(|p| p.is_file()))
        .or_else(|| on_path("pwsh.exe"))
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

pub fn detect() -> Vec<Profile> {
    let sys = system32();
    let mut out = Vec::new();
    if let Some(p) = pwsh() {
        let mut profile = Profile::new("local:pwsh", "PowerShell", &path_str(&p), &["-nologo"]);
        profile.icon = Some("powershell".into());
        out.push(profile);
    }
    let windows_ps = sys.join(r"WindowsPowerShell\v1.0\powershell.exe");
    if windows_ps.is_file() {
        let mut profile = Profile::new("local:powershell", "Windows PowerShell", &path_str(&windows_ps), &["-nologo"]);
        profile.icon = Some("powershell".into());
        out.push(profile);
    }
    let mut cmd = Profile::new("local:cmd", "CMD", &path_str(&sys.join("cmd.exe")), &[]);
    cmd.icon = Some("cmd".into());
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
    out
}
