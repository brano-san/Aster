#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod background;
mod cli;
mod config_file;
mod desktop;
mod extension_package;
mod extension_process;
mod extensions;
mod handler;
mod log;
mod local_spawn;
mod net;
mod pty_host;
mod quake;
mod remote;
mod secrets;
mod serial;
mod session;
mod shell_integration;
mod stream;
mod sync;
mod update;
mod url_scheme;
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

fn load_config<P: Platform>() -> (Config, bool) {
    let Some(path) = config_path::<P>() else { return (Config::default(), false) };
    match config_file::read_initial(&path) {
        Ok(text) if aster_vault::config::is_encrypted(&text) => (Config::default(), true),
        Ok(text) => (Config::parse(&text).map(|mut config| {
            config.vault.encrypt_config = false;
            config
        }).unwrap_or_else(|e| {
            tracing::error!("{}: {e}", path.display());
            Config::default()
        }), false),
        Err(error) => {
            tracing::error!("{}: {error}", path.display());
            (Config::default(), false)
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
    let system_language = P::ui_language();
    aster_core::i18n::set_language(aster_core::i18n::resolve("auto", &system_language));
    update::on_start::<P>();
    let windows = P::Windows::new()?;
    let waker = windows.waker();
    let alone = std::env::var_os("ASTER_CAPTURE").is_some() && std::env::var_os("ASTER_INSTANCE").is_none();
    let delegated = matches!(request.command, cli::Command::DelegatedConsole { .. });
    let wake = waker.clone();
    let requests = if alone { None } else { P::instance_listen(std::sync::Arc::new(move || wake.wake())) };
    if !alone && !delegated && requests.is_none() && P::instance_send(&request.encode()) {
        return Ok(());
    }
    let wake = waker.clone();
    let delegated_requests = if alone { None } else { P::delegated_instance_listen(std::sync::Arc::new(move || wake.wake())) };
    if !alone && delegated && delegated_requests.is_none() && P::delegated_instance_send(&request.encode()) {
        return Ok(());
    }
    let (config, encrypted) = load_config::<P>();
    aster_core::i18n::set_language(aster_core::i18n::resolve(&config.appearance.language, &system_language));
    let hidden = !encrypted && starts_hidden(&config, &request.command) && std::env::var_os("ASTER_CAPTURE").is_none() && (cfg!(windows) || requests.is_some());
    if !hidden && request.command == cli::Command::Hidden {
        request.command = cli::Command::Window;
    }
    let dark = config.appearance.color_scheme_mode != SchemeMode::Light;
    let placement = state_path::<P>().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| parse_placement(&t));
    let (mut handler, vault_worker, extensions, local_worker) = handler::Handler::<P>::new(config, waker, encrypted)
        .map_err(|e| aster_platform::PlatformError::new(0, format!("vault worker: {e}")))?;
    handler.set_instance(requests, delegated_requests, (!hidden || cfg!(windows)).then_some(request));
    let result = windows.run(WindowOptions { title: "Aster".into(), placement, dark, visible: !hidden }, handler);
    local_worker.finish();
    drop(extensions);
    drop(vault_worker);
    update::relaunch_if_requested();
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
    #[cfg(windows)]
    if args.get(1).is_some_and(|arg| arg == aster_platform_windows::defterm::SERVER_ARG) {
        use aster_platform_windows::{defterm, Windows};
        let log = log::open(log_path::<Windows>().as_deref());
        tracing_subscriber::fmt().with_ansi(false).with_writer(move || log.clone()).init();
        if let Err(error) = defterm::run_server() {
            tracing::error!("terminal delegation: {error}");
            std::process::exit(1);
        }
        return;
    }
    if let (Some("--pty-host"), Some(endpoint)) = (args.get(1).map(String::as_str), args.get(2)) {
        #[cfg(windows)]
        let code = pty_host::run::<aster_platform_windows::Windows>(endpoint);
        #[cfg(target_os = "linux")]
        let code = pty_host::run::<aster_platform_linux::Linux>(endpoint);
        std::process::exit(code);
    }
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
    #[cfg(windows)]
    if let Ok(exe) = std::env::current_exe() {
        if let Some(action) = aster_platform_windows::installer::parse(&args[1..], Some(&exe)) {
            use aster_platform_windows::Windows;
            let config = config_path::<Windows>().and_then(|path| std::fs::read_to_string(path).ok()).and_then(|text| Config::parse(&text).ok());
            let language = config.as_ref().map_or("auto", |config| config.appearance.language.as_str());
            aster_core::i18n::set_language(aster_core::i18n::resolve(language, &Windows::ui_language()));
            let Ok(action) = action else {
                say(aster_core::installation::usage());
                std::process::exit(2);
            };
            std::process::exit(aster_platform_windows::installer::run(action, &exe, env!("CARGO_PKG_VERSION"), pty_host::close_for_uninstall::<Windows>));
        }
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
        tracing::error!("{} (0x{:08X})", e.localized(), e.code as u32);
        std::process::exit(1);
    }
}

#[cfg(test)]
#[path = "../test/main.rs"]
mod tests;
