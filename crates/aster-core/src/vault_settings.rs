use aster_gfx::render::Rect;
use crate::app::{App, Effect};
use crate::i18n::t;
use crate::input::{Event, HitArea, Key, Mods, MouseButton};
use crate::settings::{VaultEntry, VaultIssue};

pub enum VaultAction {
    Show(VaultEntry),
    Rename { entry: VaultEntry, label: String },
    Replace { entry: VaultEntry, value: String },
    Export { entry: VaultEntry, path: String },
    Import { label: String, path: String },
    Delete(VaultEntry),
}

pub enum VaultReply {
    Applied,
    Value(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AfterUnlock {
    #[default]
    Close,
    Change,
    Erase,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum DialogKind {
    Create,
    Unlock(AfterUnlock),
    Change,
    Erase { encrypted: bool },
    Manage,
    Entry(VaultEntry),
    Show(VaultEntry),
    Rename(VaultEntry),
    Replace(VaultEntry),
    Export(VaultEntry),
    Delete(VaultEntry),
    Import,
    Startup,
    Authorize,
    Protect(bool),
}

#[derive(Clone, PartialEq)]
pub(crate) struct VaultDialog {
    pub opened_at: std::time::Instant,
    pub kind: DialogKind,
    fields: Vec<String>,
    ask: bool,
    pub focus: usize,
    pub revealed: bool,
    pub waiting: bool,
    request: u64,
    pub issue: Option<VaultIssue>,
    pub value: Option<String>,
    pub scroll: usize,
    pub max_scroll: std::cell::Cell<usize>,
}

impl std::fmt::Debug for VaultDialog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultDialog").field("kind", &self.kind).field("waiting", &self.waiting).finish()
    }
}

impl DialogKind {
    pub fn title(&self) -> &'static str {
        match self {
            Self::Create => t("Set a master passphrase"),
            Self::Unlock(_) => t("Show vault contents"),
            Self::Change => t("Change the master passphrase"),
            Self::Erase { .. } => t("Erase the vault"),
            Self::Manage => t("Manage vault"),
            Self::Entry(_) => t("Secret actions"),
            Self::Show(_) => t("Show secret"),
            Self::Rename(_) => t("Rename secret"),
            Self::Replace(_) => t("Replace secret"),
            Self::Export(_) => t("Export secret"),
            Self::Delete(_) => t("Delete secret"),
            Self::Import => t("Import key file"),
            Self::Startup => t("Unlock encrypted config"),
            Self::Authorize => t("Authorize vault access"),
            Self::Protect(true) => t("Encrypt the entire config"),
            Self::Protect(false) => t("Decrypt the config"),
        }
    }

    fn fields(&self) -> &'static [&'static str] {
        match self {
            Self::Create | Self::Unlock(_) | Self::Startup | Self::Authorize | Self::Protect(_) => &["Master passphrase"],
            Self::Change => &["Current passphrase", "New passphrase"],
            Self::Rename(_) => &["Secret label"],
            Self::Replace(_) => &["New secret value"],
            Self::Export(_) => &["Export file path"],
            Self::Import => &["Secret label", "Key file path"],
            _ => &[],
        }
    }

    pub fn sensitive(&self) -> bool {
        matches!(self, Self::Create | Self::Unlock(_) | Self::Startup | Self::Protect(_) | Self::Change | Self::Replace(_))
    }

    pub fn entry(&self) -> Option<&VaultEntry> {
        match self {
            Self::Entry(entry) | Self::Show(entry) | Self::Rename(entry) | Self::Replace(entry)
            | Self::Export(entry) | Self::Delete(entry) => Some(entry),
            _ => None,
        }
    }

    pub fn note(&self) -> Option<&'static str> {
        match self {
            Self::Erase { encrypted: true } => Some(t("Erase all secrets and leave the config in plain text")),
            Self::Erase { encrypted: false } => Some(t("Delete all vault entries and use the system credential store again")),
            Self::Delete(_) => Some(t("Permanently delete this secret?")),
            Self::Export(_) => Some(t("The file will contain the secret in plain text")),
            Self::Startup => Some(t("Cancel to start with defaults")),
            _ => None,
        }
    }
}

