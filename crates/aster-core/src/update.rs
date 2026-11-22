use crate::app::{App, Effect};
use crate::i18n::{t, tf};
use aster_gfx::render::Rect;
use serde_json::{json, Map, Value as Json};
use std::cmp::Ordering;
use std::time::{Duration, Instant};

pub const MANIFEST_URL: &str = "https://github.com/brano-san/Aster/releases/latest/download/latest.json";
pub const RELEASES_URL: &str = "https://github.com/brano-san/Aster/releases/latest";
pub const WINDOWS: &str = "windows-x86_64";
pub const LINUX: &str = "linux-x86_64";
pub const PLATFORM: &str = if cfg!(windows) { WINDOWS } else { LINUX };
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
const LOCAL_SCHEME: &str = "file://";
const RESULT_TOAST: Duration = Duration::from_secs(4);
const CHECKING_TOAST: Duration = Duration::from_secs(15);
pub const NOTES_VISIBLE: usize = 12;
const STATUS_LINES: usize = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub url: String,
    pub signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateRequest {
    Check { manual: bool },
    Install(Release),
    Cancel,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateEvent {
    Checked { manual: bool, found: Result<Option<Release>, String> },
    Progress { received: u64, total: Option<u64> },
    Installing,
    Failed(String),
    ReadOnly(String),
}

pub fn error_manifest() -> String {
    t("The update description could not be read").to_owned()
}

pub fn error_no_build(version: &str, platform: &str) -> String {
    tf("Aster {version} has no build for {platform}", &[("version", version), ("platform", platform)])
}

pub fn error_signature() -> String {
    t("The download does not match its signature").to_owned()
}

pub fn error_missing(name: &str) -> String {
    tf("The update archive has no {name}", &[("name", name)])
}

pub fn error_off() -> String {
    t("Update checks are turned off for this run").to_owned()
}

pub fn error_cancelled() -> String {
    t("The download was cancelled").to_owned()
}

pub fn is_local(url: &str) -> bool {
    url.starts_with(LOCAL_SCHEME)
}

pub fn local_path(url: &str) -> Option<&str> {
    let rest = url.strip_prefix(LOCAL_SCHEME)?;
    Some(match rest.strip_prefix('/') {
        Some(tail) if tail.as_bytes().get(1) == Some(&b':') => tail,
        _ => rest,
    })
}

pub fn parse_manifest(text: &str, platform: &str) -> Result<Release, String> {
    parse_manifest_since(text, platform, CURRENT, false)
}

pub fn parse_manifest_since(text: &str, platform: &str, current: &str, local: bool) -> Result<Release, String> {
    let doc: Json = serde_json::from_str(text).map_err(|_| error_manifest())?;
    let version = doc["version"].as_str().map(|v| v.trim().trim_start_matches('v')).filter(|v| parse_version(v).is_some()).ok_or_else(error_manifest)?;
    let entry = &doc["platforms"][platform];
    let field = |name: &str| entry[name].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    let (Some(url), Some(signature)) = (field("url"), field("signature")) else {
        return Err(error_no_build(version, platform));
    };
    let reachable = url.starts_with("https://") || (local && is_local(&url));
    if !reachable || base64_decode(&signature).is_none() {
        return Err(error_manifest());
    }
    let notes = release_notes(&doc, version, current);
    Ok(Release { version: version.to_owned(), notes, url, signature })
}

fn release_notes(doc: &Json, version: &str, current: &str) -> String {
    let latest = doc["notes"].as_str().unwrap_or("").trim();
    let mut skipped = Vec::new();
    for entry in doc["history"].as_array().into_iter().flatten() {
        let (Some(tag), Some(notes)) = (entry["version"].as_str(), entry["notes"].as_str()) else { continue };
        let tag = tag.trim().trim_start_matches('v');
        let notes = notes.trim();
        if !notes.is_empty() && newer(tag, current) && newer(version, tag) {
            skipped.push((tag, notes));
        }
    }
    if skipped.is_empty() {
        return latest.to_owned();
    }
    skipped.sort_by(|a, b| compare_versions(b.0, a.0).unwrap_or(Ordering::Equal));
    skipped.dedup_by(|a, b| compare_versions(a.0, b.0) == Some(Ordering::Equal));
    std::iter::once((version, latest)).chain(skipped).filter(|(_, notes)| !notes.is_empty())
        .map(|(tag, notes)| format!("## {tag}\n\n{notes}")).collect::<Vec<_>>().join("\n\n")
}

pub fn render_manifest(version: &str, notes: &str, pub_date: &str, platforms: &[(&str, String, String)]) -> String {
    let mut map = Map::new();
    for (platform, url, signature) in platforms {
        map.insert((*platform).to_owned(), json!({ "url": url, "signature": signature }));
    }
    let doc = json!({ "version": version, "notes": notes, "pub_date": pub_date, "platforms": map });
    serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n"
}

pub fn platform_of_asset(name: &str) -> Option<&'static str> {
    if !name.starts_with("aster-") {
        None
    } else if name.ends_with("-x86_64-windows.zip") {
        Some(WINDOWS)
    } else if name.ends_with("-x86_64-linux.tar.gz") {
        Some(LINUX)
    } else {
        None
    }
}

