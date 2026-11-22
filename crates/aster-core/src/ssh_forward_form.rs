use crate::app::{App, Effect};
use crate::i18n::t;
use crate::input::{CursorIcon, Event, Key, Mods, MouseButton};
use crate::overlay::OverlayKind;
use crate::panes::PaneId;
use crate::text_editor::TextEditor;
use aster_gfx::render::Rect;
use aster_proto::ssh_forward::{Command, Forward, ForwardKind, ForwardRule};

pub(crate) const KINDS: [ForwardKind; 3] = [ForwardKind::Local, ForwardKind::Remote, ForwardKind::Dynamic];
const KIND: usize = 4;
pub(crate) const SAVE: usize = 5;
pub(crate) const CANCEL: usize = 6;
pub(crate) const ADD: usize = 7;
const ORDER: [usize; 8] = [KIND, 0, 1, 2, 3, SAVE, CANCEL, ADD];

pub(crate) struct ForwardForm {
    pub pane: PaneId,
    pub kind: ForwardKind,
    pub fields: [TextEditor; 4],
    pub focus: usize,
    pub save: bool,
    pub error: Option<&'static str>,
}

pub(crate) struct FormGeometry {
    pub scale: f32,
    pub frame: Rect,
    pub title: Rect,
    pub kinds: [Rect; 3],
    pub fields: [Rect; 4],
    pub save: Rect,
    pub status: Rect,
    pub cancel: Rect,
    pub add: Rect,
}

impl ForwardForm {
    fn new(pane: PaneId) -> Self {
        let mut bind = TextEditor::new("127.0.0.1");
        bind.place_cursor(0, usize::MAX, false);
        Self { pane, kind: ForwardKind::Local, fields: [bind, TextEditor::default(), TextEditor::default(), TextEditor::default()],
            focus: 1, save: false, error: None }
    }

    pub fn field_count(&self) -> usize {
        if self.kind == ForwardKind::Dynamic { 2 } else { 4 }
    }

    pub fn kind_focused(&self) -> bool { self.focus == KIND }

    fn step(&mut self, backwards: bool, can_save: bool) {
        let at = ORDER.iter().position(|focus| *focus == self.focus).unwrap_or(0);
        let next = (1..=ORDER.len()).map(|i| ORDER[if backwards {
            (at + ORDER.len() - i) % ORDER.len()
        } else {
            (at + i) % ORDER.len()
        }]).find(|&i| (i < self.field_count() || i >= KIND) && (i != SAVE || can_save));
        if let Some(next) = next { self.focus = next; }
    }

    fn cycle_kind(&mut self, backwards: bool) {
        let current = KINDS.iter().position(|kind| *kind == self.kind).unwrap_or(0);
        self.kind = KINDS[(current + if backwards { 2 } else { 1 }) % KINDS.len()];
        self.error = None;
    }

    pub fn geometry(&self, area: Rect, scale: f32) -> FormGeometry {
        let height = if self.field_count() == 2 { 239.0 } else { 301.0 };
        let s = scale.min(area.w / 442.0).min(area.h / (height + 16.0)).max(0.2);
        let w = 420.0 * s;
        let h = height * s;
        let frame = Rect::new(area.x + (area.w - w) * 0.5, area.y + (area.h - h) * 0.5, w, h).snap();
        let x = frame.x + 16.0 * s;
        let y = frame.y + 12.0 * s;
        let width = frame.w - 32.0 * s;
        let title = Rect::new(x, y, width, 30.0 * s).snap();
        let kind_w = (width - 12.0 * s) / 3.0;
        let kinds = std::array::from_fn(|i| Rect::new(x + i as f32 * (kind_w + 6.0 * s), y + 38.0 * s, kind_w, 31.0 * s).snap());
        let port_w = 108.0 * s;
        let host_w = width - port_w - 10.0 * s;
        let fields = std::array::from_fn(|i| {
            let top = y + (81.0 + (i / 2) as f32 * 62.0) * s;
            let (x, w) = if i % 2 == 0 { (x, host_w) } else { (x + host_w + 10.0 * s, port_w) };
            Rect::new(x, top, w, 52.0 * s).snap()
        });
        let save = Rect::new(x, frame.bottom() - 89.0 * s, width, 24.0 * s).snap();
        let status = Rect::new(x, save.bottom(), width, 21.0 * s).snap();
        let add = Rect::new(x + width - 86.0 * s, frame.bottom() - 42.0 * s, 86.0 * s, 30.0 * s).snap();
        let cancel = Rect::new(add.x - 96.0 * s, add.y, 86.0 * s, add.h).snap();
        FormGeometry { scale: s, frame, title, kinds, fields, save, status, cancel, add }
    }

