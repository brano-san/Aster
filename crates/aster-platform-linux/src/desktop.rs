use std::fmt::Write;
use std::fs;
use std::io::{self, ErrorKind, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use winit::icon::{Icon, RgbaIcon};

pub const APP_ID: &str = "aster";

const ICON: &[u8] = include_bytes!("../../../assets/aster-128.png");
const LAUNCHER: &str = "applications/aster.desktop";
const ICON_PATH: &str = "icons/hicolor/128x128/apps/aster.png";

macro_rules! hicolor_icons {
    ($($file:literal),+ $(,)?) => {
        &[(ICON_PATH, ICON), $((
            concat!("icons/hicolor/", $file),
            include_bytes!(concat!("../../../assets/linux/hicolor/", $file)),
        )),+]
    };
}

const ICONS: &[(&str, &[u8])] = hicolor_icons!(
    "16x16/apps/aster.png",
    "22x22/apps/aster.png",
    "24x24/apps/aster.png",
    "32x32/apps/aster.png",
    "48x48/apps/aster.png",
    "64x64/apps/aster.png",
    "256x256/apps/aster.png",
    "512x512/apps/aster.png",
    "scalable/apps/aster.svg",
);

pub fn window_icon() -> Option<Icon> {
    static ICON_CACHE: OnceLock<Option<Icon>> = OnceLock::new();
    ICON_CACHE.get_or_init(|| match decode_window_icon() {
        Ok(icon) => Some(icon),
        Err(error) => {
            tracing::warn!("could not load the window icon: {error}");
            None
        }
    }).clone()
}

fn decode_window_icon() -> Result<Icon, Box<dyn std::error::Error>> {
    let mut reader = png::Decoder::new(ICON).read_info()?;
    let mut pixels = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut pixels)?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return Err("expected an 8-bit RGBA logo".into());
    }
    pixels.truncate(info.buffer_size());
    Ok(RgbaIcon::new(pixels, info.width, info.height)?.into())
}

pub fn value(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '\\' => out.push_str(r"\\"),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            ' ' if out.is_empty() => out.push_str(r"\s"),
            _ => out.push(c),
        }
    }
    out
}

pub fn exec(executable: &str, args: &[&str]) -> String {
    let mut line = String::new();
    for arg in std::iter::once(executable).chain(args.iter().copied()) {
        if !line.is_empty() {
            line.push(' ');
        }
        line.push('"');
        for c in arg.chars() {
            match c {
                '"' | '\x60' | '$' => { line.push('\\'); line.push(c); }
                '\\' => line.push_str(r"\\"),
                '%' => line.push_str("%%"),
                _ => line.push(c),
            }
        }
        line.push('"');
    }
    value(&line)
}

fn launcher(executable: &str, profiles: &[String]) -> String {
    let command = exec(executable, &[]);
    let mut text = format!("[Desktop Entry]\nType=Application\nName=Aster\nExec={command}\nIcon=aster\nTerminal=false\nCategories=System;TerminalEmulator;\nStartupWMClass={APP_ID}\nX-TerminalArgExec=-e\nX-TerminalArgDir=--working-directory\nX-Aster-Managed=true\nActions=new-window;");
    for i in 0..profiles.len() {
        let _ = write!(text, "profile-{i};");
    }
    let _ = write!(text, "\n\n[Desktop Action new-window]\nName=New window\nName[ru]=Новое окно\nExec={command}\n");
    for (i, name) in profiles.iter().enumerate() {
        let label = value(name);
        let command = exec(executable, &["profile", name]);
        let _ = write!(text, "\n[Desktop Action profile-{i}]\nName={label}\nExec={command}\n");
    }
    text
}

pub fn xdg_home(variable: &str, fallback: &str) -> io::Result<PathBuf> {
    std::env::var_os(variable).map(PathBuf::from).filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from).filter(|p| p.is_absolute()).map(|p| p.join(fallback)))
        .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "home directory is unavailable"))
}

pub fn executable(path: &Path) -> io::Result<&str> {
    path.to_str().filter(|_| path.is_absolute())
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "executable must have an absolute UTF-8 path"))
}

pub fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    if fs::read(path).is_ok_and(|existing| existing == contents) {
        return Ok(());
    }
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or(ErrorKind::InvalidInput)?;
    fs::create_dir_all(parent)?;
    let (staging, mut file) = loop {
        let staging = parent.join(format!(".aster-{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        match fs::OpenOptions::new().write(true).create_new(true).open(&staging) {
            Ok(file) => break (staging, file),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let result = file.write_all(contents);
    drop(file);
    let result = result.and_then(|()| fs::rename(&staging, path));
    if result.is_err() {
        let _ = fs::remove_file(staging);
    }
    result
}

pub fn remove(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

pub fn update_launcher(data: &Path, path: &Path, profiles: &[String], install: Option<bool>) -> io::Result<bool> {
    let entry = data.join(LAUNCHER);
    let managed = match fs::read_to_string(&entry) {
        Ok(text) if text.lines().any(|line| line == "X-Aster-Managed=true") => true,
        Ok(_) if install == Some(true) => return Err(io::Error::new(ErrorKind::AlreadyExists, entry.display().to_string())),
        Ok(_) => false,
        Err(e) if e.kind() == ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    let installed = install.unwrap_or(managed);
    if installed {
        let text = launcher(executable(path)?, profiles);
        for &(relative, image) in ICONS {
            write(&data.join(relative), image)?;
        }
        write(&entry, text.as_bytes())?;
    } else if managed {
        for &(relative, _) in ICONS {
            remove(&data.join(relative))?;
        }
        remove(&entry)?;
    }
    Ok(installed)
}

#[cfg(test)]
#[path = "../test/desktop.rs"]
mod tests;
