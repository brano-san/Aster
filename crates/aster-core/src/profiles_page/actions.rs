use super::editor::ProfileEditor;
use super::layout::{choice_step, editor_layout, row_action, EditorHit};
use super::{delete_group, kind_label, GroupEditor, Header, TreeKind, KINDS};
use crate::app::{App, Effect, SETTINGS_PANE};
use crate::config::Config;
use crate::i18n::{t, tf};
use crate::input::{Key, Mods};
use crate::overlay::{OverlayKind, Pick};
use crate::profile::{effective, Profile};
use crate::settings::{ProfileRow, SettingsPage};
use aster_gfx::render::Rect;

fn templates() -> [(&'static str, &'static str); 5] {
    [("local", t("Local shell")), ("ssh", t("SSH connection")), ("serial", t("Serial port")), ("telnet", t("Telnet")), ("tcp", t("TCP"))]
}

pub(crate) fn profile_templates() -> Vec<Pick> {
    templates().into_iter().map(|(id, name)| Pick { value: id.into(), label: name.into(), ..Default::default() }).collect()
}

pub(crate) fn profile_creation() -> Vec<Pick> {
    [("profile", t("New profile")), ("group", t("New group"))].into_iter()
        .map(|(value, label)| Pick { label: label.into(), value: value.into(), ..Default::default() }).collect()
}

pub(crate) fn profile_icons() -> Vec<Pick> {
    [
        ("", t("Automatic")), ("$", t("Terminal")), ("↗", t("SSH connection")), ("#", t("Serial port")),
        ("⌁", t("Network")), ("▣", t("Folder")), ("★", t("Star")), ("◆", t("Diamond")), ("●", t("Circle")),
    ].into_iter().map(|(icon, label)| Pick {
        label: label.into(), value: icon.into(), badge: icon.into(), ..Default::default()
    }).collect()
}

impl SettingsPage {
    fn select_tree(&mut self, kind: &TreeKind) {
        if let Some(at) = self.shown.iter().position(|&i| self.tree_at(i).is_some_and(|r| &r.kind == kind)) {
            self.selected = at;
        }
    }

    fn select_profile_id(&mut self, id: &str) {
        if let Some(at) = self.shown.iter().position(|&i| self.profile_at(i).is_some_and(|r| r.id == id)) {
            self.selected = at;
        }
    }
}

impl App {
    pub(crate) fn profile_rows(&self) -> Vec<ProfileRow> {
        let default = self.default_profile().map(|p| p.id);
        let mut rows: Vec<_> = self.profiles.iter().map(|p| {
            let (badge, tint) = crate::picker::badge(&effective(p, &self.config));
            let detail = match (&p.ssh, &p.serial, &p.socket) {
                _ if p.layout.is_some() => t("Layout").to_owned(),
                (Some(dest), _, _) => format!("ssh {dest}"),
                (_, Some(spec), _) => format!("serial {spec}"),
                (_, _, Some(address)) => address.clone(),
                _ => std::iter::once(p.command.clone()).chain(p.args.iter().enumerate().map(|(i, a)| {
                    if i > 0 && p.args[i - 1].eq_ignore_ascii_case("-EncodedCommand") { "…".to_owned() } else { a.clone() }
                })).collect::<Vec<_>>().join(" "),
            };
            ProfileRow {
                id: p.id.clone(), name: p.name.clone(), group: p.group.clone(), badge, tint, detail,
                custom: self.config.profiles.iter().any(|c| c.id == p.id),
                default: default.as_deref() == Some(&p.id), hidden: self.config.profile_hidden(p),
            }
        }).collect();
        rows.sort_by_key(|r| !r.custom);
        rows
    }

    pub(crate) fn sync_profile_page(&mut self) {
        if self.settings.is_none() && self.settings_stash.is_none() { return; }
        let rows = self.profile_rows();
        let profile_actions = self.profile_actions();
        let Some(page) = self.settings.as_mut().or(self.settings_stash.as_mut()) else { return };
        let item = page.current_item();
        let profile = item.and_then(|i| page.profile_at(i)).map(|r| r.id.clone());
        let kind = item.and_then(|i| page.tree_at(i)).map(|r| r.kind.clone());
        let (selected, scroll) = (page.selected, page.scroll);
        page.profiles = rows;
        page.profile_actions = profile_actions;
        page.groups.clone_from(&self.config.groups);
        page.refilter();
        page.selected = selected.min(page.shown.len().saturating_sub(1));
        page.scroll = scroll;
        if let Some(id) = profile { page.select_profile_id(&id); }
        else if let Some(kind) = kind { page.select_tree(&kind); }
    }

