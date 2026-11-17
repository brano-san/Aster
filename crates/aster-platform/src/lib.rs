pub mod host;

use aster_core::input::{CursorIcon, Event, HitArea};
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
}

pub type WindowId = u64;

pub trait Waker: Clone + Send + Sync + 'static {
    fn wake(&self);
}

pub trait Window {
    fn id(&self) -> WindowId;
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
    fn activate(&self);
    fn open_window(&self, opts: WindowOptions);
    fn placement(&self) -> Placement;
    fn clipboard_text(&self) -> Option<String>;
    fn set_clipboard_text(&self, text: &str);
    fn set_ime_area(&self, area: Rect);
    fn is_dark_mode(&self) -> bool;
    fn set_dark(&self, dark: bool);
    fn set_timer(&self, ms: u32);
    fn request_attention(&self);
    fn set_surface(&self, backdrop: aster_core::config::Backdrop) -> Surface;
    fn set_visible(&self, visible: bool);
    fn is_visible(&self) -> bool;
    fn is_foreground(&self) -> bool;
    fn dock(&self, spec: Option<DockSpec>);
    fn set_global_hotkey(&self, key: Option<(aster_core::input::Key, aster_core::input::Mods)>) -> bool;
    fn set_progress(&self, progress: Option<aster_core::app::Progress>);
}

pub trait WindowHandler<W: WindowSystem> {
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
    pub cols: u16,
    pub rows: u16,
}

pub trait Pty: Send + Sync {
    fn resize(&self, cols: u16, rows: u16);
    fn kill(&self);
    fn pid(&self) -> u32;
}

pub struct Spawned {
    pub pty: Box<dyn Pty>,
    pub reader: Box<dyn Read + Send>,
    pub writer: Box<dyn Write + Send>,
    pub raw: Option<Box<dyn Write + Send>>,
    pub wait: Box<dyn FnOnce() -> Option<u32> + Send>,
}

pub trait Platform: 'static {
    type Windows: WindowSystem;
    type Renderer: Renderer<Window = <Self::Windows as WindowSystem>::Window>;
    type Text: TextBackend;
    fn text() -> Result<Self::Text, PlatformError>;
    fn shells() -> Vec<Profile>;
    fn spawn(req: &SpawnRequest) -> Result<Spawned, PlatformError>;
    fn config_dir() -> Option<PathBuf>;
    fn home_dir() -> Option<PathBuf>;
    fn open(target: &str);
    fn ssh_command() -> Option<String>;
    fn process_cwd(pid: u32) -> Option<String>;
    fn child_processes(pid: u32) -> Vec<String>;
    fn instance_listen(wake: std::sync::Arc<dyn Fn() + Send + Sync>) -> Option<std::sync::mpsc::Receiver<String>>;
    fn instance_send(message: &str) -> bool;
    fn beep() -> bool;
    fn can_elevate() -> bool;
    fn spawn_elevated(req: &SpawnRequest) -> Result<Spawned, PlatformError>;
    fn connect_host(base: &str) -> Result<(host::Reader, host::Writer), PlatformError>;
    fn downloads_dir() -> Option<PathBuf>;
    fn secret_get(key: &str) -> Option<String>;
    fn secret_set(key: &str, value: &str) -> bool;
    fn secret_delete(key: &str);
    fn ui_language() -> String;
}

#[cfg(test)]
#[path = "../test/dock.rs"]
mod tests;
