#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod handler;
mod session;

use aster_core::config::{Config, SchemeMode};
use aster_platform::{Platform, WindowOptions, WindowSystem};
use std::path::PathBuf;

fn config_path<P: Platform>() -> Option<PathBuf> {
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

fn run<P: Platform>() -> Result<(), aster_platform::PlatformError> {
    let config = load_config::<P>();
    let dark = config.appearance.color_scheme_mode != SchemeMode::Light;
    let windows = P::Windows::new()?;
    let waker = windows.waker();
    let handler = handler::Handler::<P>::new(config, waker);
    windows.run(WindowOptions { title: "Aster".into(), placement: None, dark }, handler)
}

fn main() {
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_max_level(tracing::Level::INFO).init();
    #[cfg(windows)]
    let result = run::<aster_platform_windows::Windows>();
    #[cfg(not(windows))]
    let result: Result<(), aster_platform::PlatformError> = Err(aster_platform::PlatformError::new(-1, "this platform is not supported yet"));
    if let Err(e) = result {
        tracing::error!("{e}");
        std::process::exit(1);
    }
}