    fn detected_serial_ports(&self) -> Vec<String> {
        self.profiles.iter().filter(|p| p.builtin).filter_map(|p| p.serial.as_deref())
            .filter_map(|s| aster_proto::serial::parse(s).ok().map(|spec| spec.port)).collect()
    }

    fn open_profile_form(&mut self, profile: Profile, original: Option<String>, template: bool) {
        let ports = self.detected_serial_ports();
        let mut editor = ProfileEditor::profile(profile, original, &self.config, &self.scheme_names(), &ports);
        editor.set_jump_profiles(&self.profiles);
        editor.set_algorithm_catalog(self.ssh_algorithm_catalog.clone());
        self.ssh_editor_request = self.ssh_editor_request.wrapping_add(1);
        if template { editor.select_tab(1); }
        if let Some(page) = &mut self.settings {
            page.defaults_kind = None;
            page.editor = Some(editor);
        }
        self.dirty = true;
    }

    pub(crate) fn edit_profile_settings_row(&mut self, id: &str) {
        let Some(mut profile) = self.profiles.iter().find(|p| p.id == id).cloned() else { return };
        let original = self.config.profiles.iter().any(|p| p.id == id).then(|| id.to_owned());
        if original.is_none() {
            profile.name = tf("{name} (copy)", &[("name", &profile.name)]);
        }
        self.open_profile_form(profile, original, false);
    }

    pub(crate) fn edit_profile_template(&mut self, id: &str) {
        if !templates().iter().any(|(key, _)| *key == id) { return; }
        let mut profile = Profile::default();
        match id {
            "local" => {
                if let Some(shell) = self.profiles.iter().find(|p| p.builtin && p.is_local() && !p.elevated && !p.command.is_empty()) {
                    profile = shell.clone();
                }
            }
            "ssh" => profile.ssh = Some(String::new()),
            "serial" => profile.serial = Some(String::new()),
            "telnet" | "tcp" => profile.socket = Some(format!("{id}://")),
            _ => return,
        }
        profile.name.clear();
        self.open_profile_form(profile, None, true);
    }

    pub(crate) fn create_profile_item(&mut self, item: &str) {
        match item {
            "profile" => self.open_overlay(OverlayKind::ProfileTemplates),
            "group" => self.edit_group(None),
            _ => {}
        }
    }

    pub(crate) fn set_profile_icon(&mut self, icon: String) {
        if let Some(editor) = self.settings.as_mut().and_then(|page| page.editor.as_mut()) {
            editor.set_icon(icon);
        }
    }

    fn open_defaults(&mut self, kind: &'static str) {
        let schemes = self.scheme_names();
        let ports = self.detected_serial_ports();
        let Some(page) = self.settings.as_mut() else { return };
        let values = page.group_editor.as_ref().map_or(&self.config.profile_defaults, |g| &g.defaults);
        let mut editor = ProfileEditor::defaults(kind, &values.get(kind).cloned().unwrap_or_default(), &self.config, &schemes, &ports);
        editor.set_jump_profiles(&self.profiles);
        editor.set_algorithm_catalog(self.ssh_algorithm_catalog.clone());
        page.editor = Some(editor);
        page.defaults_kind = Some(kind);
        page.deleting = None;
    }

    fn edit_group(&mut self, original: Option<usize>) {
        if let Some(page) = &mut self.settings { page.group_editor = Some(GroupEditor::new(original, &page.groups)); }
    }

