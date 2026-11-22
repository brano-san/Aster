use crate::app::{App, Effect};
use crate::close::{command_probe_id, CloseConfirm, CloseTarget};
use crate::input::{Key, Mods};
use crate::panes::PaneId;
use aster_gfx::render::Rect;

pub(crate) struct Rename {
    pub pane: PaneId,
    pub text: String,
}

pub(crate) struct ProfileProbe {
    pub request: u64,
    pub pane: PaneId,
    pub profile: String,
}

impl App {
    pub(crate) fn pane_header(&self, pane: PaneId, rect: Rect) -> Rect {
        let shown = self.panes.get(&pane).is_some_and(|p| {
            self.config.appearance.pane_titles || p.name.is_some() || self.pane_rename.as_ref().is_some_and(|edit| edit.pane == pane)
        });
        let height = if shown { (24.0 * self.ui()).round().min((rect.h / 2.0).floor()) } else { 0.0 };
        Rect::new(rect.x, rect.y, rect.w, height)
    }

    pub(crate) fn pane_terminal_rect(&self, pane: PaneId, rect: Rect) -> Rect {
        let height = self.pane_header(pane, rect).h;
        Rect::new(rect.x, rect.y + height, rect.w, rect.h - height)
    }

    pub(crate) fn pane_header_at(&self, x: f32, y: f32) -> Option<PaneId> {
        let (pane, rect) = self.pane_at(x, y)?;
        self.pane_header(pane, rect).contains(x, y).then_some(pane)
    }

    pub(crate) fn start_pane_rename(&mut self, pane: PaneId) {
        if let Some(p) = self.panes.get(&pane) {
            self.pane_rename = Some(Rename { pane, text: p.name.clone().unwrap_or_default() });
            self.dirty = true;
        }
    }

    pub(crate) fn commit_pane_rename(&mut self) {
        let Some(edit) = self.pane_rename.take() else { return };
        if let Some(p) = self.panes.get_mut(&edit.pane) {
            let text = edit.text.trim();
            p.name = (!text.is_empty()).then(|| text.to_owned());
        }
        self.dirty = true;
    }

    pub(crate) fn pane_rename_key(&mut self, key: Key, mods: Mods) {
        let Some(edit) = self.pane_rename.as_mut() else { return };
        self.dirty = true;
        match key {
            Key::Enter | Key::Tab => self.commit_pane_rename(),
            Key::Escape => self.pane_rename = None,
            Key::Backspace if mods.ctrl => edit.text.clear(),
            Key::Backspace => {
                edit.text.pop();
            }
            Key::Char('V') if mods.ctrl => self.push(Effect::RequestPaste),
            _ => {}
        }
    }

    pub(crate) fn pane_rename_text(&mut self, text: &str) {
        if let Some(edit) = self.pane_rename.as_mut() {
            edit.text.extend(text.chars().filter(|c| !c.is_control()));
            self.dirty = true;
        }
    }

    pub(crate) fn request_pane_profile(&mut self, pane: PaneId, profile: String) {
        let profile = profile.strip_prefix(crate::picker::RECENT).unwrap_or(&profile).to_owned();
        let Some(old) = self.panes.get(&pane).filter(|_| self.focused_pane() == Some(pane)) else { return };
        let pid = old.pid.filter(|&pid| pid != 0 && old.exited.is_none() && self.config.terminal.confirm_close);
        self.pane_profile_probe = None;
        if let Some(pid) = pid {
            let request = command_probe_id();
            self.pane_profile_probe = Some(ProfileProbe { request, pane, profile });
            self.push(Effect::ProbeCommand { request, pids: vec![pid] });
        } else {
            self.replace_pane_profile(pane, &profile);
        }
    }

    pub(crate) fn finish_pane_profile_probe(&mut self, request: u64, busy: Option<bool>) -> bool {
        let Some(probe) = self.pane_profile_probe.take_if(|probe| probe.request == request) else { return false };
        if self.focused_pane() != Some(probe.pane) || self.close_confirm.is_some() || self.overlay.is_some() {
            return true;
        }
        if busy == Some(false) || self.panes.get(&probe.pane).is_some_and(|p| p.exited.is_some()) {
            self.replace_pane_profile(probe.pane, &probe.profile);
        } else {
            self.menu = None;
            self.commit_pane_rename();
            self.close_confirm = Some(CloseConfirm { target: CloseTarget::PaneProfile(probe.pane, probe.profile), programs: Vec::new(), button: 0 });
            self.dirty = true;
        }
        true
    }

    pub(crate) fn replace_pane_profile(&mut self, pane: PaneId, id: &str) {
        let Some(index) = self.tabs.iter().position(|tab| tab.root.contains(pane)) else { return };
        let Some(old) = self.panes.get(&pane) else { return };
        let Some(profile) = self.profiles.iter().find(|profile| profile.id == id && profile.layout.is_none()).cloned() else { return };
        let name = old.name.clone();
        let new = self.new_pane_in(profile, None);
        self.panes.get_mut(&new).unwrap().name = name;
        let tab = &mut self.tabs[index];
        tab.root.replace_pane(pane, new);
        if tab.focus == pane {
            tab.focus = new;
        }
        if tab.zoomed == Some(pane) {
            tab.zoomed = Some(new);
        }
        self.update_pane_references(pane, new);
        self.release_pane(pane);
        self.note_recent(id);
        self.prompt_pending_serial();
        self.dirty = true;
    }
}

#[cfg(test)]
#[path = "../test/pane_header.rs"]
mod tests;