impl VaultDialog {
    fn new(kind: DialogKind, ask: bool, opened_at: std::time::Instant) -> Self {
        let ask = ask && matches!(kind, DialogKind::Show(_) | DialogKind::Rename(_) | DialogKind::Replace(_)
            | DialogKind::Export(_) | DialogKind::Delete(_) | DialogKind::Import | DialogKind::Erase { .. });
        let mut fields = vec![String::new(); kind.fields().len() + usize::from(ask)];
        if let DialogKind::Rename(entry) = &kind {
            fields[0].clone_from(&entry.label);
        }
        Self { kind, opened_at, fields, ask, focus: 0, revealed: false, waiting: false, request: 0, issue: None, value: None, scroll: 0, max_scroll: std::cell::Cell::new(0) }
    }

    pub fn menu(&self, unlocked: bool) -> &[&'static str] {
        match self.kind {
            DialogKind::Manage if unlocked => &["Lock now", "Change the master passphrase", "Erase the vault"],
            DialogKind::Manage => &["Change the master passphrase", "Erase the vault"],
            DialogKind::Entry(_) => &["Show", "Rename", "Replace", "Export", "Delete secret"],
            _ => &[],
        }
    }

    fn menu_choice(&self, unlocked: bool, encrypted: bool) -> Option<DialogKind> {
        match &self.kind {
            DialogKind::Manage => match self.focus + usize::from(!unlocked) {
                1 => Some(if unlocked { DialogKind::Change } else { DialogKind::Unlock(AfterUnlock::Change) }),
                2 => Some(if unlocked { DialogKind::Erase { encrypted } } else { DialogKind::Unlock(AfterUnlock::Erase) }),
                _ => None,
            },
            DialogKind::Entry(entry) => {
                let action = [DialogKind::Show, DialogKind::Rename, DialogKind::Replace, DialogKind::Export, DialogKind::Delete][self.focus];
                Some(action(entry.clone()))
            }
            _ => None,
        }
    }

    pub fn field_label(&self, i: usize) -> &str {
        t(self.kind.fields().get(i).copied().unwrap_or("Master passphrase"))
    }

    pub fn can_reveal(&self) -> bool {
        !self.fields.is_empty() && (self.kind.sensitive() || self.ask)
    }

    pub fn display(&self, i: usize) -> String {
        if (self.kind.sensitive() || self.ask && i == self.fields.len() - 1) && !self.revealed {
            "•".repeat(self.fields[i].chars().count())
        } else {
            self.fields[i].replace(['\r', '\n'], " ")
        }
    }

    pub fn primary(&self) -> Option<&'static str> {
        match self.kind {
            DialogKind::Manage | DialogKind::Entry(_) => None,
            DialogKind::Show(_) if self.value.is_some() => Some(t("Copy")),
            DialogKind::Show(_) => self.ask.then(|| t("Show")),
            DialogKind::Erase { .. } | DialogKind::Delete(_) => Some(t("Confirm deletion")),
            DialogKind::Export(_) => Some(t("Export")),
            DialogKind::Import => Some(t("Import")),
            DialogKind::Unlock(_) | DialogKind::Startup | DialogKind::Authorize => Some(t("Unlock")),
            _ => Some(t("Save")),
        }
    }
}

pub(crate) struct VaultLayout {
    pub frame: Rect,
    pub title: Rect,
    pub subtitle: Rect,
    pub rows: Vec<Rect>,
    pub status: Rect,
    pub preview: Rect,
    pub reveal: Rect,
    pub cancel: Rect,
    pub primary: Rect,
}