    fn activate_tree(&mut self, item: usize, action: bool) {
        let Some(page) = &mut self.settings else { return };
        let Some(row) = page.tree_at(item) else { return };
        match row.kind.clone() {
            TreeKind::Profile(index) => {
                let id = page.profiles[index].id.clone();
                if action {
                    let Some(p) = self.profiles.iter().find(|p| p.id == id).cloned() else { return };
                    let before = self.tabs.len();
                    self.open_tab(p);
                    if self.tabs.len() > before {
                        self.background_launches.extend(self.tabs[self.active].root.panes());
                        if let Some(i) = self.tabs.iter().position(|t| t.focus == SETTINGS_PANE) { self.active = i; }
                    }
                } else {
                    self.edit_profile_settings_row(&id);
                }
            }
            TreeKind::Header(h) => {
                if action {
                    if let Some(i) = h.group_index(&page.groups) { self.edit_group(Some(i)); }
                } else {
                    if !page.collapsed.remove(&h) { page.collapsed.insert(h.clone()); }
                    self.profile_collapsed.clone_from(&page.collapsed);
                    page.refilter();
                    page.select_tree(&TreeKind::Header(h));
                    self.reveal_selected();
                }
            }
            TreeKind::NewGroup => self.edit_group(None),
            TreeKind::NewProfile => self.open_overlay(OverlayKind::ProfileCreate),
            TreeKind::Defaults(kind) => self.open_defaults(kind),
            TreeKind::DefaultsHeader => {}
        }
    }

    fn persist_profiles(&mut self, config: Config) {
        if self.config.read_only() { return self.warn_read_only_config(); }
        self.set_config(config);
        self.push_save_config();
        self.reveal_selected();
    }

    fn confirm_profile_delete(&mut self, id: String, notice: String) -> bool {
        let Some(page) = &mut self.settings else { return false };
        if page.deleting.take().as_ref() == Some(&id) {
            page.notice = None;
            true
        } else {
            page.deleting = Some(id);
            page.notice = Some(notice);
            false
        }
    }

    pub(crate) fn profile_tree_click(&mut self, item: usize, row: Rect, x: f32, y: f32) {
        let action = row_action(row, self.ui()).contains(x, y) && self.settings.as_ref().and_then(|p| p.tree_at(item)).is_some_and(|r| {
            matches!(r.kind, TreeKind::Profile(_) | TreeKind::Header(Header::Group(_)))
        });
        self.activate_tree(item, action);
    }

    pub(crate) fn profile_tree_key(&mut self, key: Key, mods: Mods) -> bool {
        let Some(page) = &mut self.settings else { return false };
        let Some(item) = page.current_item() else { return false };
        let Some(kind) = page.tree_at(item).map(|r| r.kind.clone()) else { return false };
        if key != Key::Delete { page.deleting = None; }
        match (key, kind) {
            (Key::Enter, TreeKind::Profile(i)) if mods.ctrl => {
                let mut config = self.config.clone();
                config.terminal.profile.clone_from(&page.profiles[i].id);
                self.persist_profiles(config);
            }
            (Key::Char('H'), TreeKind::Profile(i)) if mods == Mods::NONE => {
                let row = &page.profiles[i];
                self.swallow_chord = Some(key);
                let mut config = self.config.clone();
                if let Some(p) = config.profiles.iter_mut().find(|p| p.id == row.id) { p.hidden = !row.hidden; }
                else if row.hidden { config.hidden_profiles.retain(|id| id != &row.id); }
                else { config.hidden_profiles.push(row.id.clone()); }
                self.persist_profiles(config);
            }
            (Key::Char('E'), TreeKind::Header(Header::Group(_))) if mods == Mods::NONE => {
                self.swallow_chord = Some(key);
                self.activate_tree(item, true);
            }
            (Key::F(5), TreeKind::Profile(_)) => self.activate_tree(item, true),
            (Key::Enter, _) => self.activate_tree(item, false),
            (Key::Delete, TreeKind::Profile(i)) => {
                let row = page.profiles[i].clone();
                if !row.custom {
                    page.notice = Some(t("Detected profiles cannot be deleted; copy one with Enter instead").into());
                } else if self.confirm_profile_delete(format!("profile/{}", row.id), tf("Press Del again to delete {name}", &[("name", &row.name)])) {
                    let mut config = self.config.clone();
                    config.profiles.retain(|p| p.id != row.id);
                    if config.terminal.profile == row.id || config.terminal.profile == row.name { config.terminal.profile.clear(); }
                    self.persist_profiles(config);
                }
            }
            (Key::Delete, TreeKind::Header(h)) => {
                if let Some(i) = h.group_index(&page.groups) {
                    let name = page.groups[i].name.clone();
                    if self.confirm_profile_delete(format!("group/{name}"), tf("Press Del again to delete {name}", &[("name", &name)])) {
                        let mut config = self.config.clone();
                        delete_group(&mut config, i);
                        self.persist_profiles(config);
                    }
                }
            }
            (Key::Delete, TreeKind::Defaults(kind)) => self.reset_defaults(kind),
            _ => return false,
        }
        true
    }


