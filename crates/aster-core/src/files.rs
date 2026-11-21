use crate::app::{App, Effect};
use crate::input::{Key, Mods};
use crate::panes::PaneId;
use aster_gfx::render::Rect;
use std::path::PathBuf;

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
    List(String),
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
    Listing { path: String, entries: Vec<FileEntry> },
    Progress { id: u64, done: u64, total: u64 },
    Downloaded { id: u64, local: PathBuf },
    Uploaded { id: u64, remote: String },
    Changed { dir: String },
    Editing { id: u64, local: PathBuf },
    Saved { id: u64, remote: String },
    Failed { id: Option<u64>, error: String },
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
    format!("{year:04}-{month:02}-{day:02}")
}

impl FilesPage {
    pub fn new(pane: PaneId, host: String) -> Self {
        Self { pane, host, path: String::new(), entries: Vec::new(), query: String::new(), shown: Vec::new(), selected: 0, scroll: 0.0, loading: true, error: None, prompt: None }
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

pub const ROW_H: f32 = 30.0;
pub const TRANSFER_H: f32 = 30.0;
pub const MAX_TRANSFERS: usize = 4;

#[derive(Clone, Debug, PartialEq)]
pub struct FilesGeometry {
    pub title: Rect,
    pub close: Rect,
    pub path: Rect,
    pub filter: Rect,
    pub list: Rect,
    pub rows: Vec<(usize, Rect)>,
    pub height: f32,
    pub transfers: Vec<(u64, Rect)>,
    pub hint: Rect,
}

pub fn geometry(content: Rect, s: f32, page: &FilesPage, transfers: &[&Transfer]) -> FilesGeometry {
    let width = (980.0 * s).min(content.w - 32.0 * s).max(240.0 * s).round();
    let x = (content.x + (content.w - width) / 2.0).round();
    let top = content.y + (14.0 * s).round();
    let header_h = (34.0 * s).round();
    let close = Rect::new(x + width - header_h, top, header_h, header_h);
    let title = Rect::new(x, top, width - header_h, header_h);
    let bar_y = top + header_h + (8.0 * s).round();
    let bar_h = (32.0 * s).round();
    let filter_w = (220.0 * s).min(width * 0.35).round();
    let filter = Rect::new(x + width - filter_w, bar_y, filter_w, bar_h);
    let path = Rect::new(x, bar_y, (width - filter_w - 8.0 * s).max(0.0), bar_h);
    let hint_h = (24.0 * s).round();
    let hint = Rect::new(x, content.bottom() - hint_h - (6.0 * s).round(), width, hint_h);
    let shown = transfers.len().min(MAX_TRANSFERS);
    let th = (TRANSFER_H * s).round();
    let transfers_top = hint.y - shown as f32 * th - if shown > 0 { (8.0 * s).round() } else { 0.0 };
    let transfer_rects = transfers.iter().take(MAX_TRANSFERS).enumerate().map(|(k, t)| (t.id, Rect::new(x, transfers_top + (8.0 * s).round() + k as f32 * th, width, th))).collect();
    let list_top = bar_y + bar_h + (10.0 * s).round();
    let list = Rect::new(x, list_top, width, (transfers_top - list_top - 6.0 * s).max(0.0));
    let rh = (ROW_H * s).round();
    let rows = (0..page.rows()).map(|r| (r, Rect::new(x, list.y + r as f32 * rh - page.scroll, width, rh))).collect();
    FilesGeometry { title, close, path, filter, list, rows, height: page.rows() as f32 * rh, transfers: transfer_rects, hint }
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
        Some(geometry(self.chrome.content, self.ui(), page, &self.transfers_of(page.pane)))
    }

    pub(crate) fn can_browse(&self, pane: PaneId) -> bool {
        self.config.ssh.client == crate::config::SshClient::Native && self.panes.get(&pane).is_some_and(|p| p.profile.ssh.is_some() && p.exited.is_none())
    }

    pub(crate) fn open_files(&mut self) {
        let Some(pane) = self.focused_pane().filter(|p| self.can_browse(*p)) else { return };
        let host = self.panes.get(&pane).and_then(|p| p.profile.ssh.clone()).unwrap_or_default();
        self.overlay = None;
        self.settings = None;
        let mut page = FilesPage::new(pane, host);
        if let Some(last) = self.last_dirs.get(&pane) {
            page.path = last.clone();
        }
        let path = page.path.clone();
        self.files = Some(page);
        self.request(pane, FileRequest::List(path));
        self.dirty = true;
    }

    pub(crate) fn close_files(&mut self) {
        self.files = None;
        self.dirty = true;
    }

    fn request(&mut self, pane: PaneId, request: FileRequest) {
        self.push(Effect::Files { pane, request });
    }

    fn browse(&mut self, path: String) {
        let Some(page) = self.files.as_mut() else { return };
        page.loading = true;
        page.error = None;
        let pane = page.pane;
        self.request(pane, FileRequest::List(path));
    }

    fn next_id(&mut self) -> u64 {
        self.transfer_seq += 1;
        self.transfer_seq
    }

    pub(crate) fn upload(&mut self, local: PathBuf) {
        let Some(page) = self.files.as_ref() else { return };
        let (pane, dir) = (page.pane, page.path.clone());
        let id = self.next_id();
        let name = local.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| local.display().to_string());
        self.transfers.push(Transfer { id, pane, name, upload: true, edit: false, done: 0, total: 0, state: TransferState::Running, local: Some(local.clone()) });
        self.request(pane, FileRequest::Upload { id, local, dir });
        self.dirty = true;
    }