pub(crate) fn vault_layout(area: Rect, scale: f32, dialog: &VaultDialog, unlocked: bool) -> VaultLayout {
    let pad = 18.0 * scale;
    let fields = dialog.fields.len();
    let menu = dialog.menu(unlocked).len();
    let count = fields.max(menu);
    let pitch = if menu == 0 { 56.0 } else { 36.0 } * scale;
    let preferred_width = if dialog.value.is_some() { 670.0 } else { 520.0 };
    let width = (preferred_width * scale).min((area.w - 16.0 * scale).max(0.0));
    let desired = if matches!(dialog.kind, DialogKind::Show(_)) { 285.0 * scale } else { 148.0 * scale + count as f32 * pitch };
    let height = desired.min((area.h - 8.0 * scale).max(0.0));
    let frame = Rect::new(area.x + (area.w - width) * 0.5, area.y + (area.h - height) * 0.5, width, height).snap();
    let inner = frame.inset(pad.min(width * 0.07), pad.min(height * 0.10));
    let title = Rect::new(inner.x, inner.y, inner.w, 23.0 * scale).snap();
    let subtitle = Rect::new(inner.x, title.bottom(), inner.w, 22.0 * scale).snap();
    let button_h = 32.0 * scale;
    let button_y = inner.bottom() - button_h;
    let button_w = (122.0 * scale).min(inner.w * 0.27);
    let primary = Rect::new(inner.right() - button_w, button_y, button_w, button_h).snap();
    let cancel = Rect::new(if dialog.primary().is_some() { primary.x - button_w - 8.0 * scale } else { primary.x }, button_y, button_w, button_h).snap();
    let reveal = Rect::new(inner.x, button_y, (135.0 * scale).min(inner.w * 0.38), button_h).snap();
    let status = Rect::new(inner.x, button_y - 24.0 * scale, inner.w, 20.0 * scale).snap();
    let top = subtitle.bottom();
    let body = (status.y - top).max(0.0);
    let preview = Rect::new(inner.x, top, inner.w, body).snap();
    let fit = (body / pitch).floor().max(1.0) as usize;
    let first = dialog.focus.saturating_add(1).saturating_sub(fit);
    let rows = (0..count).map(|i| {
        if i >= first && i - first < fit {
            Rect::new(inner.x, top + (i - first) as f32 * pitch, inner.w, (pitch - 4.0 * scale).min(body)).snap()
        } else {
            Rect::default()
        }
    }).collect();
    VaultLayout { frame, title, subtitle, rows, status, preview, reveal, cancel, primary }
}

impl App {
    pub fn vault_request_id(&self) -> u64 {
        self.settings.as_ref().and_then(|page| page.vault_dialog.as_ref()).map_or(self.vault_input_id, |dialog| dialog.request)
    }

    pub(crate) fn open_vault_dialog(&mut self, kind: DialogKind) {
        let now = self.clock();
        let Some(page) = self.settings.as_mut() else { return };
        self.vault_input_id = self.vault_input_id.wrapping_add(1);
        let show = match &kind { DialogKind::Show(entry) => Some(entry.clone()), _ => None };
        let dialog = VaultDialog::new(kind, self.config.vault.remember_minutes == 0, now);
        let ask = dialog.ask;
        page.vault_dialog = Some(dialog);
        self.preedit = Default::default();
        self.dirty = true;
        if let Some(entry) = show.filter(|_| !ask) {
            self.submit_vault_action(VaultAction::Show(entry), None);
        }
    }

    fn submit_vault_action(&mut self, action: VaultAction, passphrase: Option<String>) {
        if let Some(dialog) = self.settings.as_mut().and_then(|page| page.vault_dialog.as_mut()) {
            if dialog.ask {
                if let Some(password) = dialog.fields.last_mut() {
                    password.clear();
                }
                dialog.revealed = false;
            }
            dialog.waiting = true;
            dialog.issue = None;
            self.vault_input_id = self.vault_input_id.wrapping_add(1);
            dialog.request = self.vault_input_id;
            self.push(Effect::VaultManage { action, passphrase, request: self.vault_input_id });
        }
    }

