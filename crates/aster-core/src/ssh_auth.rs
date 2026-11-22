use crate::app::{App, Effect};
use crate::input::{Key, Mods, MouseButton};
use crate::panes::PaneId;
use aster_gfx::render::Rect;
use std::time::Instant;
use aster_proto::ssh::{InteractiveAnswer, InteractiveChallenge, SshEvent};

pub(crate) enum Row {
    Text(String),
    Link(String),
    Prompt(usize),
}

pub(crate) struct Panel {
    pub challenge: InteractiveChallenge,
    pub opened_at: Instant,
    pub rows: Vec<Row>,
    pub focus: usize,
    pub remember: bool,
    pub pending: bool,
    values: Vec<String>,
    scroll: usize,
}

pub(crate) struct Geometry {
    pub frame: Rect,
    pub title: Rect,
    pub entries: Vec<(usize, Rect)>,
    pub remember: Option<Rect>,
    pub submit: Rect,
    pub cancel: Rect,
    capacity: usize,
}

impl Panel {
    fn new(challenge: InteractiveChallenge, opened_at: Instant) -> Self {
        let mut rows = Vec::new();
        for line in challenge.name.lines().chain(challenge.instructions.lines()) {
            let clean: String = line.chars().filter(|c| !c.is_control()).collect();
            let chars: Vec<_> = clean.chars().collect();
            let mut start = 0;
            for link in crate::links::find_links(&clean).into_iter().filter(|link| {
                let target = link.target.to_ascii_lowercase();
                target.starts_with("https://") || target.starts_with("http://")
            }) {
                add_text(&mut rows, &chars[start..link.start]);
                rows.push(Row::Link(link.target));
                start = link.end;
            }
            add_text(&mut rows, &chars[start..]);
        }
        rows.extend((0..challenge.prompts.len()).map(Row::Prompt));
        let values = vec![String::new(); challenge.prompts.len()];
        Self { challenge, opened_at, rows, values, focus: 0, remember: false, pending: false, scroll: 0 }
    }

    pub(crate) fn display(&self, index: usize) -> String {
        let value = &self.values[index];
        if self.challenge.prompts[index].echo { value.clone() } else { "•".repeat(value.chars().count()) }
    }

    fn insert(&mut self, text: &str) {
        if !self.pending {
            if let Some(value) = self.values.get_mut(self.focus) {
                value.extend(text.chars().filter(|c| !c.is_control()));
            }
        }
    }

    fn answer(&mut self, submit: bool) -> InteractiveAnswer {
        self.pending = true;
        let answers = std::mem::take(&mut self.values);
        InteractiveAnswer { id: self.challenge.id, answers: submit.then_some(answers), remember: submit && self.remember }
    }

    pub(crate) fn geometry(&self, pane: Rect, s: f32) -> Geometry {
        let pad = 12.0 * s;
        let line = 33.0 * s;
        let width = (pane.w - 2.0 * pad).clamp(0.0, 540.0 * s);
        let remember = self.challenge.remember.is_some() && !self.pending;
        let fixed = (if remember { 108.0 } else { 80.0 }) * s;
        let rows = if self.pending { 1 } else { self.rows.len() };
        let capacity = (((pane.h - fixed - 2.0 * pad) / line).floor() as usize).min(rows);
        let height = (fixed + capacity as f32 * line).min(pane.h);
        let frame = Rect::new(pane.x + (pane.w - width) / 2.0, pane.y + (pane.h - height) / 3.0, width, height).snap();
        let body = frame.inset(pad, pad);
        let title = Rect::new(body.x, body.y, body.w, 24.0 * s);
        let start = self.scroll.min(rows.saturating_sub(capacity));
        let entries = (start..start + capacity).map(|index| {
            let row = Rect::new(body.x, title.bottom() + (index - start) as f32 * line, body.w, line - 2.0 * s);
            (index, row)
        }).collect();
        let button_w = (94.0 * s).min(body.w * 0.42).max(0.0);
        let y = body.bottom() - 29.0 * s;
        let cancel = Rect::new(body.right() - button_w, y, button_w, 29.0 * s);
        let submit = Rect::new(cancel.x - button_w - 6.0 * s, y, button_w, 29.0 * s);
        let remember = remember.then(|| Rect::new(body.x, y - 26.0 * s, body.w, 24.0 * s));
        Geometry { frame, title, entries, remember, submit, cancel, capacity }
    }

    fn step(&mut self, backwards: bool, capacity: usize) {
        let count = (self.values.len() + usize::from(self.challenge.remember.is_some())).max(1);
        self.focus = (self.focus + if backwards { count - 1 } else { 1 }) % count;
        if let Some(row) = self.rows.iter().position(|row| matches!(row, Row::Prompt(index) if *index == self.focus)) {
            self.scroll = self.scroll.min(row).max((row + 1).saturating_sub(capacity));
        }
    }
}

fn add_text(rows: &mut Vec<Row>, text: &[char]) {
    let text: String = text.iter().collect();
    if !text.trim().is_empty() { rows.push(Row::Text(text.trim().to_owned())); }
}

