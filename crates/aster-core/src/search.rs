use crate::app::App;
use crate::config::SearchOptions;
use crate::input::{Key, Mods};
use crate::panes::PaneId;
use aster_gfx::render::Rect;
use aster_vt::search::SearchOptions as VtOptions;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchBar {
    pub pane: PaneId,
    pub query: String,
    pub error: bool,
    pub missing: bool,
    pub current: Option<usize>,
    pub count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchHit {
    Field,
    Case,
    Regex,
    Word,
    Older,
    Newer,
    Close,
}

pub struct SearchGeometry {
    pub frame: Rect,
    pub field: Rect,
    pub buttons: [(SearchHit, Rect); 6],
}

pub fn geometry(pane: Rect, s: f32) -> SearchGeometry {
    let margin = (8.0 * s).round().min(pane.w / 8.0).min(pane.h / 8.0);
    let w = (380.0 * s).round().min(pane.w - 2.0 * margin);
    let h = (34.0 * s).round().min(pane.h - 2.0 * margin);
    let frame = Rect::new(pane.right() - w - margin, pane.y + margin, w, h);
    let control_scale = s.min(w / 250.0);
    let b = (26.0 * control_scale).round().min(h);
    let pad = (4.0 * control_scale).round();
    let order = [SearchHit::Close, SearchHit::Newer, SearchHit::Older, SearchHit::Word, SearchHit::Regex, SearchHit::Case];
    let mut buttons = [(SearchHit::Close, Rect::default()); 6];
    let mut x = frame.right() - pad;
    for (i, hit) in order.into_iter().enumerate() {
        x -= b;
        buttons[i] = (hit, Rect::new(x, frame.y + (h - b) / 2.0, b, b));
        x -= if i == 2 { 8.0 * control_scale } else { 2.0 * control_scale };
    }
    let field = Rect::new(frame.x + (10.0 * s).round(), frame.y, (x - frame.x - 14.0 * s).max(0.0), h);
    SearchGeometry { frame, field, buttons }
}

fn vt(o: SearchOptions) -> VtOptions {
    VtOptions { regex: o.regex, case_sensitive: o.case_sensitive, whole_word: o.whole_word }
}

impl App {
    pub fn open_search(&mut self) {
        let Some(pane) = self.focused_pane() else { return };
        let selected = self.panes.get(&pane).and_then(|p| p.terminal.lock().selection_text()).filter(|t| !t.contains('\n'));
        let query = selected.or_else(|| self.search.as_ref().map(|s| s.query.clone())).unwrap_or_default();
        self.search = Some(SearchBar { pane, query, error: false, missing: false, current: None, count: 0 });
        self.rerun_search();
    }

    pub fn close_search(&mut self) {
        if let Some(bar) = self.search.take() {
            if let Some(p) = self.panes.get(&bar.pane) {
                p.terminal.lock().clear_search();
            }
            self.dirty = true;
        }
    }

    fn rerun_search(&mut self) {
        let options = vt(self.config.terminal.search_options);
        let Some(bar) = self.search.as_mut() else { return };
        let Some(p) = self.panes.get(&bar.pane) else { return };
        let mut t = p.terminal.lock();
        match t.set_search(&bar.query, options) {
            Ok(()) => {
                bar.error = false;
                bar.missing = !bar.query.is_empty() && !t.search_step(true);
            }
            Err(_) => {
                bar.error = true;
                bar.missing = false;
            }
        }
        self.dirty = true;
    }

    pub fn search_step(&mut self, older: bool) {
        let Some(bar) = self.search.as_mut() else { return };
        if let Some(p) = self.panes.get(&bar.pane) {
            bar.missing = !p.terminal.lock().search_step(older);
        }
        self.dirty = true;
    }

    fn toggle(&mut self, hit: SearchHit) {
        let o = &mut self.config.terminal.search_options;
        match hit {
            SearchHit::Case => o.case_sensitive = !o.case_sensitive,
            SearchHit::Regex => o.regex = !o.regex,
            SearchHit::Word => o.whole_word = !o.whole_word,
            _ => return,
        }
        self.push_save_config();
        self.rerun_search();
    }

    pub(crate) fn search_key(&mut self, key: Key, mods: Mods) -> bool {
        if self.search.is_none() {
            return false;
        }
        match key {
            Key::Escape => self.close_search(),
            Key::Enter | Key::Up if !mods.shift => self.search_step(true),
            Key::Enter | Key::Down => self.search_step(false),
            Key::Up => self.search_step(true),
            Key::Backspace => {
                if let Some(bar) = self.search.as_mut() {
                    if mods.ctrl {
                        let cut = bar.query.trim_end().rfind(char::is_whitespace).map_or(0, |i| i + 1);
                        bar.query.truncate(cut);
                    } else {
                        bar.query.pop();
                    }
                }
                self.rerun_search();
            }
            Key::Char('C') if mods.alt => self.toggle(SearchHit::Case),
            Key::Char('R') if mods.alt => self.toggle(SearchHit::Regex),
            Key::Char('W') if mods.alt => self.toggle(SearchHit::Word),
            _ => return false,
        }
        true
    }

    pub(crate) fn search_char(&mut self, c: char) -> bool {
        let Some(bar) = self.search.as_mut() else { return false };
        if !c.is_control() {
            bar.query.push(c);
            self.rerun_search();
        }
        true
    }

    pub(crate) fn search_paste(&mut self, text: &str) -> bool {
        let Some(bar) = self.search.as_mut() else { return false };
        bar.query.push_str(&text.replace(['\r', '\n'], " "));
        self.rerun_search();
        true
    }

    pub(crate) fn search_geometry(&self) -> Option<SearchGeometry> {
        let bar = self.search.as_ref()?;
        let rect = self.layout.panes.iter().find(|(id, _)| *id == bar.pane)?.1;
        Some(geometry(rect, self.ui()))
    }

    pub(crate) fn search_click(&mut self, x: f32, y: f32) -> bool {
        let Some(g) = self.search_geometry() else { return false };
        if !g.frame.contains(x, y) {
            return false;
        }
        match g.buttons.iter().find(|(_, r)| r.contains(x, y)).map(|(h, _)| *h) {
            Some(SearchHit::Close) => self.close_search(),
            Some(SearchHit::Older) => self.search_step(true),
            Some(SearchHit::Newer) => self.search_step(false),
            Some(hit) => self.toggle(hit),
            None => {}
        }
        true
    }
}

#[cfg(test)]
#[path = "../test/search.rs"]
mod tests;