    pub fn vault_reply(&mut self, request: u64, result: Result<VaultReply, VaultIssue>) {
        let Some(page) = self.settings.as_mut() else { return };
        let Some(dialog) = page.vault_dialog.as_mut().filter(|dialog| dialog.waiting && dialog.request == request) else { return };
        dialog.waiting = false;
        match result {
            Err(issue) => dialog.issue = Some(issue),
            Ok(VaultReply::Value(value)) => {
                dialog.fields.clear();
                dialog.revealed = false;
                dialog.value = Some(value);
            }
            Ok(VaultReply::Applied) => {
                let next = match dialog.kind {
                    DialogKind::Unlock(AfterUnlock::Change) => Some(DialogKind::Change),
                    DialogKind::Unlock(AfterUnlock::Erase) => Some(DialogKind::Erase { encrypted: self.config.vault.encrypt_config }),
                    _ => None,
                };
                page.vault_dialog = None;
                if let Some(kind) = next {
                    self.open_vault_dialog(kind);
                }
            }
        }
        self.dirty = true;
    }

    fn confirm_vault_dialog(&mut self) {
        let Some(page) = self.settings.as_mut() else { return };
        let Some(dialog) = page.vault_dialog.as_mut().filter(|dialog| !dialog.waiting) else { return };
        if !dialog.menu(page.vault.unlocked).is_empty() {
            let choice = dialog.menu_choice(page.vault.unlocked, self.config.vault.encrypt_config);
            if let Some(kind) = choice {
                self.open_vault_dialog(kind);
            } else {
                page.vault_dialog = None;
                self.push(Effect::VaultLock);
            }
            return;
        }
        if let Some(value) = dialog.value.clone() {
            self.push(Effect::SetClipboard(value));
            return;
        }
        let passphrase = dialog.ask.then(|| dialog.fields.last().cloned().unwrap_or_default());
        let missing = dialog.fields.iter().position(String::is_empty)
            .filter(|_| matches!(dialog.kind, DialogKind::Create | DialogKind::Unlock(_) | DialogKind::Protect(_) | DialogKind::Change));
        if let Some(i) = missing.or_else(|| passphrase.as_ref().filter(|value| value.is_empty()).map(|_| dialog.fields.len() - 1)) {
            dialog.focus = i;
            dialog.issue = Some(VaultIssue::EmptyPassphrase);
            return;
        }        let field = |i: usize| dialog.fields[i].clone();
        let effect = match &dialog.kind {
            DialogKind::Create => Effect::VaultCreate { passphrase: field(0) },
            DialogKind::Unlock(_) => Effect::VaultUnlock { passphrase: field(0) },
            DialogKind::Change => Effect::VaultChange { old: field(0), new: field(1) },
            DialogKind::Erase { .. } => Effect::VaultErase { passphrase },
            DialogKind::Protect(enabled) => Effect::ProtectConfig { request: self.vault_input_id.wrapping_add(1), enabled: *enabled, passphrase: field(0) },
            _ => {
                let action = match &dialog.kind {
                    DialogKind::Show(entry) => VaultAction::Show(entry.clone()),
                    DialogKind::Rename(entry) => VaultAction::Rename { entry: entry.clone(), label: field(0) },
                    DialogKind::Replace(entry) => VaultAction::Replace { entry: entry.clone(), value: field(0) },
                    DialogKind::Export(entry) => VaultAction::Export { entry: entry.clone(), path: field(0) },
                    DialogKind::Delete(entry) => VaultAction::Delete(entry.clone()),
                    DialogKind::Import => VaultAction::Import { label: field(0), path: field(1) },
                    _ => return,
                };
                self.submit_vault_action(action, passphrase);
                return;
            }
        };
        dialog.fields.iter_mut().for_each(String::clear);
        dialog.revealed = false;
        dialog.waiting = true;
        dialog.issue = None;
        self.vault_input_id = self.vault_input_id.wrapping_add(1);
        dialog.request = self.vault_input_id;
        self.push(effect);
    }

