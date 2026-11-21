#![cfg(target_os = "linux")]

mod fonts;
mod gl;
mod instance;
mod pty;
mod shells;
mod text;
mod update;
mod window;

pub use gl::{GlDevice, GlRenderer};
pub use text::LinuxText;
pub use window::{LinuxWaker, LinuxWindow, LinuxWindows};

use aster_core::profile::Profile;
use aster_gfx::atlas::Slot;
use aster_gfx::render::DrawList;
use aster_platform::{Platform, PlatformError, RenderError, Renderer, SpawnRequest, Spawned, Window};
use std::path::PathBuf;
use std::sync::Arc;
use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};

pub struct Linux;

impl Platform for Linux {
    type Windows = LinuxWindows;
    type Renderer = LinuxRenderer;
    type Text = LinuxText;

    fn text() -> Result<LinuxText, PlatformError> {
        Ok(LinuxText::new())
    }

    fn font_families(monospace: bool) -> Vec<String> {
        fonts::fonts().wait().fc.as_ref().map(|fc| fc.families(monospace)).unwrap_or_default()
    }

    fn shells() -> Vec<Profile> {
        shells::detect()
    }

    fn spawn(req: &SpawnRequest) -> Result<Spawned, PlatformError> {
        pty::spawn(req)
    }

    fn config_dir() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).filter(|p| p.is_absolute()).or_else(|| Self::home_dir().map(|h| h.join(".config")))?;
        Some(base.join("aster"))
    }

    fn home_dir() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }

    fn process_cwd(pid: u32) -> Option<String> {
        std::fs::read_link(format!("/proc/{pid}/cwd")).ok().map(|p| p.to_string_lossy().into_owned())
    }

    fn child_processes(pid: u32) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
        entries
            .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<u32>().ok())
            .filter_map(|child| {
                let stat = std::fs::read_to_string(format!("/proc/{child}/stat")).ok()?;
                let (name, rest) = stat.split_once('(').and_then(|(_, r)| r.rsplit_once(')'))?;
                let parent: u32 = rest.split_whitespace().nth(1)?.parse().ok()?;
                (parent == pid).then(|| name.to_owned())
            })
            .collect()
    }

    fn instance_listen(wake: std::sync::Arc<dyn Fn() + Send + Sync>) -> Option<std::sync::mpsc::Receiver<String>> {
        instance::listen(wake)
    }

    fn set_explorer_menu(_label: &str, _enabled: bool) -> bool {
        false
    }

    fn instance_send(message: &str) -> bool {
        instance::send(message)
    }

    fn downloads_dir() -> Option<PathBuf> {
        let home = Self::home_dir()?;
        let config = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
        let listed = std::fs::read_to_string(config.join("user-dirs.dirs")).ok().and_then(|text| {
            text.lines().find_map(|l| l.trim().strip_prefix("XDG_DOWNLOAD_DIR=")).map(|v| v.trim_matches('"').replace("$HOME", &home.to_string_lossy()))
        });
        Some(listed.map(PathBuf::from).unwrap_or_else(|| home.join("Downloads")))
    }

    fn secret_get(key: &str) -> Option<String> {
        let out = std::process::Command::new("secret-tool").args(["lookup", "service", "aster", "account", key]).stderr(std::process::Stdio::null()).output().ok()?;
        let value = String::from_utf8(out.stdout).ok()?;
        (out.status.success() && !value.is_empty()).then_some(value)
    }

    fn secret_set(key: &str, value: &str) -> bool {
        use std::io::Write;
        let child = std::process::Command::new("secret-tool")
            .args(["store", &format!("--label=Aster {key}"), "service", "aster", "account", key])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        let Ok(mut child) = child else { return false };
        let wrote = child.stdin.take().is_some_and(|mut i| i.write_all(value.as_bytes()).is_ok());
        wrote && child.wait().is_ok_and(|s| s.success())
    }

    fn secret_delete(key: &str) {
        let _ = std::process::Command::new("secret-tool").args(["clear", "service", "aster", "account", key]).stderr(std::process::Stdio::null()).status();
    }

    fn can_elevate() -> bool {
        false
    }

    fn spawn_elevated(_req: &SpawnRequest) -> Result<Spawned, PlatformError> {
        Err(PlatformError::new(-1, "Administrator tabs are only available on Windows"))
    }

    fn connect_host(_base: &str) -> Result<(aster_platform::host::Reader, aster_platform::host::Writer), PlatformError> {
        Err(PlatformError::new(-1, "The terminal host is only available on Windows"))
    }

    fn ui_language() -> String {
        ["LC_ALL", "LC_MESSAGES", "LANG"].iter().filter_map(|v| std::env::var(v).ok()).find(|v| !v.trim().is_empty()).unwrap_or_else(|| "en".to_owned())
    }

    fn beep() -> bool {
        false
    }

    fn ssh_command() -> Option<String> {
        std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join("ssh")).find(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned())
    }

    fn download(url: &str, out: &mut dyn std::io::Write, progress: &mut dyn FnMut(u64, Option<u64>) -> bool) -> Result<(), PlatformError> {
        update::download(url, out, progress)
    }

    fn wait_exit(pid: u32, limit: std::time::Duration) {
        update::wait_exit(pid, limit)
    }

    fn run_elevated(_args: &[String]) -> Result<u32, PlatformError> {
        Err(PlatformError::new(-1, "Installing with administrator rights is only available on Windows"))
    }

    fn delete_at_reboot(_path: &std::path::Path) {}

    fn open(target: &str) {
        let target = target.to_owned();
        let _ = std::thread::Builder::new().name("aster-open".into()).spawn(move || {
            if let Err(e) = std::process::Command::new("xdg-open").arg(&target).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status() {
                tracing::warn!("xdg-open {target}: {e}");
            }
        });
    }
}

pub struct LinuxRenderer {
    gl: GlRenderer,
    window: Arc<dyn winit::window::Window>,
}

fn handle_error(e: impl std::fmt::Display) -> PlatformError {
    PlatformError::new(-1, format!("window handle: {e}"))
}

impl Renderer for LinuxRenderer {
    type Window = LinuxWindow;
    type Device = GlDevice;

    fn device(software: bool) -> Result<GlDevice, PlatformError> {
        if software {
            return Err(PlatformError::new(-1, "OpenGL ES has no separate software device, Mesa picks the driver for the display"));
        }
        let display = window::display().ok_or_else(|| PlatformError::new(-1, "no display connection"))?;
        GlDevice::new(display.display_handle().map_err(handle_error)?.as_raw())
    }

    fn new(device: &GlDevice, window: &LinuxWindow, atlas_size: u32) -> Result<Self, PlatformError> {
        let raw = window.winit().clone();
        let gl = GlRenderer::new(device, raw.window_handle().map_err(handle_error)?.as_raw(), window.size(), atlas_size)?;
        if let Err(e) = gl.set_swap_interval(if window.is_wayland() { 0 } else { 1 }) {
            tracing::warn!("swap interval: {e:?}");
        }
        Ok(Self { gl, window: raw })
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.gl.resize(width, height);
    }

    fn upload(&mut self, slot: Slot, rgba: &[u8]) {
        self.gl.upload(slot, rgba);
    }

    fn render(&mut self, list: &DrawList) -> Result<(), RenderError> {
        self.window.pre_present_notify();
        self.gl.render(list)
    }
}