    fn rule(&self) -> Result<(ForwardRule, Forward), &'static str> {
        let field = |i: usize| self.fields[i].text().trim();
        let bind = address(field(0), true).ok_or_else(|| t("Enter a valid bind address"))?;
        let port = valid_port(field(1), true).ok_or_else(|| t("Enter a listening port from 0 to 65535"))?;
        let destination = if self.kind == ForwardKind::Dynamic {
            None
        } else {
            let host = address(field(2), false).ok_or_else(|| t("Enter a valid destination host"))?;
            let port = valid_port(field(3), false).ok_or_else(|| t("Enter a destination port from 1 to 65535"))?;
            Some(crate::ssh_toolbar::endpoint(host, u32::from(port)))
        };
        let rule = ForwardRule { kind: self.kind, listen: crate::ssh_toolbar::endpoint(bind, u32::from(port)), destination, description: String::new() };
        let forward = rule.resolve().map_err(|_| t("Check the forwarding addresses"))?;
        Ok((rule, forward))
    }
}

fn address(input: &str, allow_wildcard: bool) -> Option<&str> {
    if allow_wildcard && input == "*" { return Some(input); }
    let bracketed = input.strip_prefix('[').and_then(|text| text.strip_suffix(']'));
    let text = bracketed.unwrap_or(input);
    if bracketed.is_some() || text.contains(':') {
        let (ip, scope) = text.split_once('%').map_or((text, None), |(ip, scope)| (ip, Some(scope)));
        ip.parse::<std::net::Ipv6Addr>().ok()?;
        if scope.is_some_and(|scope| scope.is_empty() || !scope.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))) { return None; }
    } else if !text.chars().any(char::is_alphanumeric) || !text.chars().all(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_')) {
        return None;
    }
    Some(text)
}

fn valid_port(text: &str, allow_zero: bool) -> Option<u16> {
    if !text.bytes().all(|c| c.is_ascii_digit()) { return None; }
    text.parse::<u16>().ok().filter(|port| allow_zero || *port != 0)
}

impl App {
    pub(crate) fn open_forward_form(&mut self, pane: PaneId) {
        if self.ssh_forwards.contains_key(&pane) {
            self.overlay = None;
            self.ssh_forward_form = Some(ForwardForm::new(pane));
            self.dirty = true;
        }
    }

    pub(crate) fn can_save_forward(&self, pane: PaneId) -> bool {
        !self.config.read_only() && self.panes.get(&pane).is_some_and(|pane|
            self.config.profiles.iter().any(|profile| profile.id == pane.source_profile.id && profile.ssh.is_some()))
    }

    fn save_forward(&mut self, pane: PaneId, rule: ForwardRule, forward: &Forward) {
        let Some(pane) = self.panes.get(&pane) else { return };
        let id = pane.source_profile.id.clone();
        let mut config = self.config.clone();
        let Some(at) = config.profiles.iter().position(|profile| profile.id == id && profile.ssh.is_some()) else { return };
        let effective = crate::profile::effective(&config.profiles[at], &config);
        let inherited = effective.ssh_options.unwrap_or_default();
        let profile = &mut config.profiles[at];
        let options = profile.ssh_options.get_or_insert_default();
        if options.forwards.is_none() {
            options.inherit_forwards = inherited.forwards.is_none() || inherited.inherit_forwards;
            options.forwards = inherited.forwards;
        }
        let rules = options.forwards.get_or_insert_default();
        if rules.iter().any(|rule| rule.resolve().as_ref() == Ok(forward)) { return; }
        rules.push(rule);
        let options = options.clone();
        for pane in self.panes.values_mut().filter(|pane| pane.source_profile.id == id) {
            let copy_rules = |target: &mut aster_proto::ssh::SshOptions| {
                target.forwards.clone_from(&options.forwards);
                target.inherit_forwards = options.inherit_forwards;
            };
            copy_rules(pane.source_profile.ssh_options.get_or_insert_default());
            copy_rules(pane.profile.ssh_options.get_or_insert_default());
            if let Some(launch) = pane.retry_launch.as_mut() {
                copy_rules(launch.ssh_options.get_or_insert_default());
            }
        }
        self.set_config(config);
        self.push_save_config();
    }

    fn close_forward_form(&mut self) {
        if let Some(form) = self.ssh_forward_form.take() {
            if self.ssh_forwards.contains_key(&form.pane) {
                self.open_overlay(OverlayKind::SshPorts(form.pane));
            }
            self.dirty = true;
        }
    }

