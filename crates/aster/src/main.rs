#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod handler;
mod remote;
mod session;

use aster_core::config::{Config, SchemeMode};
use aster_platform::{Placement, Platform, WindowOptions, WindowSystem};
use std::path::PathBuf;

pub fn config_path<P: Platform>() -> Option<PathBuf> {
    P::config_dir().map(|d| d.join("config.toml"))
}

fn load_config<P: Platform>() -> Config {
    let Some(path) = config_path::<P>() else { return Config::default() };
    match std::fs::read_to_string(&path) {
        Ok(text) => Config::parse(&text).unwrap_or_else(|e| {
            tracing::error!("{}: {e}", path.display());
            Config::default()
        }),
        Err(_) => {
            let config = Config::default();
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&path, config.to_text());
            config
        }
    }
}

pub fn session_path<P: Platform>() -> Option<PathBuf> {
    P::config_dir().map(|d| d.join("session.json"))
}

pub fn state_path<P: Platform>() -> Option<PathBuf> {
    P::config_dir().map(|d| d.join("window.state"))
}

pub fn parse_placement(text: &str) -> Option<Placement> {
    let v: Vec<i32> = text.split_whitespace().filter_map(|n| n.parse().ok()).collect();
    match v.as_slice() {
        [x, y, width, height, maximized] => Some(Placement { x: *x, y: *y, width: *width, height: *height, maximized: *maximized != 0 }),
        _ => None,
    }
}

pub fn format_placement(p: &Placement) -> String {
    format!("{} {} {} {} {}", p.x, p.y, p.width, p.height, u8::from(p.maximized))
}

fn run<P: Platform>() -> Result<(), aster_platform::PlatformError> {
    let config = load_config::<P>();
    let dark = config.appearance.color_scheme_mode != SchemeMode::Light;
    let placement = state_path::<P>().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| parse_placement(&t));
    let windows = P::Windows::new()?;
    let waker = windows.waker();
    let handler = handler::Handler::<P>::new(config, waker);
    windows.run(WindowOptions { title: "Aster".into(), placement, dark }, handler)
}

fn main() {
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_max_level(tracing::Level::INFO).init();
    #[cfg(windows)]
    let result = run::<aster_platform_windows::Windows>();
    #[cfg(target_os = "linux")]
    let result = run::<aster_platform_linux::Linux>();
    if let Err(e) = result {
        tracing::error!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_round_trips_and_rejects_garbage() {
        let p = Placement { x: -8, y: 20, width: 1200, height: 700, maximized: true };
        assert_eq!(parse_placement(&format_placement(&p)), Some(p));
        assert_eq!(parse_placement("1 2 3"), None);
        assert_eq!(parse_placement(""), None);
    }
}
