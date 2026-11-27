use crate::app::{App, Pane, Tab};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub workspaces: Vec<WorkspaceState>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub tabs: Vec<TabState>,
    pub active: usize,
}

impl WindowState {
    pub fn has_layout(&self) -> bool {
        !self.tabs.is_empty() || self.workspace.is_some() || !self.workspaces.is_empty()
    }

    pub fn all_tabs(&self) -> impl Iterator<Item = &TabState> {
        self.tabs.iter().chain(self.workspaces.iter().flat_map(|workspace| &workspace.tabs))
    }
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
    let mut output = Vec::new();
    for line in lines {
        output.extend_from_slice(line.as_bytes());
        output.extend_from_slice(b"\r\n");
    }
    output
}

pub(crate) fn restore_history_text(lines: &[String]) -> Vec<u8> {
    let mut output = restore_text(lines);
    output.extend_from_slice(b"\x1b[2J\x1b[H");
    output
}

#[derive(Clone, Copy)]
enum RestoreMode {
    ClosedTab,
    Startup { hosted: bool },
}

impl Pane {
    pub(crate) fn restore_history(&mut self) {
        let Some(lines) = self.restore_buffer.take() else { return };
        self.restore_output(&restore_history_text(&lines));
    }

    fn restore_output(&mut self, output: &[u8]) {
        let mut terminal = self.terminal.lock();
        terminal.feed_saved_output(output);
        self.seen = terminal.output_seq();
        self.activity_seq = self.seen;
    }
}

impl App {
    pub(crate) fn profile_tree(&mut self, profile: Profile, cwd: Option<String>) -> Option<Node> {
        let Some(layout) = profile.layout.as_ref() else { return Some(Node::Leaf(self.new_pane_in(profile, cwd))) };
        match layout.resolve(&self.profiles) {
            Ok(state) => Some(self.build(&state)),
            Err(id) => {
                self.toast(crate::i18n::tf("Layout profile unavailable: {id}", &[("id", &id)]), crate::app::TOAST);
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
        if reattached {
            pane.restore_buffer = None;
        } else {
            pane.restore_history();
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
                    let buffer = pane.restore_buffer.as_deref().map_or_else(|| pane.terminal.lock().saved_tail(BUFFER_LINES), <[String]>::to_vec);
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
        self.capture_tab(self.tabs.get(index)?, false)
    }

    fn capture_tab(&self, t: &Tab, checkpoint: bool) -> Option<TabState> {
        let root = self.node_state(&t.root, checkpoint)?;
        let focus = t.root.panes().into_iter().filter(|id| self.panes.get(id).is_some_and(|pane| !pane.profile.hold))
            .position(|id| id == t.focus).unwrap_or(0);
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
        let workspace = self.capture_workspace(self.workspace.as_deref(), &self.tabs, self.active, checkpoint);
        let workspaces = self.workspaces.iter().map(|workspace| {
            self.capture_workspace(workspace.name.as_deref(), &workspace.tabs, workspace.active, checkpoint)
        }).collect();
        WindowState { tabs: workspace.tabs, active: workspace.active, workspace: workspace.name, workspaces }
    }

    fn capture_workspace(&self, name: Option<&str>, tabs: &[Tab], active: usize, checkpoint: bool) -> WorkspaceState {
        let mut state = WorkspaceState { name: name.map(str::to_owned), ..WorkspaceState::default() };
        for (index, tab) in tabs.iter().enumerate() {
            if let Some(tab) = self.capture_tab(tab, checkpoint) {
                if index <= active {
                    state.active = state.tabs.len();
                }
                state.tabs.push(tab);
            }
        }
        state
    }

    pub(crate) fn build(&mut self, state: &NodeState) -> Node {
        self.build_session_node(state, RestoreMode::ClosedTab)
    }

    fn build_session_node(&mut self, state: &NodeState, mode: RestoreMode) -> Node {
        match state {
            NodeState::Pane(p) => {
                let profile = Profile { elevated: false, ..p.profile.clone() };
                let id: PaneId = self.new_pane_in(profile, p.cwd.clone());
                if let Some(pane) = self.panes.get_mut(&id) {
                    pane.host_id = p.host_id.filter(|_| matches!(mode, RestoreMode::Startup { hosted: true }) && pane.profile.is_local());
                    pane.name = p.name.clone().filter(|name| !name.trim().is_empty());
                    if !p.buffer.is_empty() {
                        match mode {
                            RestoreMode::ClosedTab => pane.restore_output(&restore_text(&p.buffer)),
                            RestoreMode::Startup { .. } => pane.restore_buffer = Some(p.buffer.clone().into()),
                        }
                    }
                }
                Node::Leaf(id)
            }
            NodeState::Split { vertical, ratio, a, b } => {
                let a = self.build_session_node(a, mode);
                let b = self.build_session_node(b, mode);
                Node::Split { axis: if *vertical { Axis::Vertical } else { Axis::Horizontal }, ratio: ratio.clamp(0.05, 0.95), a: Box::new(a), b: Box::new(b) }
            }
        }
    }

    pub fn restore(&mut self, state: &WindowState) -> bool {
        let existing = std::mem::take(&mut self.tabs);
        let (tabs, active) = self.restore_tabs(existing, &state.tabs, state.active);
        self.tabs = tabs;
        self.active = active;
        self.workspace.clone_from(&state.workspace);
        for workspace in &state.workspaces {
            let (tabs, active) = self.restore_tabs(Vec::new(), &workspace.tabs, workspace.active);
            self.workspaces.push(crate::workspaces::Workspace { name: workspace.name.clone(), tabs, active, ..Default::default() });
        }
        self.dirty = true;
        !self.tabs.is_empty() || state.has_layout()
    }

    fn restore_tabs(&mut self, mut tabs: Vec<Tab>, saved: &[TabState], active: usize) -> (Vec<Tab>, usize) {
        tabs.reserve(saved.len());
        for tab in saved {
            let root = self.build_session_node(&tab.root, RestoreMode::Startup { hosted: self.config.terminal.persistent_sessions });
            let order = root.panes();
            self.background_launches.extend(order.iter().copied().filter(|id| self.panes[id].host_id.is_some()));
            let focus = order.get(tab.focus).or(order.first()).copied().unwrap_or(0);
            tabs.push(Tab { title: tab.title.clone(), color: tab.color.clone(), pinned: tab.pinned, ..Tab::new(root, focus) });
        }
        let active = tabs.get(active.min(tabs.len().saturating_sub(1))).map(|tab| tab.key);
        tabs.sort_by_key(|t| !t.pinned);
        let active = tabs.iter().position(|t| Some(t.key) == active).unwrap_or(0);
        (tabs, active)
    }
}

#[cfg(test)]
#[path = "../test/session.rs"]
mod tests;