impl App {
    pub fn ssh_event(&mut self, pane: PaneId, event: SshEvent) {
        if !self.panes.get(&pane).is_some_and(|pane| pane.exited.is_none()) { return; }
        self.ssh_auth_paste = None;
        let now = self.clock();
        match event {
            SshEvent::Challenge(challenge) => { self.ssh_auth.insert(pane, Panel::new(challenge, now)); }
            SshEvent::Finished => { self.ssh_auth.remove(&pane); }
            SshEvent::Forwards(statuses) => self.set_ssh_forwards(pane, statuses),
        }
        self.dirty = true;
    }

    pub(crate) fn ssh_auth_active(&self) -> bool {
        self.focused_pane().is_some_and(|pane| self.ssh_auth.contains_key(&pane))
    }

    pub(crate) fn ssh_auth_key(&mut self, key: Key, mods: Mods) {
        let Some(pane) = self.focused_pane() else { return };
        let bounds = self.pane_terminal_rect(pane, self.layout.panes.iter().find(|(id, _)| *id == pane).map_or(self.chrome.content, |(_, rect)| *rect));
        let s = self.ui();
        let Some(panel) = self.ssh_auth.get_mut(&pane) else { return };
        self.ssh_auth_paste = None;
        match key {
            Key::Escape | Key::Char('C') if key == Key::Escape || mods.ctrl => self.submit_ssh_auth(pane, false),
            _ if panel.pending => {}
            Key::Enter => self.submit_ssh_auth(pane, true),
            Key::Tab => panel.step(mods.shift, panel.geometry(bounds, s).capacity),
            Key::Space if panel.focus == panel.values.len() && panel.challenge.remember.is_some() => panel.remember = !panel.remember,
            Key::Backspace => {
                if let Some(value) = panel.values.get_mut(panel.focus) {
                    if mods.ctrl { value.clear(); } else { value.pop(); }
                }
            }
            Key::Char('V') if mods.ctrl && panel.focus < panel.values.len() => {
                self.vault_input_id = self.vault_input_id.wrapping_add(1);
                self.ssh_auth_paste = Some((self.vault_input_id, pane, panel.challenge.id, panel.focus));
                self.push(Effect::RequestVaultPaste(self.vault_input_id));
            }
            _ => {}
        }
        self.dirty = true;
    }

    fn submit_ssh_auth(&mut self, pane: PaneId, submit: bool) {
        if let Some(panel) = self.ssh_auth.get_mut(&pane) {
            if panel.pending && submit { return; }
            let answer = panel.answer(submit);
            self.push(Effect::SshInteractive { pane, answer });
        }
    }

    pub(crate) fn ssh_auth_text(&mut self, text: &str) -> bool {
        let Some(panel) = self.focused_pane().and_then(|pane| self.ssh_auth.get_mut(&pane)) else { return false };
        panel.insert(text);
        self.dirty = true;
        true
    }

    pub(crate) fn ssh_auth_pasted(&mut self, input: u64, text: &str) -> bool {
        let Some((token, pane, challenge, field)) = self.ssh_auth_paste else { return false };
        if token != input { return false; }
        self.ssh_auth_paste = None;
        if self.focused_pane() == Some(pane) {
            if let Some(panel) = self.ssh_auth.get_mut(&pane).filter(|p| p.challenge.id == challenge && p.focus == field) {
                panel.insert(text);
                self.dirty = true;
            }
        }
        true
    }

    pub(crate) fn ssh_auth_click(&mut self, pane: PaneId, bounds: Rect, x: f32, y: f32, button: MouseButton) -> bool {
        let s = self.ui();
        let Some(panel) = self.ssh_auth.get_mut(&pane) else { return false };
        if button != MouseButton::Left { return true; }
        self.ssh_auth_paste = None;
        let geo = panel.geometry(bounds, s);
        if geo.cancel.contains(x, y) { self.submit_ssh_auth(pane, false); }
        else if !panel.pending {
            if geo.submit.contains(x, y) { self.submit_ssh_auth(pane, true); }
            else if geo.remember.is_some_and(|rect| rect.contains(x, y)) { panel.remember = !panel.remember; }
            else if let Some((index, _)) = geo.entries.iter().find(|(_, rect)| rect.contains(x, y)) {
                match &panel.rows[*index] {
                    Row::Prompt(index) => panel.focus = *index,
                    Row::Link(url) => self.effects.push(Effect::Open(url.clone())),
                    Row::Text(_) => {}
                }
            }
        }
        self.dirty = true;
        true
    }

    pub(crate) fn ssh_auth_wheel(&mut self, pane: PaneId, bounds: Rect, dy: f32) -> bool {
        let s = self.ui();
        let Some(panel) = self.ssh_auth.get_mut(&pane) else { return false };
        if !panel.pending {
            let capacity = panel.geometry(bounds, s).capacity;
            panel.scroll = panel.scroll.saturating_add_signed(if dy > 0.0 { -2 } else { 2 }).min(panel.rows.len().saturating_sub(capacity));
            self.dirty = true;
        }
        true
    }
}

#[cfg(test)]
#[path = "../test/ssh_auth.rs"]
mod tests;
