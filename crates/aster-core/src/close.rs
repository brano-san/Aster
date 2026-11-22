use crate::app::{App, Effect};
use crate::input::Key;
use crate::panes::PaneId;
use aster_gfx::render::Rect;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub const BUTTONS: [&str; 2] = ["Close", "Cancel"];
pub const LISTED: usize = 6;

const COMMAND_INTERVAL: Duration = Duration::from_secs(2);

pub(crate) fn command_probe_id() -> u64 {
    static REQUEST: AtomicU64 = AtomicU64::new(1);
    REQUEST.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum CommandWatch {
    #[default]
    Off,
    Waiting { busy: bool, next: Instant },
    Polling { busy: bool, request: u64 },
    Finished,
}

impl CommandWatch {
    pub(crate) fn deadline(self) -> Option<Instant> {
        if let Self::Waiting { next, .. } = self { Some(next) } else { None }
    }

    pub(crate) fn armed(self) -> bool {
        matches!(self, Self::Waiting { .. } | Self::Polling { .. })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CloseTarget {
    Pane(PaneId),
    PaneProfile(PaneId, String),
    Tabs(Vec<PaneId>),
    Window,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloseConfirm {
    pub(crate) opened_at: Instant,
    pub target: CloseTarget,
    pub programs: Vec<String>,
    pub ssh: bool,
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
        self.close_confirm.as_ref().map(|c| geometry(self.chrome.content, self.ui(), c))
    }

    fn target_panes(&self, target: &CloseTarget) -> Vec<PaneId> {
        match target {
            CloseTarget::Pane(id) | CloseTarget::PaneProfile(id, _) => vec![*id],
            CloseTarget::Tabs(reps) => self.tabs.iter().filter(|t| reps.iter().any(|p| t.root.contains(*p))).flat_map(|t| t.root.panes()).collect(),
            CloseTarget::Window => self.tabs.iter().flat_map(|t| t.root.panes()).collect(),
        }
    }

    fn live_shells<'a>(&'a self, panes: &'a [PaneId]) -> impl Iterator<Item = (&'a crate::profile::Profile, u32)> + 'a {
        panes.iter().filter_map(|id| self.panes.get(id)).filter(|p| p.exited.is_none())
            .filter_map(|p| Some((&p.profile, p.pid.filter(|&pid| pid != 0)?)))
    }

    fn command_pids(&self, tab: &crate::app::Tab) -> Vec<u32> {
        self.live_shells(&tab.root.panes()).filter(|(profile, _)| profile.is_local()).map(|(_, pid)| pid).collect()
    }

    pub(crate) fn busy_programs(&self, panes: &[PaneId]) -> Vec<String> {
        let Some(probe) = self.children_probe.as_ref() else { return Vec::new() };
        let mut out: Vec<String> = Vec::new();
        for (_, pid) in self.live_shells(panes) {
            for name in probe(pid) {
                if !out.contains(&name) {
                    out.push(name);
                }
            }
        }
        out
    }

    pub(crate) fn can_watch_command(&self, index: usize) -> bool {
        self.tabs.get(index).is_some_and(|t| t.command_watch.armed() || !self.command_pids(t).is_empty())
    }

    pub(crate) fn toggle_command_watch(&mut self) {
        if !self.can_watch_command(self.active) {
            return;
        }
        let now = Instant::now();
        let watch = &mut self.tabs[self.active].command_watch;
        *watch = if watch.armed() { CommandWatch::Off } else { CommandWatch::Waiting { busy: false, next: now } };
        self.poll_commands(now);
        self.dirty = true;
    }

    pub(crate) fn poll_commands(&mut self, now: Instant) {
        for i in 0..self.tabs.len() {
            let tab = &self.tabs[i];
            let CommandWatch::Waiting { busy, next } = tab.command_watch else { continue };
            if now < next {
                continue;
            }
            let pids = self.command_pids(tab);
            if pids.is_empty() {
                self.tabs[i].command_watch = if busy { CommandWatch::Finished } else { CommandWatch::Off };
                if busy {
                    self.notify_command_finished(i);
                }
                self.dirty = true;
            } else {
                let request = command_probe_id();
                self.tabs[i].command_watch = CommandWatch::Polling { busy, request };
                self.push(Effect::ProbeCommand { request, pids });
                self.dirty = true;
            }
        }
    }

    pub fn command_probe_finished(&mut self, request: u64, result: Option<bool>) {
        if self.finish_pane_profile_probe(request, result) {
            return;
        }
        let Some(index) = self.tabs.iter().position(|t| matches!(t.command_watch, CommandWatch::Polling { request: r, .. } if r == request)) else { return };
        let tab = &mut self.tabs[index];
        let CommandWatch::Polling { busy, .. } = tab.command_watch else { return };
        if busy && result == Some(false) {
            tab.command_watch = CommandWatch::Finished;
            self.notify_command_finished(index);
        } else {
            tab.command_watch = CommandWatch::Waiting { busy: result.unwrap_or(busy), next: Instant::now() + COMMAND_INTERVAL };
            self.schedule_timer();
        }
        self.dirty = true;
    }

    pub(crate) fn request_close(&mut self, target: CloseTarget) {
        let panes = self.target_panes(&target);
        let mut programs = if self.config.terminal.confirm_close { self.busy_programs(&panes) } else { Vec::new() };
        let mut ssh = false;
        if self.config.ssh.warn_on_close {
            for pane in panes.iter().filter_map(|pane| self.panes.get(pane)).filter(|pane| pane.exited.is_none() && pane.profile.ssh.is_some()) {
                ssh = true;
                let name = if pane.profile.name.is_empty() { pane.profile.ssh.as_deref().unwrap_or_default() } else { &pane.profile.name };
                if !programs.iter().any(|item| item == name) { programs.push(name.to_owned()); }
            }
        }
        let pinned = matches!(&target, CloseTarget::Tabs(reps) if self.tabs.iter().any(|tab| tab.pinned && reps.contains(&tab.focus)));
        if programs.is_empty() && !pinned && !ssh {
            return self.close_now(target);
        }
        self.overlay = None;
        self.menu = None;
        self.close_confirm = Some(CloseConfirm { target, programs, ssh, button: 0, opened_at: self.clock() });
        self.dirty = true;
    }

    fn close_now(&mut self, target: CloseTarget) {
        match target {
            CloseTarget::Pane(id) => self.close_pane(id),
            CloseTarget::PaneProfile(pane, profile) => self.replace_pane_profile(pane, &profile),
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
