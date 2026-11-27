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
    Waiting { pane: PaneId, after: u64, busy: bool, next: Instant },
    Polling { pane: PaneId, after: u64, busy: bool, request: u64 },
    Marked { pane: PaneId, after: u64 },
    Finished,
}

impl CommandWatch {
    pub(crate) fn deadline(self) -> Option<Instant> {
        if let Self::Waiting { next, .. } = self { Some(next) } else { None }
    }

    pub(crate) fn armed(self) -> bool {
        matches!(self, Self::Waiting { .. } | Self::Polling { .. } | Self::Marked { .. })
    }

    fn pane(self) -> Option<PaneId> {
        match self {
            Self::Waiting { pane, .. } | Self::Polling { pane, .. } | Self::Marked { pane, .. } => Some(pane),
            _ => None,
        }
    }

    fn accepts(self, pane: PaneId, seq: u64) -> bool {
        match self {
            Self::Waiting { pane: target, after, .. } | Self::Polling { pane: target, after, .. } | Self::Marked { pane: target, after } =>
                target == pane && seq > after,
            _ => false,
        }
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
            CloseTarget::Window => self.all_tabs().flat_map(|t| t.root.panes()).collect(),
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
        self.tabs.get(index).is_some_and(|t| t.command_watch.armed() || self.marked_shell(t.focus) || !self.command_pids(t).is_empty())
    }

    fn marked_shell(&self, pane: PaneId) -> bool {
        self.panes.get(&pane).is_some_and(|p| p.exited.is_none() && p.terminal.lock().shell_integrated())
    }

    pub(crate) fn toggle_command_watch(&mut self) {
        if !self.can_watch_command(self.active) {
            return;
        }
        let now = Instant::now();
        let tab = &self.tabs[self.active];
        let pane = tab.focus;
        let after = self.terminal(pane).map_or(0, |terminal| terminal.lock().output_seq());
        self.tabs[self.active].command_watch = if tab.command_watch.armed() {
            CommandWatch::Off
        } else if self.marked_shell(pane) {
            CommandWatch::Marked { pane, after }
        } else {
            CommandWatch::Waiting { pane, after, busy: false, next: now }
        };
        self.poll_commands(now);
        self.dirty = true;
    }

    pub(crate) fn poll_commands(&mut self, now: Instant) {
        let ready: Vec<_> = self.all_tabs().filter_map(|tab| {
            let CommandWatch::Waiting { pane, after, busy, next } = tab.command_watch else { return None };
            if now < next { return None; }
            let (watch, probe) = if self.marked_shell(pane) {
                (CommandWatch::Marked { pane, after }, None)
            } else {
                let pids = self.command_pids(tab);
                if pids.is_empty() {
                    (if busy { CommandWatch::Finished } else { CommandWatch::Off }, None)
                } else {
                    let request = command_probe_id();
                    (CommandWatch::Polling { pane, after, busy, request }, Some(Effect::ProbeCommand { request, pids }))
                }
            };
            Some((tab.key, watch, probe))
        }).collect();
        for (key, watch, probe) in ready {
            if let Some(tab) = self.all_tabs_mut().find(|tab| tab.key == key) {
                tab.command_watch = watch;
            }
            if let Some(probe) = probe {
                self.push(probe);
            } else if watch == CommandWatch::Finished {
                self.notify_command_finished(key, None);
            }
            self.dirty = true;
        }
    }

    pub fn command_probe_finished(&mut self, request: u64, result: Option<bool>) {
        if self.finish_pane_profile_probe(request, result) {
            return;
        }
        let Some((key, pane, after, busy)) = self.all_tabs().find_map(|tab| match tab.command_watch {
            CommandWatch::Polling { pane, after, busy, request: pending } if pending == request => Some((tab.key, pane, after, busy)),
            _ => None,
        }) else { return };
        let watch = if self.marked_shell(pane) {
            CommandWatch::Marked { pane, after }
        } else if busy && result == Some(false) {
            CommandWatch::Finished
        } else {
            CommandWatch::Waiting { pane, after, busy: result.unwrap_or(busy), next: Instant::now() + COMMAND_INTERVAL }
        };
        if let Some(tab) = self.all_tabs_mut().find(|tab| tab.key == key) {
            tab.command_watch = watch;
        }
        if watch == CommandWatch::Finished {
            self.notify_command_finished(key, None);
        } else if watch.deadline().is_some() {
            self.schedule_timer();
        }
        self.dirty = true;
    }

    pub(crate) fn shell_command_finished(&mut self, pane: PaneId, code: Option<i32>, seq: u64) {
        let finished = self.all_tabs_mut().find(|tab| tab.command_watch.accepts(pane, seq)).map(|tab| {
            tab.command_watch = CommandWatch::Finished;
            tab.key
        });
        if let Some(key) = finished {
            self.notify_command_finished(key, code);
            self.dirty = true;
        }
    }

    pub(crate) fn clear_command_watch(&mut self, pane: PaneId) {
        if let Some(tab) = self.all_tabs_mut().find(|tab| tab.command_watch.pane() == Some(pane)) {
            tab.command_watch = CommandWatch::Off;
        }
    }

    pub(crate) fn shell_command_session_ended(&mut self, pane: PaneId, code: Option<u32>) {
        let Some((key, watch)) = self.all_tabs().find(|tab| tab.command_watch.pane() == Some(pane)).map(|tab| (tab.key, tab.command_watch)) else { return };
        if !matches!(watch, CommandWatch::Marked { .. }) && !self.marked_shell(pane) {
            return;
        }
        let running = self.terminal(pane).is_some_and(|terminal| terminal.lock().shell_command_running());
        if let Some(tab) = self.all_tabs_mut().find(|tab| tab.key == key) {
            tab.command_watch = if running { CommandWatch::Finished } else { CommandWatch::Off };
        }
        if running {
            self.notify_command_finished(key, code.and_then(|code| i32::try_from(code).ok()));
        }
    }

    pub(crate) fn jump_shell_prompt(&mut self, older: bool) {
        if self.focused_pane().and_then(|pane| self.terminal(pane))
            .is_some_and(|terminal| terminal.lock().jump_prompt(older)) {
            self.dirty = true;
        }
    }

    pub(crate) fn copy_shell_command_output(&mut self) {
        if let Some(text) = self.focused_pane().and_then(|pane| self.terminal(pane))
            .and_then(|terminal| terminal.lock().latest_command_output()) {
            self.push(Effect::SetClipboard(text));
        }
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
