use crate::app::{App, Tab};
use crate::panes::{Axis, Node, PaneId};
use crate::profile::Profile;
use serde::{Deserialize, Serialize};

pub const BUFFER_LINES: usize = 1000;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionState {
    pub windows: Vec<WindowState>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub tabs: Vec<TabState>,
    pub active: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TabState {
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
pub struct PaneState {
    pub profile: Profile,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub buffer: Vec<String>,
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
    fn node_state(&self, node: &Node) -> Option<NodeState> {
        Some(match node {
            Node::Leaf(id) => {
                let pane = self.panes.get(id).filter(|p| !p.profile.hold)?;
                let cwd = self.pane_cwd(*id);
                let t = pane.terminal.lock();
                NodeState::Pane(Box::new(PaneState { profile: pane.profile.clone(), cwd, buffer: t.tail_lines(BUFFER_LINES) }))
            }
            Node::Split { axis, ratio, a, b } => match (self.node_state(a), self.node_state(b)) {
                (Some(a), Some(b)) => NodeState::Split { vertical: *axis == Axis::Vertical, ratio: *ratio, a: Box::new(a), b: Box::new(b) },
                (Some(one), None) | (None, Some(one)) => one,
                (None, None) => return None,
            },
        })
    }

    pub(crate) fn tab_state(&self, index: usize) -> Option<TabState> {
        let t = self.tabs.get(index)?;
        let root = self.node_state(&t.root)?;
        let focus = t.root.panes().iter().position(|p| *p == t.focus).unwrap_or(0);
        Some(TabState { root, focus, title: t.title.clone(), color: t.color.clone() })
    }

    pub fn session_snapshot(&self) -> WindowState {
        self.closing.clone().unwrap_or_else(|| self.snapshot())
    }

    pub fn snapshot(&self) -> WindowState {
        let tabs = (0..self.tabs.len()).filter_map(|i| self.tab_state(i)).collect::<Vec<_>>();
        WindowState { active: self.active.min(tabs.len().saturating_sub(1)), tabs }
    }

    pub(crate) fn build(&mut self, state: &NodeState) -> Node {
        match state {
            NodeState::Pane(p) => {
                let profile = Profile { elevated: false, ..p.profile.clone() };
                let id: PaneId = self.new_pane_in(profile, p.cwd.clone());
                if let Some(pane) = self.panes.get_mut(&id) {
                    let mut t = pane.terminal.lock();
                    if !p.buffer.is_empty() {
                        t.feed(&restore_text(&p.buffer));
                    }
                    t.set_cwd(p.cwd.clone());
                    let seen = t.output_seq();
                    drop(t);
                    pane.seen = seen;
                }
                Node::Leaf(id)
            }
            NodeState::Split { vertical, ratio, a, b } => {
                let a = self.build(a);
                let b = self.build(b);
                Node::Split { axis: if *vertical { Axis::Vertical } else { Axis::Horizontal }, ratio: ratio.clamp(0.05, 0.95), a: Box::new(a), b: Box::new(b) }
            }
        }
    }

    pub fn restore(&mut self, state: &WindowState) -> bool {
        for tab in &state.tabs {
            let root = self.build(&tab.root);
            let order = root.panes();
            let focus = order.get(tab.focus).or(order.first()).copied().unwrap_or(0);
            self.tabs.push(Tab { root, focus, zoomed: None, title: tab.title.clone(), color: tab.color.clone() });
        }
        self.active = state.active.min(self.tabs.len().saturating_sub(1));
        self.dirty = true;
        !self.tabs.is_empty()
    }
}

#[cfg(test)]
#[path = "../test/session.rs"]
mod tests;
