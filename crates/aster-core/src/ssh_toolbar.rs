use crate::app::{App, Effect};
use crate::i18n::{ssh_error, t, tf};
use crate::input::{CursorIcon, MouseButton};
use crate::overlay::{Overlay, OverlayKind, Pick};
use crate::panes::PaneId;
use aster_gfx::render::Rect;
use aster_proto::ssh_forward::{Command, Forward, ForwardStatus};

const ADD_FORWARD: &str = "add";

#[derive(Clone, Copy)]
pub(crate) struct Toolbar {
    pub buttons: [Rect; 3],
}

impl App {
    pub(crate) fn ssh_toolbar_visible(&self, pane: PaneId, rect: Rect) -> bool {
        let s = self.ui();
        rect.w >= 125.0 * s && rect.h >= 70.0 * s && self.panes.get(&pane).is_some_and(|pane| pane.profile.ssh.is_some())
    }

    pub(crate) fn ssh_toolbar(&self, pane: PaneId, rect: Rect) -> Option<Toolbar> {
        if !self.ssh_toolbar_visible(pane, rect) { return None; }
        let s = self.ui();
        let bar = self.pane_header(pane, rect);
        let gap = 5.0 * s;
        let title_width = (bar.w * 0.22).min(72.0 * s);
        let scale = ((bar.w - title_width - 4.0 * gap) / (216.0 * s)).min(1.0);
        let widths = [94.0, 58.0, 64.0].map(|w| w * s * scale);
        let height = (bar.h - 6.0 * s).max(1.0);
        let mut x = bar.right() - widths.iter().sum::<f32>() - 3.0 * gap;
        let buttons = widths.map(|width| {
            let button = Rect::new(x, bar.y + 3.0 * s, width, height).snap();
            x += width + gap;
            button
        });
        Some(Toolbar { buttons })
    }

    pub(crate) fn ssh_toolbar_buttons(&self, pane: PaneId) -> [bool; 3] {
        let reconnect = self.panes.get(&pane).is_some_and(|pane| pane.retry_launch.is_some());
        let native = self.config.ssh.client == crate::config::SshClient::Native;
        let files = self.can_browse(pane) && self.ssh_forwards.contains_key(&pane) && !self.ssh_auth.contains_key(&pane);
        [reconnect, files, native]
    }

    pub(crate) fn ssh_pointer_at(&self, pane: PaneId, rect: Rect, x: f32, y: f32) -> Option<CursorIcon> {
        if self.pane_header(pane, rect).contains(x, y) {
            let toolbar = self.ssh_toolbar(pane, rect)?;
            let index = toolbar.buttons.iter().position(|button| button.contains(x, y))?;
            return Some(if self.ssh_toolbar_buttons(pane)[index] { CursorIcon::Hand } else { CursorIcon::Default });
        }
        let panel = self.ssh_auth.get(&pane)?;
        let geometry = panel.geometry(self.pane_terminal_rect(pane, rect), self.ui());
        let icon = geometry.entries.iter().find(|(_, rect)| rect.contains(x, y)).filter(|_| !panel.pending).map_or(CursorIcon::Default, |(index, _)| {
            match panel.rows[*index] {
                crate::ssh_auth::Row::Link(_) => CursorIcon::Hand,
                crate::ssh_auth::Row::Prompt(_) => CursorIcon::Text,
                crate::ssh_auth::Row::Text(_) => CursorIcon::Default,
            }
        });
        Some(icon)
    }

    pub(crate) fn ssh_toolbar_click(&mut self, pane: PaneId, rect: Rect, x: f32, y: f32, button: MouseButton) -> bool {
        if button != MouseButton::Left { return false; }
        let Some(toolbar) = self.ssh_toolbar(pane, rect) else { return false };
        let Some(index) = toolbar.buttons.iter().position(|rect| rect.contains(x, y)) else { return false };
        self.focus_pane(pane);
        if self.ssh_toolbar_buttons(pane)[index] {
            match index {
                0 => self.reconnect_ssh(pane),
                1 => self.open_files(),
                _ => self.open_overlay(OverlayKind::SshPorts(pane)),
            }
        }
        true
    }

