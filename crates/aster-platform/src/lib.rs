pub mod host;
pub mod shell_hooks;
pub const BASH_MARKS: &str = include_str!("bash_marks.sh");
pub const BASH_LOGIN_BRIDGE: &str = "source \"$ASTER_BASH_RC\"; __aster_prepare_prompt";
pub const ZSH_MARKS: &str = include_str!("zsh_marks.sh");
pub mod monitors;

use aster_core::input::{CursorIcon, Event, HitArea, PasteTarget};
use aster_core::profile::Profile;
use aster_gfx::atlas::Slot;
use aster_gfx::render::{DrawList, Rect};
use aster_gfx::text::TextBackend;
use std::fmt;
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct PlatformError {
    pub code: i64,
    pub message: String,
}

impl PlatformError {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }

    pub fn localized(&self) -> String {
        let code = self.code as u32;
        let raw = if cfg!(windows) && code & 0xffff_0000 == 0x8007_0000 {
            (code & 0xffff) as i32
        } else {
            i32::try_from(self.code).unwrap_or(-1)
        };
        aster_core::i18n::platform_error(std::io::Error::from_raw_os_error(raw).kind(), &self.message)
    }
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (0x{:08X})", self.message, self.code as u32)
    }
}

impl std::error::Error for PlatformError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Surface {
    pub translucent: bool,
    pub backdrop: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DockSpec {
    pub edge: Edge,
    pub fill: f32,
    pub space: f32,
    pub on_top: bool,
    pub screen: aster_core::config::DockScreen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DockSlide {
    pub x: i32,
    pub shown_y: i32,
    pub hidden_y: i32,
}

impl DockSlide {
    pub fn top((x, y, _, height): (i32, i32, i32, i32), monitor_top: i32) -> Self {
        Self { x, shown_y: y, hidden_y: monitor_top.saturating_sub(height) }
    }
}

pub fn dock_rect(work: (i32, i32, i32, i32), spec: DockSpec) -> (i32, i32, i32, i32) {
    let (x, y, w, h) = work;
    let fill = spec.fill.clamp(0.05, 1.0);
    let space = spec.space.clamp(0.2, 1.0);
    match spec.edge {
        Edge::Top | Edge::Bottom => {
            let (dw, dh) = ((w as f32 * space).round() as i32, (h as f32 * fill).round() as i32);
            let dx = x + (w - dw) / 2;
            let dy = if spec.edge == Edge::Top { y } else { y + h - dh };
            (dx, dy, dw, dh)
        }
        Edge::Left | Edge::Right => {
            let (dw, dh) = ((w as f32 * fill).round() as i32, (h as f32 * space).round() as i32);
            let dy = y + (h - dh) / 2;
            let dx = if spec.edge == Edge::Left { x } else { x + w - dw };
            (dx, dy, dw, dh)
        }
    }
}

#[derive(Clone, Debug)]
pub struct WindowOptions {
    pub title: String,
    pub placement: Option<Placement>,
    pub dark: bool,
    pub visible: bool,
}

pub type WindowId = u64;

pub trait Waker: Clone + Send + Sync + 'static {
    fn wake(&self);
}

pub trait Window {
    fn id(&self) -> WindowId;
    fn accessibility_active(&self) -> bool { false }
    fn update_accessibility(&self, _tree: aster_core::accessibility::Tree) {}
    fn size(&self) -> (u32, u32);
    fn scale(&self) -> f32;
    fn request_redraw(&self);
    fn set_title(&self, title: &str);
    fn set_cursor(&self, cursor: CursorIcon);
    fn minimize(&self);
    fn toggle_maximize(&self);
    fn is_maximized(&self) -> bool;
    fn toggle_fullscreen(&self);
    fn is_fullscreen(&self) -> bool;
    fn close(&self);
    fn request_quit(&self) {}
    fn activate(&self);
    fn open_window(&self, opts: WindowOptions);
    fn placement(&self) -> Placement;
    fn clipboard_text(&self, target: Option<PasteTarget>) -> Option<String>;
    fn set_clipboard_text(&self, text: &str);
    fn set_clipboard_html(&self, text: &str, html: &str);
    fn set_ime_area(&self, area: Rect);
    fn is_dark_mode(&self) -> bool;
    fn set_dark(&self, dark: bool);
    fn set_timer(&self, ms: u32);
    fn request_attention(&self);
    fn notify(&self, _notification: aster_core::notifications::Notification) {}
    fn set_surface(&self, backdrop: aster_core::config::Backdrop, blur: f32) -> Surface;
    fn set_visible(&self, visible: bool);
    fn is_visible(&self) -> bool;
    fn is_foreground(&self) -> bool;
    fn dock(&self, spec: Option<DockSpec>);
    fn dock_slide(&self) -> Option<DockSlide> { None }
    fn screen_position(&self) -> Option<(i32, i32)> { None }
    fn set_position(&self, _x: i32, _y: i32) -> bool { false }
    fn set_global_hotkey(&self, key: Option<(aster_core::input::Key, aster_core::input::Mods)>) -> bool;
    fn set_progress(&self, progress: Option<aster_core::app::Progress>);
}

pub trait WindowHandler<W: WindowSystem> {
    fn tray_enabled(&self) -> bool { false }
    fn quitting(&mut self, _quitting: bool) {}
    fn confirming_close(&self, _window: &W::Window) -> bool { false }
    fn background_woken(&mut self) -> bool { false }
    fn created(&mut self, window: &W::Window);
    fn event(&mut self, window: &W::Window, event: Event);
    fn hit_test(&mut self, window: &W::Window, x: f32, y: f32) -> HitArea;
    fn redraw(&mut self, window: &W::Window);
    fn woken(&mut self, window: &W::Window);
    fn destroyed(&mut self, window: &W::Window);
}

pub trait WindowSystem: Sized + 'static {
    type Window: Window;
    type Waker: Waker;
    fn new() -> Result<Self, PlatformError>;
    fn waker(&self) -> Self::Waker;
    fn run<H: WindowHandler<Self>>(self, first: WindowOptions, handler: H) -> Result<(), PlatformError>;
}

