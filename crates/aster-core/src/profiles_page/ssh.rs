use super::{enum_text, optional, EditorField, EditorTab, Field, Profile, ProfileEditor};
use crate::i18n::t;
use aster_proto::ssh::{proxy_endpoint, SshProxy};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::profiles_page) enum SshEditorAction {
    SetPassword,
    ForgetPassword,
    BrowseKey,
    ImportKey,
    VaultKey,
}

pub(super) fn tab(profile: &Profile, defaults: bool) -> EditorTab {
    let s = profile.ssh_options.clone().unwrap_or_default();
    let (method, argument) = match &s.proxy {
        None => ("", ""),
        Some(SshProxy::Direct) => ("direct", ""),
        Some(SshProxy::Command { command }) => ("command", command.as_str()),
        Some(SshProxy::Jump { profile }) => ("jump", profile.as_str()),
        Some(SshProxy::Socks { address }) => ("socks", address.as_str()),
        Some(SshProxy::Http { address }) => ("http", address.as_str()),
    };
    let mut fields = vec![
        Field::Destination.text(optional(profile.ssh.as_ref())), Field::Port.text(optional(s.port)), Field::User.text(optional(s.user)),
        Field::Auth.choice(enum_text(s.auth), [
            ("", t("Inherit")), ("auto", t("Automatic")), ("password", t("Password")),
            ("key", t("Private key")), ("agent", t("SSH agent")), ("interactive", t("Keyboard-interactive")),
        ].into_iter().map(|(key, label)| (key.into(), label.into())).collect(), false),
        Field::ProxyMethod.choice(method, [
            ("", t("Use SSH configuration")), ("direct", t("Direct connection")), ("command", t("Proxy command")),
            ("jump", t("Jump profile")), ("socks", t("SOCKS5")), ("http", t("HTTP CONNECT")),
        ].into_iter().map(|(key, label)| (key.into(), label.into())).collect(), false),
    ];
    fields.extend(proxy_field(method, argument.into()));
    if !defaults {
        fields.extend([Field::Password.text(""), button(Field::SetPassword), button(Field::ForgetPassword)]);
    }
    fields.extend(s.identity_files.unwrap_or_default().into_iter().map(|f| Field::Identity.text(f)));
    fields.extend([Field::Identity.text(""), button(Field::BrowseKey), button(Field::ImportKey), button(Field::VaultKey)]);
    EditorTab::new("SSH", fields)
}

pub(super) fn advanced_tab(profile: &Profile) -> EditorTab {
    let defaults = aster_proto::ssh::SshOptions::default();
    let s = profile.ssh_options.as_ref().unwrap_or(&defaults);
    EditorTab::new("Advanced", vec![
        Field::X11.toggle(s.x11, true), Field::ForwardAgent.toggle(s.forward_agent, true),
        Field::Banner.toggle(s.skip_banner, true), Field::ReuseSession.toggle(s.reuse_session, true),
        Field::Remember.toggle(s.remember_cwd, true), Field::Term.text(optional(s.term.as_ref())),
        Field::Keepalive.text(optional(s.keepalive_ms)), Field::Count.text(optional(s.keepalive_count)),
        Field::Timeout.text(optional(s.ready_timeout_ms)),
    ])
}

fn button(id: Field) -> EditorField {
    id.choice("", Vec::new(), false)
}

fn proxy_field(method: &str, argument: String) -> Option<EditorField> {
    let mut field = Field::ProxyTarget.text(argument);
    field.label = match method {
        "command" => t("Proxy command line"),
        "jump" => t("Jump profile"),
        "socks" => t("SOCKS5 proxy address"),
        "http" => t("HTTP CONNECT proxy address"),
        _ => return None,
    }.into();
    field.editable = method != "jump";
    Some(field)
}

impl EditorField {
    pub(crate) fn masked(&self) -> Option<String> {
        (self.id == Field::Password).then(|| "•".repeat(self.value.chars().count()))
    }

    pub(crate) fn is_button(&self) -> bool {
        matches!(self.id, Field::SetPassword | Field::ForgetPassword | Field::BrowseKey | Field::ImportKey | Field::VaultKey | Field::AddForward)
    }
}

impl ProfileEditor {
    pub(in crate::profiles_page) fn set_jump_profiles(&mut self, profiles: &[Profile]) {
        if self.kind != "ssh" { return; }
        self.jump_choices = profiles.iter().filter(|p| p.ssh.is_some() && p.id != self.source.id)
            .map(|p| (p.id.clone(), p.name.clone())).collect();
        self.jump_choices.sort_by(|a, b| a.1.cmp(&b.1));
        self.jump_choices.insert(0, (String::new(), t("Choose a jump profile").into()));
        for tab in &mut self.tabs {
            if let Some(field) = tab.fields.iter_mut().find(|f| f.id == Field::ProxyTarget && !f.editable) {
                field.choices = self.jump_choices.clone();
                if !field.choices.iter().any(|(id, _)| *id == field.value) {
                    field.choices.push((field.value.clone(), field.value.clone()));
                }
            }
        }
    }