    fn reset_defaults(&mut self, kind: &'static str) {
        if !self.confirm_profile_delete(format!("defaults/{kind}"), tf("Press Del again to reset {type} defaults", &[("type", kind_label(kind))])) { return; }
        if let Some(group) = self.settings.as_mut().and_then(|p| p.group_editor.as_mut()) {
            group.defaults.remove(kind);
        } else {
            let mut config = self.config.clone();
            config.profile_defaults.remove(kind);
            self.persist_profiles(config);
        }
    }

    pub(crate) fn edit_environment(&mut self) {
        if self.settings.is_none() {
            self.open_settings();
        }
        if let Some(page) = &mut self.settings {
            page.editor = Some(ProfileEditor::global_environment(&self.config.terminal.environment));
            page.group_editor = None;
            page.defaults_kind = None;
            page.notice = None;
            self.dirty = true;
        }
    }

    fn close_profile_modal(&mut self) {
        let Some(page) = &mut self.settings else { return };
        if page.editor.take().is_none() { page.group_editor = None; }
        page.defaults_kind = None;
        page.notice = None;
        page.deleting = None;
        self.vault_input_id = self.vault_input_id.wrapping_add(1);
        self.ssh_editor_request = self.ssh_editor_request.wrapping_add(1);
    }

    fn save_profile_modal(&mut self) {
        if self.config.read_only() { return self.warn_read_only_config(); }
        let Some(page) = self.settings.as_mut() else { return };
        let mut config = self.config.clone();
        if let Some(editor) = &mut page.editor {
            if editor.is_global_environment() {
                match editor.environment() {
                    Ok(env) => config.terminal.environment = env,
                    Err(error) => { editor.error = Some(error); return; }
                }
                self.close_profile_modal();
                self.persist_profiles(config);
                return;
            }
            if let Some(kind) = page.defaults_kind {
                let defaults = match editor.defaults_result() {
                    Ok(defaults) => defaults,
                    Err(error) => { editor.error = Some(error); return; }
                };
                page.editor = None;
                page.defaults_kind = None;
                if let Some(group) = &mut page.group_editor { group.defaults.insert(kind.into(), defaults); return; }
                config.profile_defaults.insert(kind.into(), defaults);
            } else {
                let mut profile = match editor.profile_result() {
                    Ok(p) => p,
                    Err(error) => { editor.error = Some(error); return; }
                };
                profile.id = editor.original.clone().unwrap_or_else(|| self.custom_profile_id(&profile.name));
                let id = profile.id.clone();
                if let Some(p) = config.profiles.iter_mut().find(|p| p.id == id) { *p = profile; }
                else { config.profiles.push(profile); }
                self.close_profile_modal();
                self.persist_profiles(config);
                if let Some(page) = &mut self.settings {
                    if super::reveal_profile(&id, &page.groups, &page.profiles, &mut page.collapsed) {
                        self.profile_collapsed.clone_from(&page.collapsed);
                        page.refilter();
                    }
                    page.select_profile_id(&id);
                }
                self.reveal_selected();
                return;
            }
        } else if let Some(group) = &mut page.group_editor {
            match group.save(&mut config) {
                Ok(i) => {
                    let key = super::group_key(&config.groups[i]).to_owned();
                    self.close_profile_modal();
                    self.persist_profiles(config);
                    if let Some(page) = &mut self.settings { page.select_tree(&TreeKind::Header(Header::Group(key))); }
                    self.reveal_selected();
                    return;
                }
                Err(error) => { group.error = error; return; }
            }
        }
        self.persist_profiles(config);
    }

