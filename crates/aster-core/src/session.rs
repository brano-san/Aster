use crate::app::{App, Tab};
use crate::panes::{Axis, Node, PaneId};
use crate::profile::Profile;
use serde::{Deserialize, Serialize};

pub const BUFFER_LINES: usize = 1000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    pub windows: Vec<WindowState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_folders: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub tabs: Vec<TabState>,
    pub active: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabState {
    #[serde(default)]
    pub pinned: bool,
    pub root: NodeState,
    pub focus: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum NodeState {
    Pane(Box<PaneState>),
    Split { vertical: bool, ratio: f32, a: Box<NodeState>, b: Box<NodeState> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ProfileLayout {
    Pane { profile: String, cwd: Option<String> },
    Split { vertical: bool, ratio: f32, a: Box<ProfileLayout>, b: Box<ProfileLayout> },
}

impl ProfileLayout {
    fn resolve(&self, profiles: &[Profile]) -> Result<NodeState, String> {
        Ok(match self {
            Self::Pane { profile, cwd } => {
                let source = profiles.iter().find(|p| &p.id == profile && p.layout.is_none()).ok_or_else(|| profile.clone())?;
                let mut source = source.clone();
                if cwd.is_some() {
                    source.cwd.clone_from(cwd);
                }
                NodeState::Pane(Box::new(PaneState { profile: source, cwd: cwd.clone(), buffer: Vec::new(), name: None, host_id: None }))
            }
            Self::Split { vertical, ratio, a, b } => NodeState::Split {
                vertical: *vertical, ratio: if ratio.is_finite() { *ratio } else { 0.5 },
                a: Box::new(a.resolve(profiles)?), b: Box::new(b.resolve(profiles)?),
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PaneState {
    pub profile: Profile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub buffer: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_id: Option<u64>,
}

impl SessionState {
    pub fn parse(text: &str) -> Option<SessionState> {
        serde_json::from_str(text).ok()
    }

    pub fn to_text(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

fn restore_text(lines: &[String]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in lines {
        out.extend_from_slice(line.as_bytes());
        out.extend_from_slice(b"\r\n");
    }
    out
}

impl App {
    pub(crate) fn profile_tree(&mut self, profile: Profile, cwd: Option<String>) -> Option<Node> {
        let Some(layout) = profile.layout.as_ref() else { return Some(Node::Leaf(self.new_pane_in(profile, cwd))) };
        match layout.resolve(&self.profiles) {
            Ok(state) => Some(self.build(&state)),
            Err(id) => {
                self.tab_toast = Some((crate::i18n::tf("Layout profile unavailable: {id}", &[("id", &id)]), std::time::Instant::now() + crate::app::TOAST));
                self.dirty = true;
                None
            }
        }
    }

    fn capture_layout(&self, node: &Node, layout_id: &str, config: &mut crate::config::Config) -> Option<ProfileLayout> {
        Some(match node {
            Node::Leaf(id) => {
                let pane = self.panes.get(id)?;
                let source = &pane.source_profile;
                let profile = if self.profiles.iter().any(|p| p.id == source.id && p.layout.is_none()) {
                    source.id.clone()
                } else {
                    let base = format!("{layout_id}/pane-{id}");
                    let id = std::iter::once(base.clone()).chain((2..).map(|n| format!("{base}-{n}")))
                        .find(|id| self.profiles.iter().chain(&config.profiles).all(|p| &p.id != id))?;
                    config.profiles.push(Profile { id: id.clone(), hidden: true, builtin: false, elevated: false, hold: false, ..source.clone() });
                    id
                };
                ProfileLayout::Pane { profile, cwd: self.pane_cwd(*id).or_else(|| pane.profile.cwd.clone()) }
            }
            Node::Split { axis, ratio, a, b } => ProfileLayout::Split {
                vertical: *axis == Axis::Vertical, ratio: *ratio,
                a: Box::new(self.capture_layout(a, layout_id, config)?), b: Box::new(self.capture_layout(b, layout_id, config)?),
            },
        })
    }

    pub(crate) fn can_save_layout(&self) -> bool {
        self.tabs.get(self.active).is_some_and(|tab| matches!(tab.root, Node::Split { .. }))
    }

    pub(crate) fn save_as_profile(&mut self, layout: bool) {
        if self.config.read_only() { return self.warn_read_only_config(); }
        let Some(tab) = self.tabs.get(self.active) else { return };
        let Some(pane) = self.panes.get(&tab.focus) else { return };
        let name = crate::i18n::tf("{title} (saved)", &[("title", &self.tab_title(self.active))]);
        let id = self.custom_profile_id(&name);
        let mut config = self.config.clone();
        let profile = if layout {
            if !self.can_save_layout() { return; }
            let Some(tree) = self.capture_layout(&tab.root, &id, &mut config) else { return };
            Profile { layout: Some(tree), ..Profile::default() }
        } else {
            Profile { cwd: self.pane_cwd(tab.focus).or_else(|| pane.profile.cwd.clone()), ..pane.profile.clone() }
        };
        config.profiles.push(Profile { id: id.clone(), name, hidden: false, builtin: false, elevated: false, hold: false, ..profile });
        self.set_config(config);
        self.push_save_config();
        self.select_profile_settings(&id);
    }

    pub fn pane_host_id(&self, pane: PaneId) -> Option<u64> {
        self.panes.get(&pane)?.host_id
    }

    pub fn set_pane_host_id(&mut self, pane: PaneId, id: Option<u64>) {
        if let Some(pane) = self.panes.get_mut(&pane) { pane.host_id = id; }
    }

    pub fn pane_profile_name(&self, pane: PaneId) -> Option<&str> {
        self.panes.get(&pane).map(|pane| pane.profile.name.as_str())
    }

    pub fn finish_pane_restore(&mut self, id: PaneId, reattached: bool) {
        let Some(pane) = self.panes.get_mut(&id) else { return };
        if let Some(buffer) = pane.restore_buffer.take().filter(|_| !reattached) {
            let mut terminal = pane.terminal.lock();
            terminal.feed(&restore_text(&buffer));
            pane.seen = terminal.output_seq();
            pane.activity_seq = pane.seen;
        }
    }

    fn node_state(&self, node: &Node, checkpoint: bool) -> Option<NodeState> {
        Some(match node {
            Node::Leaf(id) => {
                let pane = self.panes.get(id).filter(|p| !p.profile.hold)?;
                let (cwd, buffer) = if checkpoint {
                    let cwd = pane.terminal.lock().cwd().map(str::to_owned).or_else(|| pane.profile.cwd.clone());
                    (cwd, Vec::new())
                } else {
                    let cwd = self.pane_cwd(*id);
                    let buffer = pane.restore_buffer.clone().unwrap_or_else(|| pane.terminal.lock().tail_lines(BUFFER_LINES));
                    (cwd, buffer)
                };
                NodeState::Pane(Box::new(PaneState { profile: pane.source_profile.clone(), name: pane.name.clone(), cwd, buffer, host_id: pane.host_id }))
            }
            Node::Split { axis, ratio, a, b } => match (self.node_state(a, checkpoint), self.node_state(b, checkpoint)) {
                (Some(a), Some(b)) => NodeState::Split { vertical: *axis == Axis::Vertical, ratio: *ratio, a: Box::new(a), b: Box::new(b) },
                (Some(one), None) | (None, Some(one)) => one,
                (None, None) => return None,
            },
        })
    }

    pub(crate) fn tab_state(&self, index: usize) -> Option<TabState> {
        self.capture_tab(index, false)
    }

    fn capture_tab(&self, index: usize, checkpoint: bool) -> Option<TabState> {
        let t = self.tabs.get(index)?;
        let root = self.node_state(&t.root, checkpoint)?;
        let focus = t.root.panes().iter().position(|p| *p == t.focus).unwrap_or(0);
        Some(TabState { root, focus, title: t.title.clone(), color: t.color.clone(), pinned: t.pinned })
    }

    pub fn session_snapshot(&self) -> WindowState {
        self.closing.clone().unwrap_or_else(|| self.snapshot())
    }

    pub fn snapshot(&self) -> WindowState {
        self.capture_session(false)
    }

    pub fn session_checkpoint(&self) -> WindowState {
        self.capture_session(true)
    }

    fn capture_session(&self, checkpoint: bool) -> WindowState {
        let mut tabs = Vec::new();
        let mut active = 0;
        for index in 0..self.tabs.len() {
            if let Some(tab) = self.capture_tab(index, checkpoint) {
                if index <= self.active {
                    active = tabs.len();
                }
                tabs.push(tab);
            }
        }
        WindowState { active, tabs }
    }

    pub(crate) fn build(&mut self, state: &NodeState) -> Node {
        self.build_session_node(state, false)
    }

    fn build_session_node(&mut self, state: &NodeState, hosted: bool) -> Node {
        match state {
            NodeState::Pane(p) => {
                let profile = Profile { elevated: false, ..p.profile.clone() };
                let id: PaneId = self.new_pane_in(profile, p.cwd.clone());
                if let Some(pane) = self.panes.get_mut(&id) {
                    pane.host_id = p.host_id.filter(|_| hosted && pane.profile.is_local());
                    pane.name = p.name.clone().filter(|name| !name.trim().is_empty());
                    let mut t = pane.terminal.lock();
                    if !p.buffer.is_empty() && pane.host_id.is_some() {
                        pane.restore_buffer = Some(p.buffer.clone());
                    } else if !p.buffer.is_empty() {
                        t.feed(&restore_text(&p.buffer));
                    }
                    let seen = t.output_seq();
                    drop(t);
                    pane.seen = seen;
                    pane.activity_seq = seen;
                }
                Node::Leaf(id)
            }
            NodeState::Split { vertical, ratio, a, b } => {
                let a = self.build_session_node(a, hosted);
                let b = self.build_session_node(b, hosted);
                Node::Split { axis: if *vertical { Axis::Vertical } else { Axis::Horizontal }, ratio: ratio.clamp(0.05, 0.95), a: Box::new(a), b: Box::new(b) }
            }
        }
    }

    pub fn restore(&mut self, state: &WindowState) -> bool {
        for tab in &state.tabs {
            let root = self.build_session_node(&tab.root, self.config.terminal.persistent_sessions);
            let order = root.panes();
            self.background_launches.extend(order.iter().copied().filter(|id| self.panes[id].host_id.is_some()));
            let focus = order.get(tab.focus).or(order.first()).copied().unwrap_or(0);
            self.tabs.push(Tab { title: tab.title.clone(), color: tab.color.clone(), pinned: tab.pinned, ..Tab::new(root, focus) });
        }
        self.active = state.active.min(self.tabs.len().saturating_sub(1));
        if let Some(focus) = self.focused_pane() {
            self.tabs.sort_by_key(|t| !t.pinned);
            self.active = self.tabs.iter().position(|t| t.focus == focus).unwrap_or(0);
        }
        self.dirty = true;
        !self.tabs.is_empty()
    }
}

#[cfg(test)]
#[path = "../test/session.rs"]
mod tests;
