use crate::app::{App, Effect, TOAST, Tab};
use crate::i18n::t;
use crate::overlay::{OverlayKind, Pick};
use crate::panes::PaneId;

pub(crate) const MAX_NAME_CHARS: usize = 80;

#[derive(Default)]
pub(crate) struct Workspace {
    pub(crate) name: Option<String>,
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active: usize,
    pub(crate) closed_tabs: Vec<crate::session::TabState>,
    pub(crate) mru: crate::tabs::MruTabs,
    pub(crate) last_tab: Option<PaneId>,
}

impl Tab {
    pub(crate) fn remove_leaf(&mut self, pane: PaneId) {
        let order = self.root.panes();
        let at = order.iter().position(|&id| id == pane).unwrap_or(0);
        self.root.remove(pane);
        if self.focus == pane {
            let rest = self.root.panes();
            self.focus = rest[at.saturating_sub(1).min(rest.len() - 1)];
        }
        if self.zoomed == Some(pane) {
            self.zoomed = None;
        }
    }
}

fn workspace_label(name: Option<&str>) -> &str {
    name.unwrap_or_else(|| t("Default workspace"))
}

impl App {
    pub(crate) fn workspace_window_title(&self, title: String) -> String {
        if self.workspace.is_none() && self.workspaces.is_empty() {
            title
        } else {
            let name = workspace_label(self.workspace.as_deref());
            if title.is_empty() { name.into() } else { format!("{name} · {title}") }
        }
    }

    pub(crate) fn all_tabs(&self) -> impl Iterator<Item = &Tab> {
        self.tabs.iter().chain(self.workspaces.iter().flat_map(|workspace| &workspace.tabs))
    }

    pub(crate) fn all_tabs_mut(&mut self) -> impl Iterator<Item = &mut Tab> {
        self.tabs.iter_mut().chain(self.workspaces.iter_mut().flat_map(|workspace| &mut workspace.tabs))
    }

    pub(crate) fn close_window_if_no_tabs(&mut self) -> bool {
        if self.config.terminal.close_window_with_last_tab && self.all_tabs().next().is_none() {
            self.push(Effect::Close);
            true
        } else {
            false
        }
    }

    pub(crate) fn close_hidden_pane(&mut self, pane: PaneId) {
        let location = self.workspaces.iter().enumerate().find_map(|(workspace, state)| {
            state.tabs.iter().position(|tab| tab.root.contains(pane)).map(|tab| (workspace, tab))
        });
        let Some((workspace, index)) = location else { return };
        self.release_pane(pane);
        let state = &mut self.workspaces[workspace];
        if state.tabs[index].root == crate::panes::Node::Leaf(pane) {
            let tab = state.tabs.remove(index);
            state.mru.remove(tab.key);
            if state.active > index || state.active >= state.tabs.len() {
                state.active = state.active.saturating_sub(1);
            }
            self.close_window_if_no_tabs();
        } else {
            state.tabs[index].remove_leaf(pane);
        }
        self.dirty = true;
    }

    pub(crate) fn workspace_picks(&self) -> Vec<Pick> {
        let mut others: Vec<_> = self.workspaces.iter().enumerate().map(|(index, workspace)| Pick {
            label: workspace_label(workspace.name.as_deref()).into(),
            value: index.to_string(),
            ..Pick::default()
        }).collect();
        others.sort_by_cached_key(|pick| pick.label.to_lowercase());
        let current = Pick {
            label: workspace_label(self.workspace.as_deref()).into(),
            hint: t("Current workspace").into(),
            ..Pick::default()
        };
        std::iter::once(current).chain(others).collect()
    }

    pub(crate) fn open_workspace_name(&mut self, rename: bool) {
        self.open_overlay(OverlayKind::WorkspaceName { rename });
    }

    pub(crate) fn submit_workspace_name(&mut self, rename: bool, value: &str) {
        let name = value.trim();
        if name.is_empty() || name.chars().any(char::is_control) || name.chars().count() > MAX_NAME_CHARS {
            return;
        }
        let folded = name.to_lowercase();
        let in_use = |other: Option<&str>| folded == workspace_label(other).to_lowercase();
        if self.workspaces.iter().any(|workspace| in_use(workspace.name.as_deref())) || (!rename && in_use(self.workspace.as_deref())) {
            self.toast(t("Workspace name is already in use").into(), TOAST);
            return;
        }
        self.overlay = None;
        if rename {
            self.workspace = Some(name.into());
            self.dirty = true;
        } else {
            let previous = self.replace_workspace(Workspace { name: Some(name.into()), ..Workspace::default() });
            self.workspaces.push(previous);
        }
    }

    pub(crate) fn choose_workspace(&mut self, value: &str) {
        if let Ok(index) = value.parse::<usize>() {
            self.switch_workspace(index);
        }
    }

    fn switch_workspace(&mut self, index: usize) {
        if index < self.workspaces.len() {
            let target = std::mem::take(&mut self.workspaces[index]);
            self.workspaces[index] = self.replace_workspace(target);
        }
    }

    pub(crate) fn activate_workspace_for_pane(&mut self, pane: PaneId) -> bool {
        if self.tabs.iter().any(|tab| tab.root.contains(pane)) {
            return true;
        }
        if let Some(index) = self.workspaces.iter().position(|workspace| workspace.tabs.iter().any(|tab| tab.root.contains(pane))) {
            self.switch_workspace(index);
            return true;
        }
        false
    }

    pub(crate) fn hidden_workspace_search_panes(&self) -> Vec<(PaneId, String)> {
        let mut panes = Vec::new();
        for workspace in &self.workspaces {
            let name = workspace_label(workspace.name.as_deref());
            for tab in &workspace.tabs {
                let title = self.title_for_tab(tab);
                for pane in tab.root.panes() {
                    let Some(state) = self.panes.get(&pane) else { continue };
                    let label = state.name.as_deref().unwrap_or(&state.profile.name);
                    let source = if title == label { format!("{name} · {title}") } else { format!("{name} · {title} · {label}") };
                    panes.push((pane, source));
                }
            }
        }
        panes
    }

    pub(crate) fn show_tab_by_key(&mut self, key: u64) {
        if let Some(index) = self.workspaces.iter().position(|workspace| workspace.tabs.iter().any(|tab| tab.key == key)) {
            self.switch_workspace(index);
        }
        if let Some(index) = self.tabs.iter().position(|tab| tab.key == key) {
            self.activate(index);
        }
    }
}