    fn download_selected(&mut self) {
        if let Some(e) = self.selected_entry() {
            self.download(&e.name);
        }
    }

    fn download(&mut self, name: &str) {
        let Some(page) = self.files.as_ref() else { return };
        let (pane, remote) = (page.pane, join(&page.path, name));
        let id = self.next_id();
        self.transfers.push(Transfer { id, pane, name: name.to_owned(), upload: false, edit: false, done: 0, total: 0, state: TransferState::Running, local: None });
        self.request(pane, FileRequest::Download { id, remote });
        self.dirty = true;
    }

    fn edit_locally(&mut self, name: &str) {
        let Some(page) = self.files.as_ref() else { return };
        let (pane, remote) = (page.pane, join(&page.path, name));
        let id = self.next_id();
        self.transfers.push(Transfer { id, pane, name: name.to_owned(), upload: false, edit: true, done: 0, total: 0, state: TransferState::Running, local: None });
        self.request(pane, FileRequest::Edit { id, remote });
        self.dirty = true;
    }

    fn activate_row(&mut self, row: usize) {
        let Some(page) = self.files.as_ref() else { return };
        if page.is_up(row) {
            let up = parent(&page.path);
            return self.browse(up);
        }
        let Some(entry) = page.entry(row).cloned() else { return };
        if entry.dir || entry.link {
            let path = join(&page.path, &entry.name);
            self.browse(path);
        } else {
            self.download(&entry.name);
        }
    }

    pub fn files_reply(&mut self, pane: PaneId, reply: FileReply) {
        self.dirty = true;
        let visible = self.files.as_ref().is_some_and(|p| p.pane == pane);
        match reply {
            FileReply::Listing { path, entries } => {
                self.last_dirs.insert(pane, path.clone());
                let Some(page) = self.files.as_mut().filter(|_| visible) else { return };
                let keep = (page.path == path).then(|| page.entry(page.selected).map(|e| e.name.clone())).flatten();
                page.path = path;
                page.entries = entries;
                page.loading = false;
                page.error = None;
                page.query.clear();
                page.refilter();
                if let Some(name) = keep {
                    if let Some(row) = (0..page.rows()).find(|&r| page.entry(r).is_some_and(|e| e.name == name)) {
                        page.selected = row;
                    }
                }
                self.reveal_file();
            }
            FileReply::Changed { dir } => {
                if visible && self.files.as_ref().is_some_and(|p| p.path == dir) {
                    self.browse(dir);
                }
            }
            FileReply::Progress { id, done, total } => {
                if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
                    t.done = done;
                    t.total = total;
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
                let dir = parent(&remote);
                if visible && self.files.as_ref().is_some_and(|p| p.path == dir) {
                    self.browse(dir);
                }
            }
            FileReply::Editing { id, local } => {
                if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
                    t.done = t.total.max(t.done);
                    t.state = TransferState::Editing { saves: 0 };
                    t.local = Some(local.clone());
                }
                self.push(Effect::Open(local.display().to_string()));
            }
            FileReply::Saved { id, remote } => {
                if let Some(t) = self.transfers.iter_mut().find(|t| t.id == id) {
                    if let TransferState::Editing { saves } = &mut t.state {
                        *saves += 1;
                    }
                }
                let dir = parent(&remote);
                if visible && self.files.as_ref().is_some_and(|p| p.path == dir) {
                    self.browse(dir);
                }
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
                    page.loading = false;
                    page.error = Some(error);
                }
            }
        }
    }

    fn reveal_file(&mut self) {
        let Some(geo) = self.files_geometry() else { return };
        let rh = (ROW_H * self.ui()).round();
        let Some(page) = self.files.as_mut() else { return };
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
        match key {
            Key::Escape if !page.query.is_empty() => {
                page.query.clear();
                page.refilter();
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
            Key::Char('L') if mods.ctrl => page.prompt = Some(Prompt::GoTo(page.path.clone())),
            Key::Char('U') if mods.ctrl => page.prompt = Some(Prompt::Upload(String::new())),
            Key::Char('V') if mods.ctrl => self.push(Effect::RequestPaste),
            _ => {}
        }
        self.reveal_file();
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
                page.query.push_str(&clean);
                page.refilter();
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
        if let Some((id, _)) = geo.transfers.iter().find(|(_, r)| r.contains(x, y)) {
            let id = *id;
            let Some(t) = self.transfers.iter().find(|t| t.id == id).cloned() else { return };
            match (&t.state, &t.local) {
                (TransferState::Running | TransferState::Editing { .. }, _) => {
                    self.request(t.pane, FileRequest::Cancel(id));
                }
                (TransferState::Done(_), Some(local)) if !t.upload => {
                    let folder = local.parent().map(|p| p.display().to_string()).unwrap_or_default();
                    self.push(Effect::Open(folder));
                }
                _ => self.transfers.retain(|x| x.id != id),
            }
            return;
        }
        if !geo.list.contains(x, y) {
            return;
        }
        let Some((row, _)) = geo.rows.iter().find(|(_, r)| r.contains(x, y)).copied() else { return };
        if let Some(page) = self.files.as_mut() {
            page.prompt = None;
            page.selected = row;
        }
        if clicks >= 2 {
            self.activate_row(row);
        }
    }

    pub(crate) fn files_wheel(&mut self, dy: f32) {
        let Some(geo) = self.files_geometry() else { return };
        let rh = (ROW_H * self.ui()).round();
        let Some(page) = self.files.as_mut() else { return };
        page.scroll = (page.scroll - dy * 3.0 * rh).clamp(0.0, (geo.height - geo.list.h).max(0.0));
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
