use crate::app::{App, Effect};
use crate::i18n::t;
use crate::overlay::{Pick, CLEAR_RECENT, MANAGE};
use crate::profile::Profile;

pub const RECENT: &str = "recent:";

pub fn badge(p: &Profile) -> (String, u32) {
    let (text, color) = if p.ssh.is_some() {
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
        match p.icon.as_deref() {
            Some("powershell") => ("PS", 0x2671BE),
            Some("cmd") => ("C:", 0x4D4D4D),
            Some("linux") => ("WSL", 0xE95420),
            Some("git") => ("Git", 0xF05032),
            Some("cygwin") => ("Cy", 0x4B8B3B),
            Some("msys2") => ("M2", 0x7B4FA0),
            Some("vs") => ("VS", 0x68217A),
            _ => ("$", 0x3C7A3C),
        }
    };
    let tint = p.color.as_deref().and_then(aster_vt::schemes::parse_hex).map(|c| u32::from(c.r) << 16 | u32::from(c.g) << 8 | u32::from(c.b)).unwrap_or(color);
    (text.to_owned(), tint)
}

pub fn detail(p: &Profile) -> String {
    p.ssh.clone().or_else(|| p.serial.clone()).or_else(|| p.socket.clone()).unwrap_or_else(|| p.command.clone())
}

fn group_of(p: &Profile) -> (u32, String) {
    if !p.builtin {
        return match p.group.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
            Some(group) => (3, group.to_owned()),
            None => (2, t("My profiles").to_owned()),
        };
    }
    if p.id.starts_with("ssh:") {
        (5, t("From ~/.ssh/config").to_owned())
    } else if p.id.starts_with("serial:") {
        (6, t("Serial ports").to_owned())
    } else {
        (4, t("Built-in").to_owned())
    }
}

fn pick(p: &Profile, group: String, rank: u32, value: String) -> Pick {
    let (badge, tint) = badge(p);
    Pick { label: p.name.clone(), hint: detail(p), value, group, rank, badge, tint: Some(tint) }
}

impl App {
    pub(crate) fn profile_picks(&self) -> Vec<Pick> {
        let mut picks = Vec::new();
        let recent: Vec<&Profile> = self.recent.iter().filter_map(|id| self.profiles.iter().find(|p| &p.id == id)).take(self.config.terminal.recent_profiles).collect();
        for p in &recent {
            picks.push(pick(p, t("Recent").to_owned(), 1, format!("{RECENT}{}", p.id)));
        }
        if !recent.is_empty() {
            picks.push(Pick { label: t("Clear recent profiles").to_owned(), value: CLEAR_RECENT.into(), group: t("Recent").to_owned(), rank: 1, badge: "\u{00D7}".into(), ..Pick::default() });
        }
        let mut rest: Vec<(u32, String, Pick)> = self
            .profiles
            .iter()
            .map(|p| {
                let (rank, group) = group_of(p);
                let pick = pick(p, group.clone(), rank, p.id.clone());
                (rank, group, pick)
            })
            .collect();
        rest.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| if a.0 == 3 { a.1.to_lowercase().cmp(&b.1.to_lowercase()) } else { std::cmp::Ordering::Equal }));
        picks.extend(rest.into_iter().map(|(_, _, p)| p));
        picks.push(Pick { label: t("Manage profiles").to_owned(), value: MANAGE.into(), rank: 9, badge: "\u{2699}".into(), ..Pick::default() });
        picks
    }

    pub(crate) fn note_recent(&mut self, id: &str) {
        self.recent.retain(|r| r != id);
        self.recent.insert(0, id.to_owned());
        self.recent.truncate(self.config.terminal.recent_profiles.max(1) * 3);
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