    fn submit_forward(&mut self) {
        let Some(form) = &mut self.ssh_forward_form else { return };
        let (rule, forward) = match form.rule() {
            Ok(valid) => valid,
            Err(error) => { form.error = Some(error); self.dirty = true; return; }
        };
        let pane = form.pane;
        if form.save && self.can_save_forward(pane) {
            self.save_forward(pane, rule, &forward);
        }
        if self.ssh_forwards.contains_key(&pane) {
            self.push(Effect::SshForward { pane, command: Command::AddForward(forward) });
        }
        self.close_forward_form();
    }

    pub(crate) fn ssh_forward_form_text(&mut self, text: &str) -> bool {
        let Some(form) = &mut self.ssh_forward_form else { return false };
        if let Some(field) = form.fields.get_mut(form.focus) {
            let text = text.replace(['\r', '\n', '\t'], " ");
            field.insert(&text);
            form.error = None;
            self.dirty = true;
        }
        true
    }

    fn ssh_forward_form_key(&mut self, key: Key, mods: Mods, can_save: bool) {
        let Some(form) = &mut self.ssh_forward_form else { return };
        self.dirty = true;
        match key {
            Key::Escape => self.close_forward_form(),
            Key::Enter if form.focus == CANCEL => self.close_forward_form(),
            Key::Enter | Key::Space if form.focus == ADD => self.submit_forward(),
            Key::Space if form.focus == SAVE && can_save => form.save = !form.save,
            Key::Space if form.focus == CANCEL => self.close_forward_form(),
            Key::Left | Key::Right | Key::Space if form.focus == KIND => form.cycle_kind(key == Key::Left),
            Key::Enter => self.submit_forward(),
            Key::Tab => form.step(mods.shift, can_save),
            Key::Char('V') if mods.ctrl && form.focus < form.field_count() => self.push(Effect::RequestPaste),
            Key::Char('C') if mods.ctrl && form.focus < form.field_count() => {
                let text = form.fields[form.focus].selected_text().to_owned();
                if !text.is_empty() { self.push(Effect::SetClipboard(text)); }
            }
            _ => {
                if let Some(field) = form.fields.get_mut(form.focus) {
                    if field.key(key, mods, 1, true) { form.error = None; }
                }
            }
        }
    }

    pub(crate) fn ssh_forward_form_event(&mut self, event: &Event) -> bool {
        let Some(form) = &self.ssh_forward_form else { return false };
        let can_save = self.can_save_forward(form.pane);
        match event {
            Event::Key { key, mods, pressed: true } => self.ssh_forward_form_key(*key, *mods, can_save),
            Event::Char(c) | Event::AltChar(c) if !c.is_control() => { self.ssh_forward_form_text(&c.to_string()); }
            Event::MouseDown { x, y, button: MouseButton::Left, .. } => {
                let geo = form.geometry(self.chrome.content, self.ui());
                let form = self.ssh_forward_form.as_mut().unwrap();
                self.dirty = true;
                if geo.cancel.contains(*x, *y) { self.close_forward_form(); }
                else if geo.add.contains(*x, *y) { self.submit_forward(); }
                else if can_save && geo.save.contains(*x, *y) {
                    form.focus = SAVE;
                    form.save = !form.save;
                } else if let Some(i) = geo.kinds.iter().position(|r| r.contains(*x, *y)) {
                    form.kind = KINDS[i];
                    form.focus = KIND;
                    form.error = None;
                } else if let Some(i) = geo.fields[..form.field_count()].iter().position(|r| r.contains(*x, *y)) {
                    form.focus = i;
                }
            }
            Event::MouseMove { x, y } => {
                let geo = form.geometry(self.chrome.content, self.ui());
                let on_field = geo.fields[..form.field_count()].iter().any(|r| r.contains(*x, *y));
                let on_action = [geo.add, geo.cancel].iter().chain(&geo.kinds).any(|r| r.contains(*x, *y)) || can_save && geo.save.contains(*x, *y);
                self.mouse = (*x, *y);
                self.set_cursor(if on_field { CursorIcon::Text } else if on_action { CursorIcon::Hand } else { CursorIcon::Default });
                self.dirty = true;
            }
            Event::Key { .. } | Event::Char(_) | Event::AltChar(_) | Event::MouseDown { .. } | Event::MouseUp { .. }
                | Event::Wheel { .. } | Event::DroppedFiles(_) => {}
            _ => return false,
        }
        true
    }
}

#[cfg(test)]
#[path = "../test/ssh_forward_form.rs"]
mod tests;
