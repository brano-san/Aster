use crate::app::{App, Effect};
use crate::input::Key;
use crate::panes::PaneId;
use aster_gfx::render::Rect;

pub const BUTTONS: [&str; 2] = ["Close", "Cancel"];
pub const LISTED: usize = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CloseTarget {
    Pane(PaneId),
    Tabs(Vec<PaneId>),
    Window,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseConfirm {
    pub target: CloseTarget,
    pub programs: Vec<String>,
    pub button: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CloseGeometry {
    pub frame: Rect,
    pub title: Rect,
    pub list: Rect,
    pub buttons: [Rect; 2],
}

pub fn geometry(content: Rect, s: f32, confirm: &CloseConfirm) -> CloseGeometry {
    let shown = confirm.programs.len().min(LISTED) + usize::from(confirm.programs.len() > LISTED);
    let w = (440.0 * s).min(content.w - 32.0 * s).max(220.0 * s).round();
    let pad = (16.0 * s).round();
    let title_h = (28.0 * s).round();
    let row = (20.0 * s).round();
    let list_h = shown as f32 * row;
    let button_h = (32.0 * s).round();
    let h = pad + title_h + (6.0 * s).round() + list_h + pad + button_h + pad;
    let x = (content.x + (content.w - w) / 2.0).round();
    let y = (content.y + ((content.h - h) / 3.0).max(12.0 * s)).round();
    let frame = Rect::new(x, y, w, h);
    let title = Rect::new(x + pad, y + pad, w - 2.0 * pad, title_h);
    let list = Rect::new(x + pad, title.bottom() + (6.0 * s).round(), w - 2.0 * pad, list_h);
    let bw = (96.0 * s).round();
    let gap = (8.0 * s).round();
    let by = list.bottom() + pad;
    let cancel = Rect::new(frame.right() - pad - bw, by, bw, button_h);
    let close = Rect::new(cancel.x - gap - bw, by, bw, button_h);
    CloseGeometry { frame, title, list, buttons: [close, cancel] }
}

impl App {
    pub fn close_confirm(&self) -> Option<&CloseConfirm> {
        self.close_confirm.as_ref()
    }

    pub fn close_geometry(&self) -> Option<CloseGeometry> {
        self.close_confirm.as_ref().map(|c| geometry(self.chrome.content, self.scale, c))
    }

    fn target_panes(&self, target: &CloseTarget) -> Vec<PaneId> {
        match target {
            CloseTarget::Pane(id) => vec![*id],
            CloseTarget::Tabs(reps) => self.tabs.iter().filter(|t| reps.iter().any(|p| t.root.contains(*p))).flat_map(|t| t.root.panes()).collect(),
            CloseTarget::Window => self.tabs.iter().flat_map(|t| t.root.panes()).collect(),
        }
    }

    pub(crate) fn busy_programs(&self, panes: &[PaneId]) -> Vec<String> {
        let Some(probe) = self.children_probe.as_ref() else { return Vec::new() };
        let mut out: Vec<String> = Vec::new();
        for id in panes {
            let Some(pane) = self.panes.get(id).filter(|p| p.exited.is_none()) else { continue };
            let Some(pid) = pane.pid.filter(|&p| p != 0) else { continue };
            for name in probe(pid) {
                if !out.contains(&name) {
                    out.push(name);
                }
            }
        }
        out
    }

    pub(crate) fn request_close(&mut self, target: CloseTarget) {
        let programs = if self.config.terminal.confirm_close { self.busy_programs(&self.target_panes(&target)) } else { Vec::new() };
        if programs.is_empty() {
            return self.close_now(target);
        }
        self.overlay = None;
        self.menu = None;
        self.close_confirm = Some(CloseConfirm { target, programs, button: 0 });
        self.dirty = true;
    }

    fn close_now(&mut self, target: CloseTarget) {
        match target {
            CloseTarget::Pane(id) => self.close_pane(id),
            CloseTarget::Tabs(reps) => {
                for rep in reps {
                    if let Some(i) = self.tabs.iter().position(|t| t.root.contains(rep)) {
                        self.close_tab(i);
                    }
                }
            }
            CloseTarget::Window => {
                self.closing = Some(self.snapshot());
                self.close_all();
                self.push(Effect::Close);
            }
        }
        self.dirty = true;
    }

    fn finish_close(&mut self, button: usize) {
        let Some(c) = self.close_confirm.take() else { return };
        self.dirty = true;
        if button == 0 {
            self.close_now(c.target);
        }
    }

    pub(crate) fn close_key(&mut self, key: Key) {
        let Some(c) = self.close_confirm.as_mut() else { return };
        self.dirty = true;
        match key {
            Key::Escape => self.finish_close(1),
            Key::Enter | Key::Space => {
                let b = c.button;
                self.finish_close(b);
            }
            Key::Left | Key::Right | Key::Up | Key::Down | Key::Tab => c.button = 1 - c.button,
            _ => {}
        }
    }

    pub(crate) fn close_click(&mut self, x: f32, y: f32) {
        let Some(geo) = self.close_geometry() else { return };
        if let Some(i) = geo.buttons.iter().position(|b| b.contains(x, y)) {
            self.finish_close(i);
        } else if !geo.frame.contains(x, y) {
            self.finish_close(1);
        }
    }
}

#[cfg(test)]
#[path = "../test/close.rs"]
mod tests;