fn parse_version(v: &str) -> Option<(Vec<u64>, bool)> {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = match v.split_once(['-', '+']) {
        Some((core, rest)) => (core, v[core.len()..].starts_with('-') && !rest.is_empty()),
        None => (v, false),
    };
    let parts = core.split('.').map(|p| p.parse::<u64>().ok()).collect::<Option<Vec<_>>>()?;
    (!parts.is_empty()).then_some((parts, pre))
}

pub fn newer(candidate: &str, current: &str) -> bool {
    compare_versions(candidate, current) == Some(Ordering::Greater)
}

fn compare_versions(candidate: &str, current: &str) -> Option<Ordering> {
    let (a, a_pre) = parse_version(candidate)?;
    let (b, b_pre) = parse_version(current)?;
    let n = a.len().max(b.len());
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    Some((0..n).map(|i| at(&a, i).cmp(&at(&b, i))).find(|o| *o != Ordering::Equal).unwrap_or_else(|| b_pre.cmp(&a_pre)))
}

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for k in 0..4 {
            if k <= chunk.len() {
                out.push(ALPHABET[(n >> (18 - 6 * k)) as usize & 63] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let text = text.trim().trim_end_matches('=');
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = ALPHABET.iter().position(|&a| a == c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    (acc == 0 && bits < 6).then_some(out)
}

pub fn percent(received: u64, total: Option<u64>) -> Option<u8> {
    total.filter(|&t| t > 0).map(|t| (received.min(t) * 100 / t) as u8)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Offer,
    Downloading,
    Installing,
    Failed,
    ReadOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeButton {
    Install,
    Notes,
    Later,
    Cancel,
    Retry,
    OpenReleases,
}

impl NoticeButton {
    pub fn label(self, notes_open: bool) -> &'static str {
        match self {
            Self::Install => t("Install and restart"),
            Self::Notes if notes_open => t("Hide notes"),
            Self::Notes => t("What's new"),
            Self::Later => t("Later"),
            Self::Cancel => t("Cancel"),
            Self::Retry => t("Try again"),
            Self::OpenReleases => t("Open the download page"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateNotice {
    pub release: Release,
    pub stage: Stage,
    pub progress: Option<(u64, Option<u64>)>,
    pub message: Option<String>,
    pub cancelling: bool,
    pub notes_open: bool,
    pub notes_scroll: usize,
    pub wrapped: Vec<String>,
    pub status_lines: Vec<String>,
    pub button_widths: Vec<f32>,
}

impl UpdateNotice {
    pub fn new(release: Release) -> Self {
        Self { release, stage: Stage::Offer, progress: None, message: None, cancelling: false, notes_open: false, notes_scroll: 0, wrapped: Vec::new(), status_lines: Vec::new(), button_widths: Vec::new() }
    }

    pub fn buttons(&self) -> Vec<NoticeButton> {
        use NoticeButton::*;
        match self.stage {
            Stage::Offer if self.release.notes.trim().is_empty() => vec![Install, Later],
            Stage::Offer => vec![Install, Notes, Later],
            Stage::Downloading if self.cancelling => Vec::new(),
            Stage::Downloading => vec![Cancel],
            Stage::Installing => Vec::new(),
            Stage::Failed => vec![Retry, Later],
            Stage::ReadOnly => vec![OpenReleases, Later],
        }
    }

    pub fn busy(&self) -> bool {
        matches!(self.stage, Stage::Downloading | Stage::Installing)
    }

    pub fn title(&self) -> String {
        match self.stage {
            Stage::Failed | Stage::ReadOnly => tf("Could not install Aster {version}", &[("version", &self.release.version)]),
            _ => tf("Aster {version} is available", &[("version", &self.release.version)]),
        }
    }

    pub fn status(&self) -> String {
        match self.stage {
            Stage::Offer => tf("Installed {current}. Aster restarts after the download and reopens your tabs.", &[("current", CURRENT)]),
            Stage::Downloading if self.cancelling => t("Cancelling…").to_owned(),
            Stage::Downloading => match self.progress {
                Some((received, total)) => match percent(received, total) {
                    Some(p) => tf("Downloading… {percent}%", &[("percent", &p.to_string())]),
                    None => tf("Downloading… {size} MB", &[("size", &format!("{:.1}", received as f64 / 1_048_576.0))]),
                },
                None => t("Downloading…").to_owned(),
            },
            Stage::Installing => t("Installing and restarting…").to_owned(),
            Stage::Failed | Stage::ReadOnly => self.message.clone().unwrap_or_default(),
        }
    }

    pub fn fraction(&self) -> Option<f32> {
        match self.stage {
            Stage::Installing => Some(1.0),
            Stage::Downloading => Some(self.progress.and_then(|(r, total)| percent(r, total)).map_or(0.0, |p| f32::from(p) / 100.0)),
            _ => None,
        }
    }

    pub fn notes_shown(&self) -> bool {
        self.notes_open && self.stage == Stage::Offer
    }

    pub fn visible_notes(&self) -> &[String] {
        let start = self.notes_scroll.min(self.wrapped.len());
        &self.wrapped[start..(start + NOTES_VISIBLE).min(self.wrapped.len())]
    }

    fn scroll_notes(&mut self, lines: isize) -> bool {
        let limit = self.wrapped.len().saturating_sub(NOTES_VISIBLE);
        let next = (self.notes_scroll as isize + lines).clamp(0, limit as isize) as usize;
        let moved = next != self.notes_scroll;
        self.notes_scroll = next;
        moved
    }
}

pub fn plain_line(markdown: &str) -> String {
    let line = markdown.trim_end().replace("**", "").replace('`', "");
    let heading = line.trim_start_matches('#');
    if heading.len() < line.len() {
        return heading.trim().to_owned();
    }
    match line.trim_start().strip_prefix("* ").or_else(|| line.trim_start().strip_prefix("- ")) {
        Some(item) => format!("\u{2022} {item}"),
        None => line,
    }
}

pub fn note_lines(notes: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for line in notes.lines().map(plain_line) {
        if line.is_empty() && lines.last().is_none_or(|l| l.is_empty()) {
            continue;
        }
        lines.push(line);
    }
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

#[derive(Clone, Debug, PartialEq)]
pub struct NoticeGeometry {
    pub frame: Rect,
    pub title: Rect,
    pub status: Rect,
    pub bar: Option<Rect>,
    pub notes: Option<Rect>,
    pub line_h: f32,
    pub buttons: Vec<(NoticeButton, Rect)>,
}

pub fn notice_geometry(content: Rect, s: f32, notice: &UpdateNotice) -> NoticeGeometry {
    let margin = (16.0 * s).round();
    let w = (440.0 * s).min(content.w - 2.0 * margin).max(240.0 * s).round();
    let pad = (14.0 * s).round();
    let title_h = (22.0 * s).round();
    let line_h = (18.0 * s).round();
    let status_h = (notice.status_lines.len().clamp(1, STATUS_LINES) as f32 * line_h + 2.0 * s).round();
    let gap = (8.0 * s).round();
    let button_h = (30.0 * s).round();
    let inner_w = w - 2.0 * pad;
    let mut h = pad + title_h + status_h;
    let bar = notice.fraction().map(|_| {
        let r = Rect::new(0.0, h + gap, inner_w, (6.0 * s).round().max(2.0));
        h = r.bottom();
        r
    });
    let notes = notice.notes_shown().then(|| {
        let shown = notice.wrapped.len().clamp(1, NOTES_VISIBLE) as f32;
        let r = Rect::new(0.0, h + gap, inner_w, (shown * line_h + 2.0 * gap).round());
        h = r.bottom();
        r
    });
    let buttons = notice.buttons();
    if !buttons.is_empty() {
        h += (12.0 * s).round() + button_h;
    }
    h += pad;
    let x = (content.right() - w - margin).max(content.x).round();
    let y = (content.bottom() - h - margin).max(content.y + gap).round();
    let place = |r: Rect| Rect::new(x + pad + r.x, y + r.y, r.w, r.h);
    let frame = Rect::new(x, y, w, h);
    let title = Rect::new(x + pad, y + pad, inner_w, title_h);
    let status = Rect::new(x + pad, title.bottom(), inner_w, status_h);
    let by = frame.bottom() - pad - button_h;
    let mut right = frame.right() - pad;
    let mut placed: Vec<(NoticeButton, Rect)> = buttons.iter().enumerate().rev().map(|(i, &b)| {
        let bw = notice.button_widths.get(i).copied().unwrap_or(110.0 * s).max(72.0 * s).round();
        let r = Rect::new(right - bw, by, bw, button_h);
        right -= bw + gap;
        (b, r)
    }).collect();
    placed.reverse();
    NoticeGeometry { frame, title, status, bar: bar.map(place), notes: notes.map(place), line_h, buttons: placed }
}

impl App {
    pub fn update_notice(&self) -> Option<&UpdateNotice> {
        self.update_notice.as_ref()
    }

    pub fn check_updates_on_start(&mut self) {
        if self.config.updates.check_automatically {
            self.effects.push(Effect::Update(UpdateRequest::Check { manual: false }));
        }
    }

    pub(crate) fn check_updates(&mut self) {
        if self.update_notice.as_ref().is_some_and(UpdateNotice::busy) {
            self.dirty = true;
            return;
        }
        self.toast(t("Checking for updates…").to_owned(), CHECKING_TOAST);
        self.effects.push(Effect::Update(UpdateRequest::Check { manual: true }));
    }

    pub(crate) fn toast(&mut self, text: String, lasting: Duration) {
        self.tab_toast = Some((text, Instant::now() + lasting));
        self.dirty = true;
        self.schedule_timer();
    }

    pub fn on_update(&mut self, event: UpdateEvent) {
        self.dirty = true;
        match event {
            UpdateEvent::Checked { manual, found } => {
                if manual {
                    self.tab_toast = None;
                }
                match found {
                    Ok(Some(release)) => {
                        if !self.update_notice.as_ref().is_some_and(UpdateNotice::busy) {
                            self.update_notice = Some(UpdateNotice::new(release));
                        }
                    }
                    Ok(None) if manual => self.toast(tf("Aster {version} is up to date", &[("version", CURRENT)]), RESULT_TOAST),
                    Err(error) if manual => self.toast(tf("Could not check for updates: {error}", &[("error", &error)]), RESULT_TOAST),
                    _ => {}
                }
            }
            UpdateEvent::Progress { received, total } => {
                if let Some(n) = self.update_notice.as_mut().filter(|n| n.stage == Stage::Downloading) {
                    n.progress = Some((received, total));
                }
            }
            UpdateEvent::Installing => {
                if let Some(n) = self.update_notice.as_mut().filter(|n| n.stage == Stage::Downloading) {
                    n.stage = Stage::Installing;
                }
            }
            UpdateEvent::Failed(error) => self.update_stopped(Stage::Failed, error),
            UpdateEvent::ReadOnly(dir) => self.update_stopped(Stage::ReadOnly, tf("Aster cannot replace its files in {dir}. Download the new version from the release page.", &[("dir", &dir)])),
        }
    }

    fn update_stopped(&mut self, stage: Stage, message: String) {
        match self.update_notice.as_mut() {
            Some(n) => {
                n.stage = stage;
                n.message = Some(message);
                n.cancelling = false;
                n.progress = None;
            }
            None => self.toast(message, RESULT_TOAST),
        }
    }

    pub fn update_geometry(&self) -> Option<NoticeGeometry> {
        self.update_notice.as_ref().map(|n| notice_geometry(self.chrome.content, self.ui(), n))
    }

    pub(crate) fn update_hovered(&self, x: f32, y: f32) -> bool {
        self.update_geometry().is_some_and(|g| g.frame.contains(x, y))
    }

    pub(crate) fn update_click(&mut self, x: f32, y: f32) -> bool {
        let Some(geo) = self.update_geometry() else { return false };
        if !geo.frame.contains(x, y) {
            return false;
        }
        if let Some(&(button, _)) = geo.buttons.iter().find(|(_, r)| r.contains(x, y)) {
            self.press_update(button);
        }
        self.dirty = true;
        true
    }

    pub(crate) fn update_wheel(&mut self, dy: f32) -> bool {
        let (x, y) = self.mouse;
        if !self.update_hovered(x, y) {
            return false;
        }
        if let Some(n) = self.update_notice.as_mut().filter(|n| n.notes_shown()) {
            if n.scroll_notes(if dy > 0.0 { -3 } else { 3 }) {
                self.dirty = true;
            }
        }
        true
    }

    pub fn press_update(&mut self, button: NoticeButton) {
        let Some(n) = self.update_notice.as_mut() else { return };
        self.dirty = true;
        match button {
            NoticeButton::Install | NoticeButton::Retry => {
                n.stage = Stage::Downloading;
                n.progress = None;
                n.message = None;
                n.cancelling = false;
                n.notes_open = false;
                let release = n.release.clone();
                self.effects.push(Effect::Update(UpdateRequest::Install(release)));
            }
            NoticeButton::Notes => {
                n.notes_open = !n.notes_open;
                n.notes_scroll = 0;
            }
            NoticeButton::Cancel => {
                n.cancelling = true;
                self.effects.push(Effect::Update(UpdateRequest::Cancel));
            }
            NoticeButton::Later => self.update_notice = None,
            NoticeButton::OpenReleases => {
                self.update_notice = None;
                self.effects.push(Effect::Open(RELEASES_URL.to_owned()));
            }
        }
    }
}

#[cfg(test)]
#[path = "../test/update.rs"]
mod tests;