    pub(crate) fn set_ssh_forwards(&mut self, pane: PaneId, statuses: Vec<ForwardStatus>) {
        self.ssh_forwards.insert(pane, statuses);
        if let Some(p) = self.panes.get_mut(&pane) {
            p.ssh_connecting = false;
        }
        if let Some(previous) = self.overlay.as_ref().filter(|overlay| overlay.kind == OverlayKind::SshPorts(pane)) {
            let mut overlay = Overlay::new(previous.kind, self.ssh_forward_picks(pane));
            overlay.opened_at = previous.opened_at;
            overlay.set_query(&previous.query);
            if let Some(value) = previous.selected_value() {
                if let Some(at) = overlay.shown.iter().position(|(item, _)| overlay.items[*item].value == value) {
                    overlay.select(at);
                }
            }
            self.overlay = Some(overlay);
        }
        self.dirty = true;
    }

    pub(crate) fn choose_ssh_forward(&mut self, pane: PaneId, value: &str) {
        if value == ADD_FORWARD {
            return self.open_forward_form(pane);
        }
        if let Ok(index) = value.parse() {
            if self.ssh_forwards.get(&pane).is_some_and(|rows| rows.iter().any(|status| status.index == index)) {
                self.push(Effect::SshForward { pane, command: Command::StopForward(index) });
            }
        }
    }

    pub(crate) fn ssh_forward_picks(&self, pane: PaneId) -> Vec<Pick> {
        let Some(statuses) = self.ssh_forwards.get(&pane) else { return Vec::new() };
        let rules = self.panes.get(&pane).and_then(|p| p.profile.ssh_options.as_ref()).and_then(|options| options.forwards.as_deref()).unwrap_or_default();
        let add = Pick { label: t("Add forwarding…").into(), value: ADD_FORWARD.into(), ..Default::default() };
        std::iter::once(add).chain(statuses.iter().map(|status| {
            let (kind, bind, port, destination) = match &status.forward {
                Forward::Local { bind, port, host, host_port } => (t("Local forwarding"), bind, *port, Some(endpoint(host, u32::from(*host_port)))),
                Forward::Remote { bind, port, host, host_port } => (t("Remote forwarding"), bind, *port, Some(endpoint(host, u32::from(*host_port)))),
                Forward::Dynamic { bind, port } => (t("Dynamic forwarding"), bind, *port, None),
            };
            let source = endpoint(bind, status.bound.as_ref().copied().unwrap_or(u32::from(port)));
            let label = destination.map_or_else(|| source.clone(), |to| format!("{source} → {to}"));
            let description = rules.iter().find(|rule| rule.resolve().is_ok_and(|forward| forward == status.forward)).map(|rule| rule.description.as_str()).filter(|text| !text.is_empty());
            let mut hint = description.map_or_else(|| kind.to_owned(), |text| format!("{kind} · {text}"));
            if let Err(reason) = &status.bound {
                hint.push_str(" · ");
                hint.push_str(&tf("Unavailable ({reason})", &[("reason", &ssh_error(reason))]));
            }
            if let Some(reason) = &status.stop_error {
                hint.push_str(" · ");
                hint.push_str(&tf("Could not stop forwarding ({reason})", &[("reason", &ssh_error(reason))]));
            }
            let badge = if status.bound.is_ok() { t("Stop") } else { t("Dismiss") };
            Pick { label, hint, value: status.index.to_string(), badge: badge.into(), ..Default::default() }
        })).collect()
    }
}

pub(crate) fn endpoint(host: &str, port: u32) -> String {
    if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") }
}

pub(crate) fn forward_action_rect(row: Rect, scale: f32) -> Rect {
    let w = (78.0 * scale).min(row.w * 0.36);
    Rect::new(row.right() - w - 5.0 * scale, row.y + 3.0 * scale, w, (row.h - 6.0 * scale).max(0.0)).snap()
}