#[derive(Debug)]
pub enum RenderError {
    DeviceLost,
    Other(PlatformError),
}

pub trait Renderer: Sized {
    type Window;
    type Device: Send + 'static;
    fn device(software: bool) -> Result<Self::Device, PlatformError>;
    fn new(device: &Self::Device, window: &Self::Window, atlas_size: u32) -> Result<Self, PlatformError>;
    fn resize(&mut self, width: u32, height: u32);
    fn upload(&mut self, slot: Slot, rgba: &[u8]);
    fn render(&mut self, list: &DrawList) -> Result<(), RenderError>;
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpawnRequest {
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub local_environment: Option<Box<aster_core::environment::LocalEnvironment>>,
    pub cols: u16,
    pub rows: u16,
}

pub trait Pty: Send + Sync {
    fn resize(&self, cols: u16, rows: u16);
    fn force_resize(&self, cols: u16, rows: u16) {
        self.resize(if cols > 2 { cols - 1 } else { cols + 1 }, rows);
        self.resize(cols, rows);
    }
    fn kill(&self);
    fn pid(&self) -> u32;
    fn host_session_id(&self) -> Option<u64> { None }
    fn detach(&self) {}
    fn wait_host_close(&self, _timeout: std::time::Duration) -> bool { true }
}

pub struct Spawned {
    pub pty: Box<dyn Pty>,
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub raw: Option<Box<dyn Write + Send>>,
    pub wait: Box<dyn FnOnce() -> Option<u32> + Send>,
}

pub const INSTALLATION_QUIT: &str = "aster-installation-quit";

pub trait Platform: 'static {
    type Windows: WindowSystem;
    type Renderer: Renderer<Window = <Self::Windows as WindowSystem>::Window>;
    type Text: TextBackend;
    fn text() -> Result<Self::Text, PlatformError>;
    fn font_families(monospace: bool) -> Vec<String>;
    fn shells() -> Vec<Profile>;
    fn spawn(req: &SpawnRequest) -> Result<Spawned, PlatformError>;
    fn attach_delegated_console(_token: &str) -> Result<Spawned, PlatformError> {
        Err(PlatformError::new(0, "console delegation is unsupported"))
    }
    fn spawn_extension(command: &mut std::process::Command) -> std::io::Result<std::process::Child>;
    fn config_dir() -> Option<PathBuf>;
    fn home_dir() -> Option<PathBuf>;
    fn open(target: &str);