    pub(crate) fn vault_dialog_key(&mut self, key: Key, mods: Mods) {
        let Some(page) = self.settings.as_mut() else { return };
        let Some(dialog) = page.vault_dialog.as_mut() else { return };
        if key == Key::Escape {
            page.vault_dialog = None;
            return;
        }
        if dialog.waiting {
            return;
        }
        let menu = dialog.menu(page.vault.unlocked).len();
        let count = dialog.fields.len().max(menu);
        match key {
            Key::Enter if !mods.ctrl && dialog.kind == DialogKind::Change && dialog.focus == 0 => {
                dialog.focus = 1;
                self.vault_input_id = self.vault_input_id.wrapping_add(1);
            }
            Key::Enter => self.confirm_vault_dialog(),
            Key::Char('C') if mods.ctrl && matches!(dialog.kind, DialogKind::Show(_)) => self.confirm_vault_dialog(),
            Key::Up if dialog.value.is_some() => dialog.scroll = dialog.scroll.saturating_sub(1),
            Key::Down if dialog.value.is_some() => dialog.scroll = (dialog.scroll + 1).min(dialog.max_scroll.get()),
            Key::Tab | Key::Up | Key::Down if count != 0 => {
                let step = if key == Key::Up || mods.shift { count - 1 } else { 1 };
                dialog.focus = (dialog.focus + step) % count;
                self.vault_input_id = self.vault_input_id.wrapping_add(1);
            }
            Key::Backspace if !dialog.fields.is_empty() => {
                let field = &mut dialog.fields[dialog.focus];
                if mods.ctrl { field.clear(); } else { field.pop(); }
                dialog.issue = None;
            }
            Key::Char('V') if mods.ctrl && !dialog.fields.is_empty() => self.push(Effect::RequestVaultPaste(self.vault_input_id)),
            _ => {}
        }
    }

    pub(crate) fn vault_dialog_text(&mut self, text: &str) -> bool {
        let Some(dialog) = self.settings.as_mut().and_then(|page| page.vault_dialog.as_mut()) else { return false };
        if let Some(field) = dialog.fields.get_mut(dialog.focus).filter(|_| !dialog.waiting) {
            let multiline = dialog.focus == 0 && matches!(dialog.kind, DialogKind::Replace(_));
            field.extend(text.chars().filter(|c| !c.is_control() || multiline && matches!(c, '\r' | '\n' | '\t')));
            dialog.issue = None;
            self.dirty = true;
        }
        true
    }

    pub(crate) fn vault_dialog_click(&mut self, area: Rect, x: f32, y: f32) -> bool {
        let scale = self.ui();
        let Some(page) = self.settings.as_mut() else { return false };
        let Some(dialog) = page.vault_dialog.as_mut() else { return false };
        let layout = vault_layout(area, scale, dialog, page.vault.unlocked);
        if layout.cancel.contains(x, y) {
            page.vault_dialog = None;
        } else if !dialog.waiting {
            if dialog.primary().is_some() && layout.primary.contains(x, y) {
                self.confirm_vault_dialog();
            } else if dialog.can_reveal() && layout.reveal.contains(x, y) {
                dialog.revealed = !dialog.revealed;
            } else if let Some(i) = layout.rows.iter().position(|rect| rect.h > 0.0 && rect.contains(x, y)) {
                dialog.focus = i;
                self.vault_input_id = self.vault_input_id.wrapping_add(1);
                if !dialog.menu(page.vault.unlocked).is_empty() {
                    self.confirm_vault_dialog();
                }
            }
        }
        true
    }

    pub(crate) fn vault_dialog_wheel(&mut self, dy: f32) -> bool {
        let Some(dialog) = self.settings.as_mut().and_then(|page| page.vault_dialog.as_mut()) else { return false };
        if dialog.value.is_some() {
            dialog.scroll = if dy > 0.0 { dialog.scroll.saturating_sub(3) } else { (dialog.scroll + 3).min(dialog.max_scroll.get()) };
            self.dirty = true;
        }
        true
    }
}

impl App {
    pub fn prompt_config_unlock(&mut self) {
        self.vault_input_id = self.vault_input_id.wrapping_add(1);
        self.vault_prompt = Some(VaultDialog::new(DialogKind::Startup, false, self.clock()));
        self.set_config_encrypted(true);
    }

    pub fn prompt_vault_access(&mut self, issue: Option<VaultIssue>) {
        self.vault_input_id = self.vault_input_id.wrapping_add(1);
        let mut dialog = VaultDialog::new(DialogKind::Authorize, false, self.clock());
        dialog.issue = issue;
        self.vault_prompt = Some(dialog);
        self.preedit = Default::default();
        self.dirty = true;
    }

