use crate::app::{App, Effect};
use crate::input::{Key, Mods};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rename {
    pub tab: usize,
    pub text: String,
}

impl App {
    pub fn open_tab_at(&mut self, profile: crate::profile::Profile, cwd: Option<String>) {
        let cwd = cwd.filter(|_| profile.ssh.is_none() || self.config.ssh.client != crate::config::SshClient::Native);
        let id = self.new_pane_in(profile, cwd);
        let at = if self.tabs.is_empty() { 0 } else { self.active + 1 };
        self.tabs.insert(at, crate::app::Tab { root: crate::panes::Node::Leaf(id), focus: id, zoomed: None, title: None, color: None });
        self.active = at;
        self.overlay = None;
        self.prompt_pending_serial();
        self.dirty = true;
    }

    pub fn find_profile(&self, name: &str) -> Option<crate::profile::Profile> {
        let name = name.trim();
        self.profiles.iter().find(|p| p.id == name).or_else(|| self.profiles.iter().find(|p| p.name.eq_ignore_ascii_case(name))).cloned()
    }

    pub fn run_command(&mut self, command: &[String], cwd: Option<String>) {
        let Some((program, args)) = command.split_first() else { return };
        let mut profile = crate::profile::Profile::new("run", &command.join(" "), program, &[]);
        profile.args = args.to_vec();
        profile.hold = true;
        self.open_tab_at(profile, cwd);
    }

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
        let reps: Vec<_> = self.tabs.iter().enumerate().filter(|(i, _)| !keep(*i, active)).map(|(_, t)| t.focus).collect();
        if !reps.is_empty() {
            self.request_close(crate::close::CloseTarget::Tabs(reps));
        }
    }
}

#[cfg(test)]
#[path = "../test/tabs.rs"]
mod tests;
