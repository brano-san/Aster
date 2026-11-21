use crate::app::App;
use crate::input::Key;
use crate::keymap::Action;
use aster_gfx::render::Rect;

#[derive(Clone, Copy)]
pub(crate) enum Entry {
    NewTab,
    Recent(usize),
    AllProfiles,
    Settings,
}

pub(crate) struct StartPage {
    pub entries: Vec<Entry>,
    pub selected: usize,
    bounds: Rect,
}

impl StartPage {
    pub fn row(&self, index: usize) -> Rect {
        let h = self.bounds.h / self.entries.len() as f32;
        Rect::new(self.bounds.x, self.bounds.y + index as f32 * h, self.bounds.w, h)
    }

    pub fn hit(&self, x: f32, y: f32) -> Option<usize> {
        (0..self.entries.len()).find(|&i| self.row(i).contains(x, y))
    }
}

impl App {
    pub(crate) fn start_page(&self) -> StartPage {
        let mut entries = vec![Entry::NewTab];
        let recents = self.recent.iter().filter_map(|id| self.profiles.iter().position(|p| &p.id == id && !self.config.profile_hidden(p)));
        entries.extend(recents.take(5).map(Entry::Recent));
        entries.extend([Entry::AllProfiles, Entry::Settings]);
        let s = self.ui();
        let area = self.chrome.content;
        let w = (400.0 * s).min((area.w - 24.0 * s).max(0.0)).round();
        let h = ((36.0 * s).round() * entries.len() as f32).min(area.h);
        let bounds = Rect::new((area.x + (area.w - w) / 2.0).round(), (area.y + (area.h - h) / 2.0).round(), w, h);
        StartPage { selected: self.start_selected.min(entries.len() - 1), entries, bounds }
    }

    pub(crate) fn launch_start_entry(&mut self, entry: Entry) {
        match entry {
            Entry::NewTab => self.open_default_tab(),
            Entry::Recent(i) => {
                let profile = self.profiles[i].clone();
                self.note_recent(&profile.id);
                self.open_tab(profile);
            }
            Entry::AllProfiles => self.run(Action::ProfileSelector),
            Entry::Settings => self.run(Action::Settings),
        }
    }

    pub(crate) fn start_key(&mut self, key: Key) {
        let page = self.start_page();
        let count = page.entries.len();
        self.start_selected = match key {
            Key::Up => (page.selected + count - 1) % count,
            Key::Down => (page.selected + 1) % count,
            Key::Enter => return self.launch_start_entry(page.entries[page.selected]),
            _ => return,
        };
        self.dirty = true;
    }
}

#[cfg(test)]
#[path = "../test/start.rs"]
mod tests;
