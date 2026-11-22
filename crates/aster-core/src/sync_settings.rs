use crate::app::{App, Effect};
use crate::i18n::{t, tf};
use crate::input::{Key, Mods};
use crate::settings::{InlineEdit, Row as SettingRow, SettingsPage, PAGES, SYNC};
use crate::sync::{self, Backend, Entry, Operation, Part, Status};
use aster_gfx::render::Rect;

pub(crate) const BACKENDS: &[(&str, &str)] = &[("off", "Off"), ("folder", "Folder"), ("git", "Git")];

pub(crate) fn part(key: &str) -> Option<Part> {
    Some(match key.strip_prefix("sync.parts.")? {
        "shortcuts" => Part::Shortcuts,
        "appearance" => Part::Appearance,
        "profiles" => Part::Profiles,
        "terminal" => Part::Terminal,
        "vault" => Part::Vault,
        _ => return None,
    })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Page {
    pub backend: Backend,
    pub ready: bool,
    pub entries: Vec<Entry>,
    pub delete: Option<String>,
    pub action: usize,
}

impl Page {
    pub fn setting_visible(&self, key: &str) -> bool {
        match key {
            "sync.backend" | "info.sync-status" => true,
            "sync.folder" => self.backend == Backend::Folder,
            "sync.repository" => self.backend == Backend::Git,
            _ => self.backend != Backend::Off,
        }
    }

    pub fn row(&self, index: usize) -> Option<Row> {
        if !self.ready { return None; }
        match index {
            0 => Some(Row::Store),
            1 => Some(Row::New),
            index => self.entries.get(index - 2).map(|_| Row::Entry(index - 2)),
        }
    }

    pub fn confirming(&self, row: Row) -> bool {
        if let Row::Entry(index) = row {
            return self.entries.get(index).is_some_and(|entry| self.delete.as_ref() == Some(&entry.name));
        }
        false
    }

    pub fn labels(&self, row: Row) -> (String, String) {
        match row {
            Row::Store => (t("Stored configurations").into(), if self.entries.is_empty() { t("No snapshots to display").into() } else { String::new() }),
            Row::New => (t("New configuration snapshot").into(), t("Save the selected parts under a new name").into()),
            Row::Entry(index) => {
                let entry = &self.entries[index];
                let machine = if entry.machine.is_empty() { t("Unknown machine") } else { &entry.machine };
                let detail = if self.confirming(row) { t("Delete this snapshot from the store?").into() }
                    else { format!("{} · {machine}", sync::date(entry.modified)) };
                (entry.name.clone(), detail)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Row { Store, New, Entry(usize) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Button { Refresh, New, Download, Upload, AskDelete, Delete, Cancel }

impl Button {
    pub fn label(self) -> &'static str {
        match self {
            Self::Refresh => t("Refresh"),
            Self::New => t("Upload as new"),
            Self::Download => t("Download"),
            Self::Upload => t("Upload"),
            Self::AskDelete => t("Delete config"),
            Self::Delete => t("Confirm delete"),
            Self::Cancel => t("Cancel"),
        }
    }
}

impl Row {
    pub fn buttons(self, confirming: bool, editing: bool) -> &'static [Button] {
        match self {
            Self::Store => &[Button::Refresh],
            Self::New if editing => &[Button::Upload],
            Self::New => &[Button::New],
            Self::Entry(_) if confirming => &[Button::Delete, Button::Cancel],
            Self::Entry(_) => &[Button::Download, Button::Upload, Button::AskDelete],
        }
    }

    pub fn stacked(self, width: f32, scale: f32) -> bool {
        match self {
            Self::Store => width < 330.0 * scale,
            Self::New | Self::Entry(_) => width < 560.0 * scale,
        }
    }

    pub fn control(self, row: Rect, scale: f32) -> Rect {
        let pad = (14.0 * scale).round();
        let h = (30.0 * scale).round();
        if self.stacked(row.w, scale) {
            return Rect::new(row.x + pad, row.bottom() - h - 8.0 * scale, (row.w - 2.0 * pad).max(1.0), h);
        }
        let limit = match self {
            Self::Store => 140.0,
            Self::New => 335.0,
            Self::Entry(_) => 318.0,
        };
        let w = (limit * scale).min(row.w * 0.56).round();
        Rect::new(row.right() - pad - w, row.y + ((row.h - h) / 2.0).round(), w, h)
    }
}

pub(crate) fn button_rects(control: Rect, scale: f32, count: usize, editing: bool) -> Vec<Rect> {
    let gap = (5.0 * scale).round();
    let width = match (count, editing) {
        (1, true) => (96.0 * scale).min(control.w * 0.4).round(),
        (1, false) => (172.0 * scale).min(control.w).round(),
        _ => ((control.w - gap * (count - 1) as f32) / count as f32).floor(),
    };
    let start = control.right() - (width + gap) * count as f32 + gap;
    (0..count).map(|i| Rect::new(start + (width + gap) * i as f32, control.y, width, control.h)).collect()
}

impl App {
    pub(crate) fn sync_settings_changed(&mut self) {
        self.sync_view.entries.clear();
        self.sync_view.status = Status::Idle;
        self.sync_view.conflict = None;
        self.sync_view.git_conflicts.clear();
        self.refresh_sync_rows();
    }

    pub(crate) fn refresh_sync_rows(&mut self) {
        for page in self.settings.iter_mut().chain(self.settings_stash.iter_mut()) {
            let sync = &mut page.sync;
            let old_backend = sync.backend;
            let old_ready = sync.ready;
            sync.backend = self.config.sync.backend;
            sync.ready = self.config.sync.ready();
            if sync.entries != self.sync_view.entries || old_backend != sync.backend || old_ready != sync.ready {
                let selected = page.current_item();
                let selected_name = selected.and_then(|item| match page.sync_row(item) {
                    Some(Row::Entry(index)) => Some(page.sync.entries[index].name.clone()),
                    _ => None,
                });
                let scroll = page.scroll;
                page.sync.entries.clone_from(&self.sync_view.entries);
                page.sync.delete = None;
                page.refilter();
                if let Some(index) = page.shown.iter().position(|&item| {
                    if let Some(name) = &selected_name {
                        matches!(page.sync_row(item), Some(Row::Entry(index)) if page.sync.entries[index].name == *name)
                    } else { Some(item) == selected }
                }) {
                    page.selected = index;
                    page.scroll = scroll;
                }
            }
        }
    }

    pub(crate) fn sync_status_text(&self) -> String {
        if !self.config.sync.ready() { return t("Not set up").into(); }
        let status = match &self.sync_view.status {
            Status::Syncing => t("Syncing…").into(),
            Status::Error(issue) => tf("Sync error: {error}", &[("error", &issue.message())]),
            Status::Synced(saved) => synced_at(*saved),
            Status::Idle if self.config.sync.last_synced > 0 => synced_at(self.config.sync.last_synced),
            Status::Idle => t("Ready to sync").into(),
        };
        if let Some(name) = &self.sync_view.conflict {
            format!("{status} · {}", tf("Remote settings applied. Local settings saved as {name}.", &[("name", name)]))
        } else if !self.sync_view.git_conflicts.is_empty() {
            format!("{status} · {}", sync::git_conflict_message(&self.sync_view.git_conflicts))
        } else { status }
    }

    pub(crate) fn request_sync(&mut self, operation: Operation) {
        if self.config.sync.ready() && self.sync_view.status != Status::Syncing {
            self.push(Effect::ConfigSync(operation));
        }
    }

    pub(crate) fn refresh_sync_page(&mut self) {
        if self.settings.as_ref().is_some_and(|page: &SettingsPage| PAGES[page.page] == SYNC) {
            self.request_sync(Operation::List);
        }
    }

    pub(crate) fn sync_settings_key(&mut self, key: Key, mods: Mods) -> bool {
        let Some(page) = self.settings.as_mut() else { return false };
        let Some(row) = page.current_item().and_then(|item| page.sync_row(item)) else { return false };
        if mods != Mods::NONE { return false; }
        let editing = page.editing.is_some();
        let confirming = page.sync.confirming(row);
        let buttons = row.buttons(confirming, editing);
        let button = match key {
            Key::Escape if confirming => Button::Cancel,
            Key::Enter => buttons[if editing || confirming { 0 } else { page.sync.action.min(buttons.len() - 1) }],
            Key::Delete if !editing && !confirming && matches!(row, Row::Entry(_)) => Button::AskDelete,
            Key::Left | Key::Right if !editing && !confirming => {
                let index = page.sync.action.min(buttons.len() - 1);
                page.sync.action = if key == Key::Left { index.saturating_sub(1) } else { (index + 1).min(buttons.len() - 1) };
                return true;
            }
            _ if confirming => return true,
            _ => return false,
        };
        self.sync_settings_action(row, button);
        true
    }

    pub(crate) fn sync_settings_click(&mut self, item: Option<usize>, geometry: SettingRow, x: f32, y: f32, editing_same: bool) -> bool {
        let scale = self.ui();
        let Some(item) = item else { return false };
        let Some(page) = self.settings.as_ref() else { return false };
        let Some(row) = page.sync_row(item) else {
            if let Some(page) = self.settings.as_mut() { page.sync.delete = None; }
            return false;
        };
        let editing = editing_same && row == Row::New;
        let buttons = row.buttons(page.sync.confirming(row), editing);
        let clicked = button_rects(geometry.control, scale, buttons.len(), editing).iter().position(|rect| rect.contains(x, y));
        if !editing { self.commit_edit(); }
        if let Some(page) = self.settings.as_mut() {
            let Some(index) = page.shown.iter().position(|shown| *shown == item) else { return true };
            if page.sync_row(item) != Some(row) { return true; }
            page.selected = index;
            page.sync.action = clicked.unwrap_or(0);
            if !page.sync.confirming(row) { page.sync.delete = None; }
            page.notice = None;
        }
        if let Some(index) = clicked {
            self.sync_settings_action(row, buttons[index]);
        }
        true
    }

    fn sync_settings_action(&mut self, row: Row, button: Button) {
        if button == Button::Refresh {
            return self.request_sync(Operation::List);
        }
        if button == Button::Cancel {
            if let Some(page) = self.settings.as_mut() {
                page.sync.delete = None;
                page.sync.action = 0;
            }
            return;
        }
        if self.config.read_only() { return self.warn_read_only_config(); }
        if self.sync_view.status == Status::Syncing { return; }
        let Some(page) = self.settings.as_mut() else { return };
        let request = match (row, button) {
            (Row::New, Button::New) => {
                page.editing = Some(InlineEdit::Setting(String::new()));
                page.notice = None;
                return;
            }
            (Row::New, Button::Upload) => {
                let name = page.edit_text().trim().to_owned();
                if !sync::valid_name(&name) || page.sync.entries.iter().any(|entry| entry.name.eq_ignore_ascii_case(&name)) {
                    let issue = if sync::valid_name(&name) { sync::Issue::Exists } else { sync::Issue::Name };
                    page.notice = Some(issue.message());
                    return;
                }
                page.editing = None;
                Operation::Upload { name, new: true }
            }
            (Row::Entry(index), button) => {
                let Some(entry) = page.sync.entries.get(index) else { return };
                let name = entry.name.clone();
                match button {
                    Button::Download => Operation::Download(name),
                    Button::Upload => Operation::Upload { name, new: false },
                    Button::AskDelete => {
                        page.sync.delete = Some(name);
                        return;
                    }
                    Button::Delete if page.sync.confirming(row) => {
                        page.sync.delete = None;
                        page.sync.action = 0;
                        Operation::Delete(name)
                    }
                    _ => return,
                }
            }
            _ => return,
        };
        self.request_sync(request);
    }
}

fn synced_at(seconds: u64) -> String {
    let hours = seconds / 3600 % 24;
    let minutes = seconds / 60 % 60;
    tf("Synced at {time} UTC", &[("time", &format!("{hours:02}:{minutes:02}"))])
}