    pub fn config_unlock_waiting(&self, request: u64) -> bool {
        request == self.vault_input_id && self.vault_prompt.as_ref().is_some_and(|dialog| dialog.kind == DialogKind::Startup && dialog.waiting)
    }

    pub fn config_unlock_error(&mut self, request: u64, issue: VaultIssue) {
        if self.config_unlock_waiting(request) {
            if let Some(dialog) = self.vault_prompt.as_mut() {
                dialog.waiting = false;
                dialog.issue = Some(issue);
                self.dirty = true;
            }
        }
    }

    pub fn dismiss_config_unlock(&mut self) {
        self.vault_prompt = None;
        self.dirty = true;
    }

    pub(crate) fn vault_prompt_text(&mut self, text: &str) {
        if let Some(dialog) = self.vault_prompt.as_mut().filter(|dialog| !dialog.waiting) {
            dialog.fields[0].extend(text.chars().filter(|ch| !ch.is_control()));
            dialog.issue = None;
            self.dirty = true;
        }
    }

    fn submit_config_password(&mut self) {
        let Some(dialog) = self.vault_prompt.as_mut().filter(|dialog| !dialog.waiting) else { return };
        if dialog.fields[0].is_empty() {
            dialog.issue = Some(VaultIssue::EmptyPassphrase);
            return;
        }
        dialog.waiting = true;
        dialog.revealed = false;
        dialog.issue = None;
        let passphrase = std::mem::take(&mut dialog.fields[0]);
        if dialog.kind == DialogKind::Authorize {
            self.dismiss_config_unlock();
            self.push(Effect::VaultAccessInput(Some(passphrase)));
            return;
        }
        self.vault_input_id = self.vault_input_id.wrapping_add(1);
        self.push(Effect::UnlockConfig { request: self.vault_input_id, passphrase });
    }

    fn cancel_config_password(&mut self) {
        let action = if self.vault_prompt.as_ref().is_some_and(|dialog| dialog.kind == DialogKind::Authorize) {
            Effect::VaultAccessInput(None)
        } else {
            Effect::CancelConfigUnlock
        };
        self.dismiss_config_unlock();
        self.push(action);
    }

    pub(crate) fn vault_prompt_event(&mut self, event: &Event) -> bool {
        let Some(dialog) = self.vault_prompt.as_mut() else { return false };
        match event {
            Event::Key { key, mods, pressed: true } => {
                match key {
                    Key::Escape => self.cancel_config_password(),
                    Key::Enter => self.submit_config_password(),
                    Key::Backspace if !dialog.waiting => {
                        if mods.ctrl { dialog.fields[0].clear(); } else { dialog.fields[0].pop(); }
                        dialog.issue = None;
                    }
                    Key::Char('V') if mods.ctrl && !dialog.waiting => self.push(Effect::RequestVaultPaste(self.vault_input_id)),
                    _ => {}
                }
            }
            Event::Char(ch) | Event::AltChar(ch) => self.vault_prompt_text(&ch.to_string()),
            Event::MouseDown { x, y, .. } | Event::MouseUp { x, y, .. }
                if matches!(self.hit_test(*x, *y), HitArea::Minimize | HitArea::Maximize | HitArea::Close) => return false,
            Event::MouseDown { x, y, button: MouseButton::Left, .. } => {
                let Some(dialog) = self.vault_prompt.as_ref() else { return true };
                let layout = vault_layout(self.chrome.content, self.ui(), dialog, false);
                if layout.cancel.contains(*x, *y) {
                    self.cancel_config_password();
                } else if layout.primary.contains(*x, *y) {
                    self.submit_config_password();
                } else if let Some(dialog) = self.vault_prompt.as_mut().filter(|dialog| !dialog.waiting && layout.reveal.contains(*x, *y)) {
                    dialog.revealed = !dialog.revealed;
                }
            }
            Event::Preedit(_) | Event::Wheel { .. } | Event::DroppedFiles(_) | Event::Pasted(_)
            | Event::MouseDown { .. } | Event::MouseUp { .. } | Event::Key { .. } => {}
            _ => return false,
        }
        self.dirty = true;
        true
    }
}