    fn reveal_file(path: &std::path::Path) {
        if let Some(parent) = path.parent() {
            Self::open(&parent.to_string_lossy());
        }
    }
    fn ssh_command() -> Option<String>;
    fn process_cwd(pid: u32) -> Option<String>;
    fn child_processes(pid: u32) -> Vec<String>;
    fn instance_listen(wake: std::sync::Arc<dyn Fn() + Send + Sync>) -> Option<std::sync::mpsc::Receiver<String>>;
    fn instance_send(message: &str) -> bool;
    fn delegated_instance_listen(_wake: std::sync::Arc<dyn Fn() + Send + Sync>) -> Option<std::sync::mpsc::Receiver<String>> { None }
    fn delegated_instance_send(_message: &str) -> bool { false }
    fn set_explorer_menu(label: &str, enabled: bool) -> bool;
    fn update_launcher(_profiles: &[String], _install: Option<bool>) -> std::io::Result<bool> { Ok(false) }
    fn set_autostart(enabled: bool, args: &[&str]) -> std::io::Result<()>;
    fn set_explorer_paste_menu(_label: &str, _enabled: bool) -> bool { true }
    fn start_menu_shortcut(_change: Option<bool>) -> Result<bool, PlatformError> { Ok(false) }
    fn installation_state() -> aster_core::installation::State { Default::default() }
    fn default_terminal_state() -> Result<aster_core::default_terminal::State, PlatformError> { Ok(Default::default()) }
    fn set_default_terminal(_enabled: bool) -> Result<aster_core::default_terminal::State, PlatformError> { Ok(Default::default()) }
    fn start_user_install() -> std::io::Result<()> { Err(std::io::ErrorKind::Unsupported.into()) }
    fn update_install_registration(_path: &std::path::Path, _version: &str) -> std::io::Result<()> { Ok(()) }
    fn set_url_scheme(_enabled: bool) -> bool { true }
    fn set_jump_list(_list: &aster_core::jump_list::JumpList) -> Result<(), PlatformError> { Ok(()) }
    fn beep() -> bool;
    fn can_elevate() -> bool;
    fn spawn_elevated(req: &SpawnRequest) -> Result<Spawned, PlatformError>;
    fn connect_host(base: &str) -> Result<(host::Reader, host::Writer), PlatformError>;
    fn pty_host_endpoint() -> std::io::Result<String>;
    fn listen_pty_host(endpoint: &str) -> std::io::Result<Box<dyn host::persistent::Listener>>;
    fn connect_pty_host(endpoint: &str) -> std::io::Result<host::persistent::Connection>;
    fn start_pty_host(endpoint: &str) -> std::io::Result<()>;
    fn create_pty_host_token(path: &std::path::Path) -> std::io::Result<std::fs::File>;
    fn downloads_dir() -> Option<PathBuf>;
    fn secret_get(key: &str) -> Option<String>;
    fn secret_set(key: &str, value: &str) -> bool;
    fn secret_delete(key: &str);
    fn ui_language() -> String;
    fn download(url: &str, out: &mut dyn Write, progress: &mut dyn FnMut(u64, Option<u64>) -> bool) -> Result<(), PlatformError>;
    fn wait_exit(pid: u32, limit: std::time::Duration);
    fn run_elevated(args: &[String]) -> Result<u32, PlatformError>;
    fn delete_at_reboot(path: &std::path::Path);
}

#[cfg(test)]
#[path = "../test/dock.rs"]
mod tests;
