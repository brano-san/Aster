use crate::app::{App, Effect};
use crate::input::{Key, Mods};
use crate::panes::PaneId;
use aster_gfx::render::Rect;
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub dir: bool,
    pub link: bool,
    pub size: u64,
    pub modified: Option<u64>,
    pub mode: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileRequest {
    List { id: u64, paths: Vec<String> },
    OpenLink { id: u64, path: String },
    Download { id: u64, remote: String },
    Upload { id: u64, local: PathBuf, dir: String },
    Mkdir(String),
    Remove { path: String, dir: bool },
    Rename { from: String, to: String },
    Edit { id: u64, remote: String },
    Chmod { path: String, mode: u32, recursive: bool },
    Cancel(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileReply {
    Listing { id: u64, path: String, entries: Vec<FileEntry> },
    LinkFile { id: u64, path: String },
    NavigationFailed { id: u64, error: String },
    Progress { id: u64, done: u64, total: u64 },
    Downloaded { id: u64, local: PathBuf },
    Uploaded { id: u64, remote: String },
    Changed { dir: String },
    Editing { id: u64, local: PathBuf },
    Saved { id: u64, remote: String },
    Failed { id: Option<u64>, error: String },
}

impl FileRequest {
    pub fn failure(&self, error: String) -> FileReply {
        match self {
            Self::List { id, .. } | Self::OpenLink { id, .. } => FileReply::NavigationFailed { id: *id, error },
            Self::Download { id, .. } | Self::Upload { id, .. } | Self::Edit { id, .. } | Self::Cancel(id) => FileReply::Failed { id: Some(*id), error },
            _ => FileReply::Failed { id: None, error },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransferState {
    Running,
    Editing { saves: u32 },
    Done(String),
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transfer {
    pub id: u64,
    pub pane: PaneId,
    pub name: String,
    pub upload: bool,
    pub edit: bool,
    pub done: u64,
    pub total: u64,
    pub state: TransferState,
    pub local: Option<PathBuf>,
    pub host: String,
    pub bytes_per_second: u64,
    sample_at: Instant,
    sample_bytes: u64,
    cancel_pending: bool,
}

impl Transfer {
    fn active(&self) -> bool {
        matches!(self.state, TransferState::Running | TransferState::Editing { .. })
    }

    fn progress(&mut self, done: u64, total: u64, now: Instant) {
        let elapsed = now.saturating_duration_since(self.sample_at).as_secs_f64();
        if elapsed >= 0.25 {
            self.bytes_per_second = (done.saturating_sub(self.sample_bytes) as f64 / elapsed) as u64;
            self.sample_at = now;
            self.sample_bytes = done;
        }
        self.done = done;
        self.total = total;
    }

    pub(crate) fn description(&self) -> String {
        use crate::i18n::{t, tf};
        match &self.state {
            TransferState::Running if self.total == 0 => t("Preparing…").into(),
            TransferState::Running => {
                let left = human_size(self.total.saturating_sub(self.done));
                let done = human_size(self.done);
                let total = human_size(self.total);
                if self.bytes_per_second == 0 {
                    tf("{done} of {total}  ·  {left} left", &[("done", &done), ("total", &total), ("left", &left)])
                } else {
                    let speed = human_size(self.bytes_per_second);
                    tf("{done} of {total}  ·  {left} left  ·  {speed}/s", &[("done", &done), ("total", &total), ("left", &left), ("speed", &speed)])
                }
            }
            TransferState::Editing { saves } => tf("editing locally  ·  saved {saves} times", &[("saves", &saves.to_string())]),
            TransferState::Done(message) | TransferState::Failed(message) => message.clone(),
        }
    }

    pub(crate) fn action_label(&self) -> &'static str {
        use crate::i18n::t;
        if self.active() && self.cancel_pending { return t("Cancelling…"); }
        match &self.state {
            TransferState::Running => t("Cancel"),
            TransferState::Editing { .. } => t("Stop"),
            TransferState::Done(_) if self.local.is_some() => t("Show file"),
            _ => t("Dismiss"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Prompt {
    NewFolder(String),
    Rename { from: String, text: String },
    Delete { name: String, dir: bool },
    GoTo(String),
    Upload(String),
    Chmod { name: String, dir: bool, text: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct FilesPage {
    pub pane: PaneId,
    pub host: String,
    pub path: String,
    pub entries: Vec<FileEntry>,
    pub query: String,
    pub shown: Vec<usize>,
    pub selected: usize,
    pub scroll: f32,
    pub loading: bool,
    pub error: Option<String>,
    pub prompt: Option<Prompt>,
    pub crumb: Option<usize>,
    pending: Option<u64>,
    deferred_refresh: HashSet<String>,
}

pub const UP: &str = "..";

pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() || dir == "." {
        name.to_owned()
    } else if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

pub fn parent(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) => "/".to_owned(),
        Some(i) => trimmed[..i].to_owned(),
        None if path.starts_with('/') => "/".to_owned(),
        None => ".".to_owned(),
    }
}

pub fn permissions(mode: u32) -> String {
    let bits = ['r', 'w', 'x', 'r', 'w', 'x', 'r', 'w', 'x'];
    (0..9).map(|i| if mode & (0o400 >> i) != 0 { bits[i] } else { '-' }).collect()
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else if value < 10.0 {
        format!("{value:.1} {}", UNITS[unit])
    } else {
        format!("{value:.0} {}", UNITS[unit])
    }
}

pub fn date(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    if crate::i18n::language() == "ru" { format!("{day:02}.{month:02}.{year:04}") } else { format!("{year:04}-{month:02}-{day:02}") }
}

impl FilesPage {
    pub fn new(pane: PaneId, host: String) -> Self {
        Self { pane, host, path: String::new(), entries: Vec::new(), query: String::new(), shown: Vec::new(), selected: 0, scroll: 0.0, loading: true, error: None, prompt: None, crumb: None, pending: None, deferred_refresh: HashSet::new() }
    }

    fn take_deferred_refresh(&mut self) -> Option<String> {
        let refresh = self.deferred_refresh.contains(&self.path);
        self.deferred_refresh.clear();
        refresh.then(|| self.path.clone())
    }

    pub fn with_up(&self) -> bool {
        !self.path.is_empty() && self.path != "/" && self.query.is_empty()
    }

    fn refilter(&mut self) {
        let q = self.query.to_lowercase();
        self.shown = (0..self.entries.len()).filter(|&i| q.is_empty() || self.entries[i].name.to_lowercase().contains(&q)).collect();
        self.selected = 0;
        self.scroll = 0.0;
    }

    pub fn rows(&self) -> usize {
        self.shown.len() + usize::from(self.with_up())
    }

    pub fn entry(&self, row: usize) -> Option<&FileEntry> {
        let index = if self.with_up() { row.checked_sub(1)? } else { row };
        self.shown.get(index).map(|&i| &self.entries[i])
    }

    pub fn is_up(&self, row: usize) -> bool {
        self.with_up() && row == 0
    }

    fn step(&mut self, delta: isize) {
        let n = self.rows();
        if n == 0 {
            return;
        }
        self.selected = (self.selected as isize + delta).clamp(0, n as isize - 1) as usize;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Breadcrumb {
    pub label: String,
    pub path: String,
}

pub fn breadcrumbs(path: &str) -> Vec<Breadcrumb> {
    let mut result = Vec::new();
    let mut prefix = String::new();
    if path.starts_with('/') {
        prefix.push('/');
        result.push(Breadcrumb { label: prefix.clone(), path: prefix.clone() });
    }
    for label in path.split('/').filter(|part| !part.is_empty()) {
        prefix = join(&prefix, label);
        result.push(Breadcrumb { label: label.to_owned(), path: prefix.clone() });
    }
    result
}

pub const ROW_H: f32 = 30.0;
pub const TRANSFER_H: f32 = 30.0;
pub const MAX_TRANSFERS: usize = 4;

#[derive(Clone, Debug, PartialEq)]
pub struct FilesGeometry {
    pub title: Rect,
    pub close: Rect,
    pub path: Rect,
    pub crumbs: Vec<(usize, Breadcrumb, Rect)>,
    pub shell_directory: Option<Rect>,
    pub transfer_button: Rect,
    pub filter: Rect,
    pub list: Rect,
    pub rows: Vec<(usize, Rect)>,
    pub height: f32,
    pub(crate) scroll: f32,
    pub transfers: Vec<(u64, Rect)>,
    pub hint: Rect,
}

pub fn geometry(content: Rect, s: f32, page: &FilesPage, transfers: &[&Transfer], shell_directory: bool) -> FilesGeometry {
    let width = (980.0 * s).min(content.w - 32.0 * s).max(240.0 * s).round();
    let x = (content.x + (content.w - width) / 2.0).round();
    let top = content.y + (14.0 * s).round();
    let header_h = (34.0 * s).round();
    let close = Rect::new(x + width - header_h, top, header_h, header_h);
    let gap = (6.0 * s).round();
    let button_w = (110.0 * s).min((width - header_h - 3.0 * gap) * 0.36).round();
    let transfer_button = Rect::new(close.x - button_w - gap, top, button_w, header_h);
    let shell_directory = shell_directory.then(|| Rect::new(transfer_button.x - button_w - gap, top, button_w, header_h));
    let title = Rect::new(x, top, (shell_directory.unwrap_or(transfer_button).x - gap - x).max(0.0), header_h);
    let bar_y = top + header_h + (8.0 * s).round();
    let bar_h = (32.0 * s).round();
    let filter_w = (220.0 * s).min(width * 0.35).round();
    let filter = Rect::new(x + width - filter_w, bar_y, filter_w, bar_h);
    let path = Rect::new(x, bar_y, (width - filter_w - 8.0 * s).max(0.0), bar_h);
    let crumbs = breadcrumb_geometry(path, s, page);
    let hint_h = (40.0 * s).round();
    let hint = Rect::new(x, content.bottom() - hint_h - (6.0 * s).round(), width, hint_h);
    let shown = transfers.len().min(MAX_TRANSFERS);
    let th = (TRANSFER_H * s).round();
    let transfers_top = hint.y - shown as f32 * th - if shown > 0 { (8.0 * s).round() } else { 0.0 };
    let transfer_rects = transfers.iter().take(MAX_TRANSFERS).enumerate().map(|(k, t)| (t.id, Rect::new(x, transfers_top + (8.0 * s).round() + k as f32 * th, width, th))).collect();
    let list_top = bar_y + bar_h + (10.0 * s).round();
    let list = Rect::new(x, list_top, width, (transfers_top - list_top - 6.0 * s).max(0.0));
    let rh = (ROW_H * s).round();
    let height = page.rows() as f32 * rh;
    let scroll = page.scroll.clamp(0.0, (height - list.h).max(0.0));
    let rows = (0..page.rows()).map(|r| (r, Rect::new(x, list.y + r as f32 * rh - scroll.round(), width, rh))).collect();
    FilesGeometry { title, close, path, crumbs, shell_directory, transfer_button, filter, list, rows, height, scroll, transfers: transfer_rects, hint }
}

fn breadcrumb_geometry(path: Rect, s: f32, page: &FilesPage) -> Vec<(usize, Breadcrumb, Rect)> {
    if page.prompt.is_some() {
        return Vec::new();
    }
    let crumbs = breadcrumbs(&page.path);
    let Some(last) = crumbs.len().checked_sub(1) else { return Vec::new() };
    let focus = page.crumb.unwrap_or(last).min(last);
    let gap = (5.0 * s).round();
    let inner = path.inset(gap, gap);
    let width = |crumb: &Breadcrumb| ((15.0 + 7.5 * crumb.label.chars().count() as f32) * s).min(inner.w);
    let mut start = focus;
    let mut end = focus + 1;
    let mut remaining = inner.w - width(&crumbs[focus]);
    while end < crumbs.len() && remaining >= gap + width(&crumbs[end]) {
        remaining -= gap + width(&crumbs[end]);
        end += 1;
    }
    while start > 0 && remaining >= gap + width(&crumbs[start - 1]) {
        start -= 1;
        remaining -= gap + width(&crumbs[start]);
    }
    let indices = if start > 0 && remaining >= gap + width(&crumbs[0]) { Some(0) } else { None };
    let mut x = inner.x;
    indices.into_iter().chain(start..end).map(|i| {
        let w = width(&crumbs[i]);
        let rect = Rect::new(x, inner.y, w, inner.h);
        x += w + gap;
        (i, crumbs[i].clone(), rect)
    }).collect()
}

#[derive(Default)]
pub(crate) struct TransfersPanel {
    selected: Option<u64>,
    scroll: usize,
    action_focused: bool,
}

pub(crate) struct TransfersGeometry {
    pub frame: Rect,
    pub title: Rect,
    pub close: Rect,
    pub list: Rect,
    pub rows: Vec<TransferRow>,
    pub hint: Rect,
    capacity: usize,
}

pub(crate) struct TransferRow {
    pub id: u64,
    pub details: Rect,
    pub source: Rect,
    pub action: Rect,
}

impl App {
    pub(crate) fn global_transfers_open(&self) -> bool {
        self.sftp_transfers.is_some()
    }

    pub(crate) fn toggle_global_transfers(&mut self) {
        self.sftp_transfers = if self.sftp_transfers.is_none() {
            self.overlay = None;
            Some(TransfersPanel { selected: self.global_transfer_ids().first().copied(), ..Default::default() })
        } else {
            None
        };
        self.dirty = true;
    }

    fn global_transfer_ids(&self) -> Vec<u64> {
        let transfers = self.transfers.iter().rev();
        transfers.clone().filter(|t| t.active()).chain(transfers.filter(|t| !t.active())).map(|t| t.id).collect()
    }

    pub(crate) fn global_transfer_geometry(&self) -> Option<TransfersGeometry> {
        let panel = self.sftp_transfers.as_ref()?;
        let content = self.chrome.content;
        let s = self.ui();
        let margin = (12.0 * s).round();
        let padding = (14.0 * s).round();
        let width = (800.0 * s).min((content.w - 2.0 * margin).max(0.0)).round();
        let row_h = (49.0 * s).round().max(1.0);
        let header = (32.0 * s).round();
        let footer = (24.0 * s).round();
        let ids = self.global_transfer_ids();
        let height = (2.0 * padding + header + footer + ids.len().clamp(2, 10) as f32 * row_h)
            .min((content.h - 2.0 * margin).max(0.0));
        let frame = Rect::new((content.x + (content.w - width) / 2.0).round(), (content.y + (content.h - height) / 2.0).round(), width, height);
        let inside = frame.inset(padding, padding);
        let close = Rect::new(inside.right() - header, inside.y, header, header);
        let title = Rect::new(inside.x, inside.y, (inside.w - header).max(0.0), header);
        let list = Rect::new(inside.x, title.bottom(), inside.w, (inside.h - header - footer).max(0.0));
        let hint = Rect::new(inside.x, list.bottom(), inside.w, footer);
        let capacity = (list.h / row_h) as usize;
        let skip = panel.scroll.min(ids.len().saturating_sub(capacity));
        let button_w = (92.0 * s).min(inside.w * 0.23).round();
        let rows = ids.into_iter().skip(skip).take(capacity).enumerate().map(|(i, id)| {
            let row = Rect::new(list.x, list.y + i as f32 * row_h, list.w, row_h);
            let button_h = (28.0 * s).round();
            let action = Rect::new(row.right() - button_w, row.y + (row_h - button_h) / 2.0, button_w, button_h);
            let source = Rect::new(action.x - margin - button_w, action.y, button_w, button_h);
            let details = Rect::new(row.x, row.y, (source.x - row.x - margin).max(0.0), row.h);
            TransferRow { id, details, source, action }
        }).collect();
        Some(TransfersGeometry { frame, title, close, list, rows, hint, capacity })
    }

    pub(crate) fn global_transfer_selected(&self) -> Option<u64> {
        self.sftp_transfers.as_ref()?.selected.filter(|id| self.transfers.iter().any(|t| t.id == *id))
            .or_else(|| self.global_transfer_ids().first().copied())
    }

    pub(crate) fn transfer_action_focused(&self) -> bool {
        self.sftp_transfers.as_ref().is_some_and(|panel| panel.action_focused)
    }

    pub(crate) fn global_transfers_key(&mut self, key: Key) {
        let ids = self.global_transfer_ids();
        let current = self.global_transfer_selected();
        let from = current.and_then(|id| ids.iter().position(|&i| i == id)).unwrap_or(0);
        self.dirty = true;
        let to = match key {
            Key::Escape => return self.toggle_global_transfers(),
            Key::Tab => {
                if let Some(panel) = self.sftp_transfers.as_mut() { panel.action_focused = !panel.action_focused; }
                return;
            }
            Key::Enter if self.transfer_action_focused() => {
                if let Some(id) = current { self.transfer_action(id); }
                return;
            }
            Key::Enter => {
                if let Some(id) = current { self.open_transfer_tab(id); }
                return;
            }
            Key::Delete => {
                if let Some(id) = current.filter(|id| self.transfers.iter().any(|t| t.id == *id && (t.active() || t.local.is_none()))) {
                    self.transfer_action(id);
                }
                return;
            }
            Key::Up => from.saturating_sub(1),
            Key::Down => from.saturating_add(1),
            Key::PageUp => from.saturating_sub(8),
            Key::PageDown => from.saturating_add(8),
            Key::Home => 0,
            Key::End => ids.len().saturating_sub(1),
            _ => return,
        }.min(ids.len().saturating_sub(1));
        let capacity = self.global_transfer_geometry().map_or(1, |geo| geo.capacity.max(1));
        if let Some(panel) = self.sftp_transfers.as_mut() {
            panel.selected = ids.get(to).copied();
            panel.scroll = panel.scroll.clamp(to.saturating_sub(capacity - 1), to);
        }
    }

    pub(crate) fn global_transfers_click(&mut self, x: f32, y: f32) {
        let Some(geo) = self.global_transfer_geometry() else { return };
        if geo.close.contains(x, y) || !geo.frame.contains(x, y) {
            return self.toggle_global_transfers();
        }
        let Some(row) = geo.rows.iter().find(|r| r.details.contains(x, y) || r.source.contains(x, y) || r.action.contains(x, y)) else { return };
        if let Some(panel) = self.sftp_transfers.as_mut() {
            panel.selected = Some(row.id);
            panel.action_focused = row.action.contains(x, y);
        }
        if row.source.contains(x, y) {
            self.open_transfer_tab(row.id);
        } else if row.action.contains(x, y) {
            self.transfer_action(row.id);
        }
        self.dirty = true;
    }

    pub(crate) fn global_transfers_wheel(&mut self, dy: f32) {
        let Some(geo) = self.global_transfer_geometry() else { return };
        let maximum = self.transfers.len().saturating_sub(geo.capacity);
        if let Some(panel) = self.sftp_transfers.as_mut() {
            panel.scroll = if dy > 0.0 { panel.scroll.saturating_sub(3) } else { panel.scroll.saturating_add(3) }.min(maximum);
            self.dirty = true;
        }
    }

    fn open_transfer_tab(&mut self, id: u64) {
        let Some(pane) = self.transfers.iter().find(|t| t.id == id).map(|t| t.pane) else { return };
        let Some(index) = self.tabs.iter().position(|tab| tab.root.contains(pane)) else { return };
        self.close_files();
        self.toggle_global_transfers();
        self.activate(index);
        if self.tabs[index].zoomed != Some(pane) {
            self.tabs[index].zoomed = None;
        }
        self.focus_pane(pane);
    }

    fn transfer_action(&mut self, id: u64) {
        let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) else { return };
        if t.active() {
            if t.cancel_pending { return; }
            t.cancel_pending = true;
            let pane = t.pane;
            self.request(pane, FileRequest::Cancel(id));
        } else if let (TransferState::Done(_), Some(local)) = (&t.state, &t.local) {
            let path = local.clone();
            self.push(Effect::RevealFile(path));
        } else {
            self.transfers.retain(|t| t.id != id);
        }
    }

    pub(crate) fn end_file_transfers(&mut self, pane: PaneId) {
        for transfer in self.transfers.iter_mut().filter(|t| t.pane == pane && t.active()) {
            transfer.state = TransferState::Failed(crate::i18n::t("the connection is closed").into());
        }
    }
}

impl App {
    pub fn files_open(&self) -> bool {
        self.files.is_some()
    }

    pub fn files_page(&self) -> Option<&FilesPage> {
        self.files.as_ref()
    }

    pub fn transfers_of(&self, pane: PaneId) -> Vec<&Transfer> {
        self.transfers.iter().rev().filter(|t| t.pane == pane).collect()
    }

    pub fn files_geometry(&self) -> Option<FilesGeometry> {
        let page = self.files.as_ref()?;
        Some(geometry(self.chrome.content, self.ui(), page, &self.transfers_of(page.pane), self.shell_directory(page.pane).is_some()))
    }

    pub(crate) fn can_browse(&self, pane: PaneId) -> bool {
        self.config.ssh.client == crate::config::SshClient::Native && self.panes.get(&pane).is_some_and(|p| p.profile.ssh.is_some() && p.exited.is_none())
    }

    pub(crate) fn open_files(&mut self) {
        let Some(pane) = self.focused_pane().filter(|p| self.can_browse(*p)) else { return };
        let host = self.panes.get(&pane).and_then(|p| p.profile.ssh.clone()).unwrap_or_default();
        self.overlay = None;
        self.settings = None;
        let mut paths = Vec::with_capacity(3);
        if let Some(last) = self.last_dirs.get(&pane) { paths.push(last.clone()); }
        if let Some(cwd) = self.shell_directory(pane).filter(|path| !paths.contains(path)) { paths.push(cwd); }
        paths.push(String::new());
        self.panel_motion.closed_files = None;
        self.list_motion.files = Default::default();
        self.files = Some(FilesPage::new(pane, host));
        self.request_navigation(|id| FileRequest::List { id, paths });
    }

    pub(crate) fn close_files(&mut self) {
        if let Some(page) = self.files.take() {
            self.panel_motion.closed_files = Some(page);
        }
        self.dirty = true;
    }

    fn request(&mut self, pane: PaneId, request: FileRequest) {
        self.push(Effect::Files { pane, request });
    }

    fn browse(&mut self, path: String) {
        self.request_navigation(|id| FileRequest::List { id, paths: vec![path] });
    }

    pub(crate) fn shell_directory(&self, pane: PaneId) -> Option<String> {
        self.terminal(pane)?.lock().reported_cwd()
            .filter(|path| path.starts_with('/') && !path.chars().any(char::is_control))
            .map(str::to_owned)
    }

    fn browse_shell(&mut self) {
        if let Some(path) = self.files.as_ref().and_then(|page| self.shell_directory(page.pane)) {
            self.browse(path);
        }
    }

    fn request_navigation(&mut self, request: impl FnOnce(u64) -> FileRequest) {
        let id = self.next_id();
        let Some(page) = self.files.as_mut() else { return };
        page.pending = Some(id);
        page.loading = true;
        page.error = None;
        page.prompt = None;
        page.crumb = None;
        let pane = page.pane;
        self.request(pane, request(id));
        self.dirty = true;
    }

    fn next_id(&mut self) -> u64 {
        self.transfer_seq += 1;
        self.transfer_seq
    }

    pub(crate) fn upload(&mut self, local: PathBuf) {
        let Some(page) = self.files.as_ref().filter(|page| !page.path.is_empty()) else { return };
        let (pane, dir) = (page.pane, page.path.clone());
        let name = local.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| local.display().to_string());
        let id = self.new_transfer(pane, name, true, false, Some(local.clone()));
        self.request(pane, FileRequest::Upload { id, local, dir });
    }

    fn download_selected(&mut self) {
        if let Some(e) = self.selected_entry() {
            self.download(&e.name);
        }
    }

    fn download(&mut self, name: &str) {
        let Some(page) = self.files.as_ref() else { return };
        self.download_from(page.pane, join(&page.path, name));
    }

    fn download_from(&mut self, pane: PaneId, remote: String) {
        let name = remote.rsplit('/').next().unwrap_or(&remote).to_owned();
        let id = self.new_transfer(pane, name, false, false, None);
        self.request(pane, FileRequest::Download { id, remote });
    }

    fn edit_locally(&mut self, name: &str) {
        let Some(page) = self.files.as_ref() else { return };
        let (pane, remote) = (page.pane, join(&page.path, name));
        let id = self.new_transfer(pane, name.to_owned(), false, true, None);
        self.request(pane, FileRequest::Edit { id, remote });
    }

    fn new_transfer(&mut self, pane: PaneId, name: String, upload: bool, edit: bool, local: Option<PathBuf>) -> u64 {
        let id = self.next_id();
        let host = self.panes.get(&pane).and_then(|p| p.profile.ssh.clone()).unwrap_or_default();
        self.transfers.push(Transfer { id, pane, name, upload, edit, done: 0, total: 0, state: TransferState::Running, local, host, bytes_per_second: 0, sample_at: self.clock(), sample_bytes: 0, cancel_pending: false });
        self.dirty = true;
        id
    }

    fn activate_row(&mut self, row: usize) {
        let Some(page) = self.files.as_ref() else { return };
        if page.is_up(row) {
            let up = parent(&page.path);
            return self.browse(up);
        }
        let Some(entry) = page.entry(row).cloned() else { return };
        if entry.link {
            let path = join(&page.path, &entry.name);
            self.request_navigation(|id| FileRequest::OpenLink { id, path });
        } else if entry.dir {
            let path = join(&page.path, &entry.name);
            self.browse(path);
        } else {
            self.download(&entry.name);
        }
    }

    fn refresh_files(&mut self, pane: PaneId, dir: String) {
        let Some(page) = self.files.as_mut().filter(|page| page.pane == pane) else { return };
        if page.pending.is_some() {
            page.deferred_refresh.insert(dir);
        } else if page.path == dir {
            self.browse(dir);
        }
    }

    pub fn files_reply(&mut self, pane: PaneId, reply: FileReply) {
        self.dirty = true;
        let visible = self.files.as_ref().is_some_and(|p| p.pane == pane);
        match reply {
            FileReply::Listing { id, path, entries } => {
                let Some(page) = self.files.as_mut().filter(|p| visible && p.pending == Some(id)) else { return };
                page.pending = None;
                self.last_dirs.insert(pane, path.clone());
                let keep = (page.path == path).then(|| page.entry(page.selected).map(|e| e.name.clone())).flatten();
                page.path = path;
                page.entries = entries;
                page.loading = false;
                page.error = None;
                page.query.clear();
                page.refilter();
                self.list_motion.files = Default::default();
                if let Some(name) = keep {
                    if let Some(row) = (0..page.rows()).find(|&r| page.entry(r).is_some_and(|e| e.name == name)) {
                        page.selected = row;
                    }
                }
                let refresh = page.take_deferred_refresh();
                self.reveal_file();
                if let Some(dir) = refresh { self.browse(dir); }
            }
            FileReply::LinkFile { id, path } => {
                let Some(page) = self.files.as_mut().filter(|p| visible && p.pending == Some(id)) else { return };
                page.pending = None;
                page.loading = false;
                let refresh = page.take_deferred_refresh();
                self.download_from(pane, path);
                if let Some(dir) = refresh { self.browse(dir); }
            }
            FileReply::NavigationFailed { id, error } => {
                let Some(page) = self.files.as_mut().filter(|p| visible && p.pending == Some(id)) else { return };
                page.pending = None;
                page.loading = false;
                if let Some(dir) = page.take_deferred_refresh() { self.browse(dir); }
                if let Some(page) = self.files.as_mut() { page.error = Some(error); }
            }
            FileReply::Changed { dir } => {
                self.refresh_files(pane, dir);
            }
            FileReply::Progress { id, done, total } => {
                let now = self.clock();
                if let Some(t) = self.transfers.iter_mut().find(|t| t.pane == pane && t.id == id && t.active()) {
                    t.progress(done, total, now);
                }
            }
            FileReply::Downloaded { id, local } => {
                if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
                    t.done = t.total.max(t.done);
                    t.state = TransferState::Done(crate::i18n::tf("saved to {path}", &[("path", &local.display().to_string())]));
                    t.local = Some(local);
                }
            }
            FileReply::Uploaded { id, remote } => {
                if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
                    t.done = t.total.max(t.done);
                    t.state = TransferState::Done(crate::i18n::tf("uploaded to {path}", &[("path", &remote)]));
                }
                self.refresh_files(pane, parent(&remote));
            }
            FileReply::Editing { id, local } => {
                if let Some(t) = self.transfers.iter_mut().find(|t| {
                    t.id == id && t.pane == pane && t.edit && !t.cancel_pending && matches!(t.state, TransferState::Running)
                }) {
                    t.done = t.total.max(t.done);
                    t.state = TransferState::Editing { saves: 0 };
                    t.local = Some(local.clone());
                    self.push(Effect::Open(local.display().to_string()));
                }
            }
            FileReply::Saved { id, remote } => {
                if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
                    if let TransferState::Editing { saves } = &mut t.state {
                        *saves += 1;
                    }
                }
                self.refresh_files(pane, parent(&remote));
            }
            FileReply::Failed { id: Some(id), error } => {
                if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
                    t.state = match (&t.state, error.as_str()) {
                        (TransferState::Editing { saves }, "cancelled") => TransferState::Done(crate::i18n::tf("editing stopped after {saves} saves", &[("saves", &saves.to_string())])),
                        _ => TransferState::Failed(error),
                    };
                }
            }
            FileReply::Failed { id: None, error } => {
                if let Some(page) = self.files.as_mut().filter(|_| visible) {
                    page.loading = page.pending.is_some();
                    page.error = Some(error);
                }
            }
        }
        const RECENT_LIMIT: usize = 40;
        if let Some(oldest) = self.transfers.iter().rev().filter(|t| !t.active()).nth(RECENT_LIMIT - 1).map(|t| t.id) {
            self.transfers.retain(|t| t.id >= oldest || t.active());
        }
    }

    fn reveal_file(&mut self) {
        let Some(geo) = self.files_geometry() else { return };
        let rh = (ROW_H * self.ui()).round();
        let Some(page) = self.files.as_mut() else { return };
        page.scroll = geo.scroll;
        let top = page.selected as f32 * rh;
        if top < page.scroll {
            page.scroll = top;
        } else if top + rh > page.scroll + geo.list.h {
            page.scroll = top + rh - geo.list.h;
        }
        page.scroll = page.scroll.clamp(0.0, (geo.height - geo.list.h).max(0.0));
    }

    fn commit_prompt(&mut self) {
        let Some(page) = self.files.as_mut() else { return };
        let Some(prompt) = page.prompt.take() else { return };
        let (pane, dir) = (page.pane, page.path.clone());
        match prompt {
            Prompt::NewFolder(name) if !name.trim().is_empty() => self.request(pane, FileRequest::Mkdir(join(&dir, name.trim()))),
            Prompt::Rename { from, text } if !text.trim().is_empty() && text.trim() != from => self.request(pane, FileRequest::Rename { from: join(&dir, &from), to: join(&dir, text.trim()) }),
            Prompt::Delete { name, dir: is_dir } => self.request(pane, FileRequest::Remove { path: join(&dir, &name), dir: is_dir }),
            Prompt::GoTo(path) if !path.trim().is_empty() => self.browse(path.trim().to_owned()),
            Prompt::Upload(path) if !path.trim().is_empty() => self.upload(PathBuf::from(path.trim().trim_matches('"'))),
            Prompt::Chmod { name, dir: is_dir, text } => {
                if let Ok(mode) = u32::from_str_radix(text.trim(), 8) {
                    if mode <= 0o7777 {
                        self.request(pane, FileRequest::Chmod { path: join(&dir, &name), mode, recursive: is_dir });
                    }
                }
            }
            _ => {}
        }
        self.dirty = true;
    }

    fn selected_entry(&self) -> Option<FileEntry> {
        let page = self.files.as_ref()?;
        page.entry(page.selected).cloned()
    }

    pub(crate) fn files_key(&mut self, key: Key, mods: Mods) {
        let Some(page) = self.files.as_mut() else { return };
        self.dirty = true;
        if let Some(prompt) = page.prompt.as_mut() {
            let text = match prompt {
                Prompt::NewFolder(t) | Prompt::GoTo(t) | Prompt::Upload(t) | Prompt::Rename { text: t, .. } | Prompt::Chmod { text: t, .. } => Some(t),
                Prompt::Delete { .. } => None,
            };
            match key {
                Key::Escape => page.prompt = None,
                Key::Enter => self.commit_prompt(),
                Key::Backspace => {
                    if let Some(t) = text {
                        if mods.ctrl {
                            t.clear();
                        } else {
                            t.pop();
                        }
                    }
                }
                Key::Char('V') if mods.ctrl => self.push(Effect::RequestPaste),
                _ => {}
            }
            return;
        }
        let previous_scroll = page.scroll;
        if key == Key::Tab {
            page.crumb = match (page.crumb, breadcrumbs(&page.path).len()) {
                (None, count) if count > 0 => Some(count - 1),
                _ => None,
            };
            return;
        }
        if let Some(index) = page.crumb {
            let crumbs = breadcrumbs(&page.path);
            match key {
                Key::Escape | Key::Down => { page.crumb = None; return; }
                Key::Left => { page.crumb = Some(index.saturating_sub(1)); return; }
                Key::Right => { page.crumb = Some((index + 1).min(crumbs.len().saturating_sub(1))); return; }
                Key::Enter => {
                    if let Some(crumb) = crumbs.get(index) { self.browse(crumb.path.clone()); }
                    return;
                }
                _ => {}
            }
        }
        match key {
            Key::Escape if !page.query.is_empty() => {
                page.query.clear();
                page.refilter();
                self.list_motion.files = Default::default();
            }
            Key::Escape => return self.close_files(),
            Key::Up => page.step(-1),
            Key::Down => page.step(1),
            Key::PageUp => page.step(-10),
            Key::PageDown => page.step(10),
            Key::Home => page.step(isize::MIN / 2),
            Key::End => page.step(isize::MAX / 2),
            Key::Enter => {
                let row = page.selected;
                self.activate_row(row);
            }
            Key::Backspace if !page.query.is_empty() => {
                page.query.pop();
                page.refilter();
                self.list_motion.files = Default::default();
            }
            Key::Backspace => {
                let up = parent(&page.path);
                self.browse(up);
            }
            Key::F(5) => {
                let path = page.path.clone();
                self.browse(path);
            }
            Key::F(7) => page.prompt = Some(Prompt::NewFolder(String::new())),
            Key::F(4) => {
                if let Some(e) = self.selected_entry().filter(|e| !e.dir) {
                    self.edit_locally(&e.name);
                }
            }
            Key::Char('D') if mods.ctrl => self.download_selected(),
            Key::F(9) => {
                if let Some(e) = self.selected_entry() {
                    let text = e.mode.map(|m| format!("{:o}", m & 0o7777)).unwrap_or_default();
                    if let Some(page) = self.files.as_mut() {
                        page.prompt = Some(Prompt::Chmod { name: e.name, dir: e.dir, text });
                    }
                }
            }
            Key::Char('E') if mods.ctrl => {
                if let Some(e) = self.selected_entry().filter(|e| !e.dir) {
                    self.edit_locally(&e.name);
                }
            }
            Key::F(2) => {
                if let Some(e) = self.selected_entry() {
                    if let Some(page) = self.files.as_mut() {
                        page.prompt = Some(Prompt::Rename { from: e.name.clone(), text: e.name });
                    }
                }
            }
            Key::Delete => {
                if let Some(e) = self.selected_entry() {
                    if let Some(page) = self.files.as_mut() {
                        page.prompt = Some(Prompt::Delete { name: e.name, dir: e.dir });
                    }
                }
            }
            Key::Char('L') if mods.ctrl && mods.shift => self.browse_shell(),
            Key::Char('L') if mods.ctrl => {
                page.crumb = None;
                page.prompt = Some(Prompt::GoTo(page.path.clone()));
            }
            Key::Char('U') if mods.ctrl => page.prompt = Some(Prompt::Upload(String::new())),
            Key::Char('V') if mods.ctrl => self.push(Effect::RequestPaste),
            _ => {}
        }
        self.reveal_file();
        if crate::list_motion::scroll_key(key) {
            let (animate, now) = (self.animations(), self.clock());
            if let Some(page) = self.files.as_ref() {
                self.list_motion.files.retarget(previous_scroll, page.scroll, animate, now);
            }
        }
    }

    pub(crate) fn files_text(&mut self, text: &str) {
        let Some(page) = self.files.as_mut() else { return };
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        match page.prompt.as_mut() {
            Some(Prompt::NewFolder(t) | Prompt::GoTo(t) | Prompt::Upload(t) | Prompt::Rename { text: t, .. } | Prompt::Chmod { text: t, .. }) => t.push_str(&clean),
            Some(Prompt::Delete { .. }) => {}
            None => {
                page.crumb = None;
                page.query.push_str(&clean);
                page.refilter();
                self.list_motion.files = Default::default();
            }
        }
        self.dirty = true;
    }

    pub(crate) fn files_click(&mut self, x: f32, y: f32, clicks: u8) {
        let Some(geo) = self.files_geometry() else { return };
        self.dirty = true;
        if geo.close.contains(x, y) {
            return self.close_files();
        }
        if geo.transfer_button.contains(x, y) {
            return self.toggle_global_transfers();
        }
        if geo.shell_directory.is_some_and(|r| r.contains(x, y)) {
            return self.browse_shell();
        }
        if let Some((_, crumb, _)) = geo.crumbs.iter().find(|(_, _, r)| r.contains(x, y)) {
            return self.browse(crumb.path.clone());
        }
        if geo.path.contains(x, y) {
            if let Some(page) = self.files.as_mut().filter(|p| p.prompt.is_none()) {
                page.crumb = None;
                page.prompt = Some(Prompt::GoTo(page.path.clone()));
            }
            return;
        }
        if let Some((id, _)) = geo.transfers.iter().find(|(_, r)| r.contains(x, y)) {
            return self.transfer_action(*id);
        }
        if !geo.list.contains(x, y) {
            return;
        }
        let Some((row, _)) = geo.rows.iter().find(|(_, r)| r.contains(x, y)).copied() else { return };
        if let Some(page) = self.files.as_mut() {
            page.prompt = None;
            page.crumb = None;
            page.selected = row;
        }
        if clicks >= 2 {
            self.activate_row(row);
        }
    }

    pub(crate) fn files_wheel(&mut self, dy: f32, pixels: Option<f32>) {
        let Some(geo) = self.files_geometry() else { return };
        let rh = (ROW_H * self.ui()).round();
        let (animate, now) = (self.animations(), self.clock());
        let Some(page) = self.files.as_mut() else { return };
        page.scroll = self.list_motion.files.wheel(geo.scroll, (geo.height - geo.list.h).max(0.0), dy * 3.0 * rh, pixels, animate, now);
        self.dirty = true;
    }

    pub(crate) fn files_dropped(&mut self, paths: Vec<String>) {
        for p in paths {
            self.upload(PathBuf::from(p));
        }
    }
}

#[cfg(test)]
#[path = "../test/files.rs"]
mod tests;
