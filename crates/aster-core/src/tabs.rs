use crate::app::{App, Effect};
use crate::input::{Key, Mods};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rename {
    pub tab: usize,
    pub text: String,
}

impl App {
    pub fn renaming(&self) -> Option<&Rename> {
        self.rename.as_ref()
    }

    pub(crate) fn start_rename(&mut self, tab: usize) {
        if tab < self.tabs.len() {
            self.activate(tab);
            self.rename = Some(Rename { tab, text: self.tab_title(tab) });
            self.dirty = true;
        }
    }

    pub(crate) fn commit_rename(&mut self) {
        let Some(r) = self.rename.take() else { return };
        if let Some(tab) = self.tabs.get_mut(r.tab) {
            let text = r.text.trim();
            tab.title = (!text.is_empty()).then(|| text.to_owned());
        }
        self.dirty = true;
    }

    pub(crate) fn rename_key(&mut self, key: Key, mods: Mods) {
        let Some(r) = self.rename.as_mut() else { return };
        self.dirty = true;
        match key {
            Key::Enter | Key::Tab => self.commit_rename(),
            Key::Escape => self.rename = None,
            Key::Backspace if mods.ctrl => r.text.clear(),
            Key::Backspace => {
                r.text.pop();
            }
            Key::Char('V') if mods.ctrl => self.push(Effect::RequestPaste),
            _ => {}
        }
    }

    pub(crate) fn rename_text(&mut self, text: &str) {
        let Some(r) = self.rename.as_mut() else { return };
        r.text.extend(text.chars().filter(|c| !c.is_control()));
        self.dirty = true;
    }

    pub(crate) fn close_tabs_except(&mut self, keep: impl Fn(usize, usize) -> bool) {
        let active = self.active;
        for i in (0..self.tabs.len()).rev() {
            if !keep(i, active) {
                self.close_tab(i);
            }
        }
    }
}

#[cfg(test)]
#[path = "../test/tabs.rs"]
mod tests;