    pub(super) fn cycle_field(&mut self, backwards: bool) {
        let field = self.focused_field();
        if field.id == Field::AlgorithmCheck { return self.toggle_algorithm(); }
        if let Some(value) = field.stepped_choice(backwards) { self.apply_field_choice(value); }
    }

    pub(super) fn apply_field_choice(&mut self, value: String) {
        let fields = &mut self.tabs[self.tab].fields;
        let field = &mut fields[self.focus];
        if value == field.value { return; }
        let previous = std::mem::replace(&mut field.value, value);
        if field.id == Field::AlgorithmCategory {
            self.populate_algorithm_fields();
            return;
        }
        if field.id == Field::ForwardType {
            let dynamic = field.value == "dynamic";
            fields[self.focus + 2].editable = !dynamic;
            return;
        }
        if field.id != Field::ProxyMethod { return; }
        let next = field.value.clone();
        let index = self.focus + 1;
        if fields.get(index).is_some_and(|f| f.id == Field::ProxyTarget) {
            self.proxy_arguments.insert(previous, fields.remove(index).value);
        }
        if let Some(mut field) = proxy_field(&next, self.proxy_arguments.get(&next).cloned().unwrap_or_default()) {
            if !field.editable {
                field.choices = self.jump_choices.clone();
                if !field.choices.iter().any(|(id, _)| *id == field.value) {
                    field.choices.push((field.value.clone(), field.value.clone()));
                }
            }
            fields.insert(index, field);
        }
    }

    pub(super) fn ssh_user(&self) -> Option<String> {
        if let Some(user) = self.opt(Field::User) { return Some(user); }
        if self.source.ssh_options.as_ref().is_some_and(|options| options.user.is_some()) { return Some(String::new()); }
        let destination = self.value(Field::Destination);
        if self.defaults.is_some() || crate::ssh::parse_target(destination).is_some_and(|target| target.user.is_some()) {
            return None;
        }
        let profile = Profile { group: self.opt(Field::Group), ssh: Some(destination.into()), ..Profile::default() };
        let config = crate::config::Config { groups: self.groups.clone(), profile_defaults: self.global.clone(), ..Default::default() };
        let inherited = crate::profile::effective(&profile, &config).ssh_options.is_some_and(|options| options.user.is_some());
        (!inherited).then(String::new)
    }

    pub(super) fn ssh_proxy(&self) -> Result<Option<SshProxy>, String> {
        let argument = self.value(Field::ProxyTarget);
        let proxy = match self.value(Field::ProxyMethod) {
            "direct" => SshProxy::Direct,
            "command" if !argument.is_empty() => SshProxy::Command { command: argument.into() },
            "command" => return Err(t("Enter a proxy command line").into()),
            "jump" if !argument.is_empty() => SshProxy::Jump { profile: argument.into() },
            "jump" => return Err(t("Choose a jump profile").into()),
            method @ ("socks" | "http") => {
                if proxy_endpoint(argument).is_none() { return Err(t("Enter the proxy address as host:port").into()); }
                let address = argument.into();
                if method == "socks" { SshProxy::Socks { address } } else { SshProxy::Http { address } }
            }
            _ => return Ok(None),
        };
        Ok(Some(proxy))
    }

    pub(crate) fn password_focused(&self) -> bool {
        self.tabs[self.tab].fields[self.focus].id == Field::Password
    }

    pub(in crate::profiles_page) fn ssh_action(&self, password_input: bool) -> Option<SshEditorAction> {
        match self.tabs[self.tab].fields[self.focus].id {
            Field::Password if password_input => Some(SshEditorAction::SetPassword),
            Field::SetPassword => Some(SshEditorAction::SetPassword),
            Field::ForgetPassword => Some(SshEditorAction::ForgetPassword),
            Field::BrowseKey => Some(SshEditorAction::BrowseKey),
            Field::ImportKey => Some(SshEditorAction::ImportKey),
            Field::VaultKey => Some(SshEditorAction::VaultKey),
            _ => None,
        }
    }

    pub(in crate::profiles_page) fn take_ssh_password(&mut self) -> Option<String> {
        let field = self.tabs.iter_mut().flat_map(|tab| &mut tab.fields).find(|f| f.id == Field::Password)?;
        (!field.value.is_empty()).then(|| std::mem::take(&mut field.value))
    }

    pub(in crate::profiles_page) fn add_ssh_identity(&mut self, path: String) {
        let Some((tab, fields)) = self.tabs.iter_mut().enumerate().find_map(|(tab, item)| {
            item.fields.iter().any(|field| field.id == Field::BrowseKey).then_some((tab, &mut item.fields))
        }) else { return };
        if fields.iter().any(|f| f.id == Field::Identity && f.value == path) { return; }
        if let Some(field) = fields.iter_mut().find(|f| f.id == Field::Identity && f.value.is_empty()) {
            field.value = path;
        } else if let Some(index) = fields.iter().position(|f| f.id == Field::BrowseKey) {
            fields.insert(index, Field::Identity.text(path));
            if self.tab == tab && self.focus >= index { self.focus += 1; }
        }
    }
}
