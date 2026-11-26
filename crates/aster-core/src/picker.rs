use crate::app::{App, Effect};
use crate::config::Config;
use crate::i18n::t;
use crate::keymap::Action;
use crate::overlay::{Pick, CLEAR_RECENT, MANAGE};
use crate::profile::Profile;

pub const RECENT: &str = "recent:";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProfileSection {
    Own,
    BuiltIn,
    SshConfig,
    SshFile,
    SerialPorts,
}

impl ProfileSection {
    const ALL: [Self; 5] = [Self::Own, Self::BuiltIn, Self::SshConfig, Self::SshFile, Self::SerialPorts];

    pub fn id(self) -> &'static str {
        match self {
            Self::Own => "own",
            Self::BuiltIn => "built-in",
            Self::SshConfig => "ssh-config",
            Self::SshFile => "ssh-file",
            Self::SerialPorts => "serial-ports",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|section| section.id() == id)
    }

    pub(crate) fn source_label(self) -> &'static str {
        match self {
            Self::Own => "My profiles",
            Self::BuiltIn => "Built-in",
            Self::SshConfig => "From ~/.ssh/config",
            Self::SshFile => "From ssh-profiles.yaml",
            Self::SerialPorts => "Serial ports",
        }
    }

    fn of(p: &Profile) -> Self {
        if !p.builtin {
            Self::Own
        } else if p.id.starts_with("ssh:") {
            Self::SshConfig
        } else if p.id.starts_with("file-config:") {
            Self::SshFile
        } else if p.id.starts_with("serial:") {
            Self::SerialPorts
        } else {
            Self::BuiltIn
        }
    }

    fn rank(self) -> u32 {
        match self {
            Self::Own => 2,
            Self::BuiltIn => 4,
            Self::SshConfig => 5,
            Self::SshFile => 5,
            Self::SerialPorts => 6,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ProfileFilter<'a> {
    All,
    TabMenu,
    SinglePane,
    Section(ProfileSection),
    Group(&'a str),
}

impl ProfileFilter<'_> {
    fn includes(self, p: &Profile, config: &Config) -> bool {
        match self {
            Self::All | Self::TabMenu => true,
            Self::SinglePane => p.layout.is_none(),
            Self::Section(section) => ProfileSection::of(p) == section,
            Self::Group(id) => p.group.as_deref().is_some_and(|name| {
                crate::profile::group_chain(name.trim(), config).iter().any(|group| group.id == id)
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ProfileAction {
    pub action: Action,
    pub name: Option<String>,
}

pub fn badge(p: &Profile) -> (String, u32) {
    let fallback = if p.ssh.is_some() {
        ("SSH", 0x2F8F83)
    } else if p.serial.is_some() {
        ("COM", 0x8A6D3B)
    } else if let Some(socket) = &p.socket {
        if socket.starts_with("tcp:") {
            ("TCP", 0x5A6B7D)
        } else {
            ("TN", 0x5A6B7D)
        }
    } else {
        ("$", 0x3C7A3C)
    };
    let (text, color) = match p.icon.as_deref() {
        Some("powershell") => ("PS", 0x2671BE),
        Some("cmd") => ("C:", 0x4D4D4D),
        Some("linux") => ("WSL", 0xE95420),
        Some("git") => ("Git", 0xF05032),
        Some("cygwin") => ("Cy", 0x4B8B3B),
        Some("msys2") => ("M2", 0x7B4FA0),
        Some("vs") => ("VS", 0x68217A),
        Some("ssh") => ("SSH", 0x2F8F83),
        Some(icon) if !icon.is_empty() => (icon, fallback.1),
        _ if p.layout.is_some() => ("▦", 0x5A6B7D),
        _ => fallback,
    };
    let tint = p.color.as_deref().and_then(aster_vt::schemes::parse_hex).map(|c| u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)).unwrap_or(color);
    (text.to_owned(), tint)
}

pub fn detail(p: &Profile) -> String {
    if p.layout.is_some() {
        return t("Layout").to_owned();
    }
    p.ssh.clone().or_else(|| p.serial.clone()).or_else(|| p.socket.clone()).unwrap_or_else(|| p.command.clone())
}

fn group_of(p: &Profile, config: &crate::config::Config) -> (u32, String) {
    if let Some(group) = p.group.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
        return (3, crate::profile::group_path(group, config));
    }
    let section = ProfileSection::of(p);
    (section.rank(), t(section.source_label()).to_owned())
}

fn pick(p: &Profile, group: String, rank: u32, value: String) -> Pick {
    let (badge, tint) = badge(p);
    Pick { label: p.name.clone(), hint: detail(p), value, group, rank, badge, tint: Some(tint), ..Pick::default() }
}

impl App {
    pub(crate) fn profile_actions(&self) -> Vec<ProfileAction> {
        let sections = ProfileSection::ALL.into_iter().map(|section| ProfileAction { action: Action::SelectProfileSection(section), name: None });
        let groups = self.config.groups.iter().filter(|g| !g.id.is_empty()).map(|g| ProfileAction {
            action: Action::SelectProfileGroup(g.id.clone()), name: Some(crate::profile::group_path(&g.name, &self.config)),
        });
        let profiles = self.profiles.iter().filter(|p| !p.id.is_empty() && !self.config.profile_hidden(p)).map(|p| ProfileAction {
            action: Action::OpenProfile(p.id.clone()), name: Some(p.name.clone()),
        });
        sections.chain(groups).chain(profiles).collect()
    }

    pub(crate) fn open_profile_by_id(&mut self, id: &str) {
        if let Some(p) = self.profiles.iter().find(|p| p.id == id).cloned() {
            self.note_recent(id);
            self.open_tab(p);
        }
    }

    pub(crate) fn profile_picks(&self) -> Vec<Pick> {
        self.filtered_profile_picks(ProfileFilter::All)
    }

    pub(crate) fn filtered_profile_picks(&self, filter: ProfileFilter<'_>) -> Vec<Pick> {
        let visible = |p: &Profile| self.config.profile_in_picker(p) && filter.includes(p, &self.config);
        let mut picks = Vec::new();
        let recent: Vec<&Profile> = self.recent.iter().filter_map(|id| self.profiles.iter().find(|p| &p.id == id && visible(p))).take(self.config.terminal.recent_profiles).collect();
        for p in &recent {
            picks.push(pick(&crate::profile::effective(p, &self.config), t("Recent").to_owned(), 1, format!("{RECENT}{}", p.id)));
        }
        if !recent.is_empty() && matches!(filter, ProfileFilter::All) {
            picks.push(Pick { label: t("Clear recent profiles").to_owned(), value: CLEAR_RECENT.into(), group: t("Recent").to_owned(), rank: 1, badge: "\u{00D7}".into(), ..Pick::default() });
        }
        let mut rest: Vec<(u32, String, Pick)> = self
            .profiles
            .iter()
            .filter(|p| visible(p))
            .map(|p| {
                let (rank, group) = self.extension_profile_group(&p.id).map_or_else(|| group_of(p, &self.config), |group| (7, group.to_owned()));
                let pick = pick(&crate::profile::effective(p, &self.config), group.clone(), rank, p.id.clone());
                (rank, group, pick)
            })
            .collect();
        rest.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| if a.0 == 3 { a.1.to_lowercase().cmp(&b.1.to_lowercase()) } else { std::cmp::Ordering::Equal }));
        picks.extend(rest.into_iter().map(|(_, _, p)| p));
        if matches!(filter, ProfileFilter::All) {
            picks.push(Pick { label: t("Manage profiles").to_owned(), value: MANAGE.into(), rank: 9, badge: "\u{2699}".into(), ..Pick::default() });
        }
        picks
    }

    pub(crate) fn profile_edit_address(&self, value: &str, provider: Option<crate::quick_connect::Provider>) -> Option<String> {
        let id = value.strip_prefix(RECENT).unwrap_or(value);
        let profile = crate::profile::effective(self.profiles.iter().find(|p| p.id == id)?, &self.config);
        if let Some(ssh) = profile.ssh {
            return Some(if provider == Some(crate::quick_connect::Provider::Ssh) { ssh } else { format!("ssh://{ssh}") });
        }
        if let Some(spec) = profile.serial {
            return Some(format!("serial:{spec}"));
        }
        profile.socket.map(|address| aster_proto::address::parse(&address).map_or(address, |parsed| parsed.display()))
    }

    pub(crate) fn note_recent(&mut self, id: &str) {
        self.recent.retain(|r| r != id);
        self.recent.insert(0, id.to_owned());
        self.recent.truncate(self.config.terminal.recent_profiles.max(1).saturating_mul(3));
        self.effects.push(Effect::RecentProfiles(self.recent.clone()));
    }

    pub fn set_recent(&mut self, recent: Vec<String>) {
        if self.tabs.is_empty() && self.recent != recent {
            self.start_selected = 0;
            self.dirty = true;
        }
        self.recent = recent;
    }

    pub(crate) fn clear_recent(&mut self) {
        self.recent.clear();
        self.effects.push(Effect::RecentProfiles(Vec::new()));
    }
}

#[cfg(test)]
#[path = "../test/picker.rs"]
mod tests;
