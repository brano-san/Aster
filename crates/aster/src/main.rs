#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cli;
mod config_file;
mod desktop;
mod handler;
mod log;
mod net;
mod remote;
mod secrets;
mod serial;
mod session;
mod stream;
mod update;
mod zmodem;

use aster_core::config::{Config, SchemeMode};
use aster_platform::{Placement, Platform, Waker, WindowOptions, WindowSystem};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn choose_config_dir(executable: Option<&Path>, fallback: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    executable.and_then(Path::parent).map(|dir| dir.join("data")).filter(|dir| dir.is_dir()).or_else(fallback)
}

fn config_dir<P: Platform>() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| choose_config_dir(std::env::current_exe().ok().as_deref(), P::config_dir)).as_deref()
}

pub fn config_path<P: Platform>() -> Option<PathBuf> {
    config_dir::<P>().map(|d| d.join("config.toml"))
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
    config_dir::<P>().map(|d| d.join("session.json"))
}

pub fn state_path<P: Platform>() -> Option<PathBuf> {
    config_dir::<P>().map(|d| d.join("window.state"))
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

pub fn log_path<P: Platform>() -> Option<PathBuf> {
    config_dir::<P>().map(|d| d.join("aster.log"))
}

fn starts_hidden(config: &Config, command: &cli::Command) -> bool {
    *command == cli::Command::Hidden && background_mode(config)
}

fn background_mode(config: &Config) -> bool {
    config.appearance.tray || config.appearance.dock != aster_core::config::Dock::Off
}

fn run<P: Platform>(mut request: cli::Request) -> Result<(), aster_platform::PlatformError> {
    let log = log::open(log_path::<P>().as_deref());
    tracing_subscriber::fmt().with_ansi(false).with_writer(move || log.clone()).with_max_level(tracing::Level::INFO).init();
    tracing::info!("Aster {} on {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH);
    update::on_start::<P>();
    let windows = P::Windows::new()?;
    let waker = windows.waker();
    let alone = std::env::var_os("ASTER_CAPTURE").is_some() && std::env::var_os("ASTER_INSTANCE").is_none();
    let wake = waker.clone();
    let requests = if alone { None } else { P::instance_listen(std::sync::Arc::new(move || wake.wake())) };
    if !alone && requests.is_none() && P::instance_send(&request.encode()) {
        return Ok(());
    }
    let config = load_config::<P>();
    let hidden = starts_hidden(&config, &request.command) && std::env::var_os("ASTER_CAPTURE").is_none() && (cfg!(windows) || requests.is_some());
    if !hidden && request.command == cli::Command::Hidden {
        request.command = cli::Command::Window;
    }
    let dark = config.appearance.color_scheme_mode != SchemeMode::Light;
    let placement = state_path::<P>().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| parse_placement(&t));
    let (mut handler, _vault_worker) = handler::Handler::<P>::new(config, waker)
        .map_err(|e| aster_platform::PlatformError::new(0, format!("vault worker: {e}")))?;
    handler.set_instance(requests, (!hidden || cfg!(windows)).then_some(request));
    let result = windows.run(WindowOptions { title: "Aster".into(), placement, dark, visible: !hidden }, handler);
    update::relaunch_if_installed();
    result
}

fn say(text: &str) {
    #[cfg(windows)]
    aster_platform_windows::attach_console();
    println!("{text}");
}

fn host<P: Platform>(base: &str, token: &str) -> i32 {
    tracing_subscriber::fmt().with_writer(std::io::stderr).with_max_level(tracing::Level::INFO).init();
    match P::connect_host(base) {
        Ok((reader, writer)) => match aster_platform::host::serve(reader, writer, token, &|req| P::spawn(req)) {
            Ok(()) => 0,
            Err(e) => {
                tracing::error!("terminal host: {e}");
                1
            }
        },
        Err(e) => {
            tracing::error!("terminal host: {e}");
            1
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let (Some("--host"), Some(base), Some(token)) = (args.get(1).map(String::as_str), args.get(2), args.get(3)) {
        #[cfg(windows)]
        let code = host::<aster_platform_windows::Windows>(base, token);
        #[cfg(target_os = "linux")]
        let code = host::<aster_platform_linux::Linux>(base, token);
        std::process::exit(code);
    }
    if args.get(1).map(String::as_str) == Some(update::INSTALL_ELEVATED) {
        #[cfg(windows)]
        let code = update::install_from_command_line::<aster_platform_windows::Windows>(args.get(2), args.get(3));
        #[cfg(target_os = "linux")]
        let code = update::install_from_command_line::<aster_platform_linux::Linux>(args.get(2), args.get(3));
        std::process::exit(code);
    }
    let cwd = std::env::current_dir().ok();
    let command = match cli::parse(&args[1..], cwd.as_deref()) {
        cli::Parsed::Request(command) => command,
        cli::Parsed::Version => {
            say(&format!("Aster {}", env!("CARGO_PKG_VERSION")));
            return;
        }
        cli::Parsed::Help => {
            say(cli::USAGE);
            return;
        }
        cli::Parsed::Error(message) => {
            say(&format!("{message}\n\n{}", cli::USAGE));
            std::process::exit(2);
        }
    };
    let request = cli::Request { cwd: cwd.map(|d| d.to_string_lossy().into_owned()), command };
    #[cfg(windows)]
    let result = run::<aster_platform_windows::Windows>(request);
    #[cfg(target_os = "linux")]
    let result = run::<aster_platform_linux::Linux>(request);
    if let Err(e) = result {
        tracing::error!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
#[path = "../test/main.rs"]
mod tests;