    pub(crate) fn profile_modal_key(&mut self, key: Key, mods: Mods) {
        if key == Key::Escape { return self.close_profile_modal(); }
        if mods.ctrl && matches!(key, Key::Enter | Key::Char('S')) { return self.save_profile_modal(); }
        if mods.ctrl && key == Key::Char('V') {
            if self.settings.as_ref().and_then(|p| p.editor.as_ref()).is_some_and(ProfileEditor::password_focused) {
                self.vault_input_id = self.vault_input_id.wrapping_add(1);
                return self.push(Effect::RequestVaultPaste(self.vault_input_id));
            }
            return self.push(Effect::RequestPaste);
        }
        self.vault_input_id = self.vault_input_id.wrapping_add(1);
        let Some(page) = &mut self.settings else { return };
        if key != Key::Delete { page.deleting = None; page.notice = None; }
        if let Some(editor) = &mut page.editor {
            if matches!(key, Key::Enter | Key::Space) {
                if let Some(action) = editor.ssh_action(key == Key::Enter) { return self.ssh_editor_action(action); }
                if editor.focuses_icon() { return self.open_overlay(OverlayKind::ProfileIcons); }
            }
            editor.key(key, mods);
            return;
        }
        let Some(group) = &mut page.group_editor else { return };
        match key {
            Key::Tab if mods.shift => group.focus = (group.focus + 7) % 8,
            Key::Tab => group.focus = (group.focus + 1) % 8,
            Key::Up => group.focus = group.focus.saturating_sub(1),
            Key::Down => group.focus = (group.focus + 1).min(7),
            Key::Enter if group.focus >= 4 => { let kind = KINDS[group.focus - 4]; self.open_defaults(kind); }
            Key::Delete if group.focus >= 4 => { let kind = KINDS[group.focus - 4]; self.reset_defaults(kind); }
            Key::Left | Key::Right | Key::Enter | Key::Space if group.focus == 1 => {
                group.cycle_parent(&page.groups, if key == Key::Left { -1 } else { 1 });
            }
            Key::Enter => group.focus = (group.focus + 1).min(7),
            Key::Backspace if matches!(group.focus, 0 | 2 | 3) => {
                if mods.ctrl { group.fields[group.focus].clear(); } else { group.fields[group.focus].pop(); }
                group.error.clear();
            }
            _ => {}
        }
    }

    pub(crate) fn profile_modal_click(&mut self, area: Rect, x: f32, y: f32) -> bool {
        let s = self.ui();
        let Some(page) = &mut self.settings else { return false };
        let layout = if let Some(e) = &page.editor {
            super::layout::profile_editor_layout(area, s, e)
        } else if let Some(g) = &page.group_editor {
            editor_layout(area, s, 0, 8, g.focus)
        } else { return false };
        page.notice = None;
        page.deleting = None;
        self.vault_input_id = self.vault_input_id.wrapping_add(1);
        match layout.hit(x, y) {
            Some(EditorHit::Cancel) => self.close_profile_modal(),
            Some(EditorHit::Save) => self.save_profile_modal(),
            Some(EditorHit::Tab(i)) => { if let Some(e) = &mut page.editor { e.select_tab(i); } }
            Some(EditorHit::Field(i)) => {
                let step = layout.fields.iter().find(|(index, _)| *index == i).map_or(0, |(_, r)| choice_step(*r, s, x));
                if let Some(e) = &mut page.editor {
                    e.focus = i;
                    if let Some(action) = e.ssh_action(false) { self.ssh_editor_action(action); }
                    else if e.focuses_icon() { self.open_overlay(OverlayKind::ProfileIcons); }
                    else if e.tabs[e.tab].fields[i].is_button() { e.activate(i); }
                    else if step != 0 { e.key(if step < 0 { Key::Left } else { Key::Right }, Mods::NONE); }
                    else { e.activate(i); }
                } else if let Some(g) = &mut page.group_editor {
                    g.focus = i;
                    if i == 1 { g.cycle_parent(&page.groups, if step < 0 { -1 } else { 1 }); }
                    if i >= 4 { self.open_defaults(KINDS[i - 4]); }
                }
            }
            None => {}
        }
        true
    }

    pub(crate) fn profile_modal_wheel(&mut self, dy: f32) -> bool {
        let Some(page) = &mut self.settings else { return false };
        let (focus, count) = if let Some(e) = &mut page.editor {
            (&mut e.focus, e.tabs[e.tab].fields.len())
        } else if let Some(g) = &mut page.group_editor { (&mut g.focus, 8) } else { return false };
        *focus = (*focus as isize - dy.signum() as isize).clamp(0, count.saturating_sub(1) as isize) as usize;
        self.vault_input_id = self.vault_input_id.wrapping_add(1);
        self.dirty = true;
        true
    }
}
