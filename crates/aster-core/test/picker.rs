use super::*;
use crate::config::Config;
use crate::overlay::OverlayKind;
use std::sync::Arc;

fn profiles() -> Vec<Profile> {
    let mut pwsh = Profile::new("local:pwsh", "PowerShell", "pwsh.exe", &[]);
    pwsh.icon = Some("powershell".into());
    let mut host = Profile::new("ssh:db", "SSH: db", "ssh", &["db"]);
    host.ssh = Some("db".into());
    let mut port = Profile::new("serial:COM3", "Serial: COM3", "", &[]);
    port.serial = Some("COM3:115200".into());
    vec![pwsh, host, port]
}

fn app(custom: Vec<Profile>) -> App {
    let config = Config { profiles: custom, ..Config::default() };
    let mut a = App::new(config, profiles(), true, None, Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    a
}

fn groups(picks: &[Pick]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in picks {
        if out.last() != Some(&p.group) {
            out.push(p.group.clone());
        }
    }
    out
}

#[test]
fn profiles_are_grouped_with_badges_details_and_a_manage_entry() {
    let mut work = Profile { id: "custom:prod".into(), name: "Prod".into(), command: "ssh".into(), ssh: Some("prod".into()), group: Some("Work".into()), ..Profile::default() };
    work.color = Some("#112233".into());
    let mine = Profile { id: "custom:mine".into(), name: "Mine".into(), command: "bash".into(), ..Profile::default() };
    let a = app(vec![work, mine]);
    let picks = a.profile_picks();
    assert_eq!(groups(&picks), ["My profiles", "Work", "Built-in", "From ~/.ssh/config", "Serial ports", ""]);
    let prod = picks.iter().find(|p| p.label == "Prod").unwrap();
    assert_eq!((prod.badge.as_str(), prod.hint.as_str(), prod.tint), ("SSH", "prod", Some(0x112233)));
    let pwsh = picks.iter().find(|p| p.label == "PowerShell").unwrap();
    assert_eq!((pwsh.badge.as_str(), pwsh.hint.as_str()), ("PS", "pwsh.exe"));
    assert_eq!(picks.last().map(|p| p.value.as_str()), Some(MANAGE));
}

#[test]
fn hidden_profiles_leave_both_picker_sections_but_remain_addressable() {
    let config = Config::parse("hidden-profiles = ['local:pwsh']\n[[profiles]]\nid = 'work'\nname = 'Work'\nhidden = true").unwrap();
    let mut a = App::new(config, profiles(), true, None, Arc::new(|| {}));
    a.set_recent(vec!["work".into(), "local:pwsh".into()]);
    let picks = a.profile_picks();
    assert!(!picks.iter().any(|p| p.group == "Recent" || matches!(p.value.as_str(), "work" | "local:pwsh")));
    assert_eq!(a.find_profile("Work").unwrap().id, "work");
    assert!(a.find_profile("local:pwsh").is_some());
}

#[test]
fn nested_groups_supply_the_section_title_badge_and_tint() {
    let config = Config::parse("[[groups]]\nid='team'\nname='Team'\nicon='cmd'\n[[groups]]\nid='qa'\nname='QA'\nparent='team'\ncolor='#123456'\n[[profiles]]\nid='qa-shell'\ngroup='QA'\ncommand='shell'").unwrap();
    let a = App::new(config, profiles(), true, None, Arc::new(|| {}));
    let pick = a.profile_picks().into_iter().find(|p| p.value == "qa-shell").unwrap();
    assert_eq!((pick.group.as_str(), pick.badge.as_str(), pick.tint), ("Team › QA", "C:", Some(0x123456)));
}

#[test]
fn chosen_profiles_become_recent_and_can_be_cleared() {
    let mut a = app(Vec::new());
    a.run(crate::keymap::Action::ProfileSelector);
    let o = a.overlay.as_mut().unwrap();
    o.insert("serial");
    let value = o.selected_value().unwrap();
    assert_eq!(value, "serial:COM3");
    a.overlay = None;
    a.note_recent("serial:COM3");
    a.note_recent("local:pwsh");
    a.note_recent("serial:COM3");
    assert_eq!(a.recent, ["serial:COM3", "local:pwsh"]);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::RecentProfiles(r) if r.len() == 2)));
    let picks = a.profile_picks();
    assert_eq!(picks[0].value, format!("{RECENT}serial:COM3"));
    assert_eq!(picks[2].value, CLEAR_RECENT);
    a.clear_recent();
    assert!(a.profile_picks().iter().all(|p| p.group != "Recent"));
}

#[test]
fn filtering_keeps_groups_together_and_matches_details() {
    let mut a = app(Vec::new());
    a.run(crate::keymap::Action::ProfileSelector);
    let o = a.overlay.as_mut().unwrap();
    assert_eq!(o.kind, OverlayKind::Profiles);
    o.insert("pwsh");
    assert_eq!(o.selected_value().as_deref(), Some("local:pwsh"), "the command matches too");
    o.set_query("");
    assert_eq!(o.header_before(0), Some("Built-in"));
    assert_eq!(o.header_before(1), Some("From ~/.ssh/config"));
    assert_eq!(o.header_before(3), None, "the manage entry has no header");
}

#[test]
fn manage_profiles_opens_the_profiles_page_of_the_settings() {
    let mut a = app(Vec::new());
    a.run(crate::keymap::Action::ProfileSelector);
    let o = a.overlay.as_mut().unwrap();
    o.insert("manage");
    let value = o.selected_value().unwrap();
    assert_eq!(value, MANAGE);
    a.on_event(crate::input::Event::Key { key: crate::input::Key::Enter, mods: crate::input::Mods::NONE, pressed: true });
    let page = a.settings.as_ref().expect("settings open");
    let row = page.shown[page.selected];
    assert!(page.profile_at(row).is_some() || page.is_new_profile(row));
}

#[test]
fn backspace_on_an_empty_query_puts_the_address_up_for_editing() {
    let mut a = app(Vec::new());
    a.run(crate::keymap::Action::ProfileSelector);
    let at = a.overlay.as_ref().unwrap().shown.iter().position(|(i, _)| a.overlay.as_ref().unwrap().items[*i].value == "ssh:db").unwrap();
    a.overlay.as_mut().unwrap().select(at);
    a.on_event(crate::input::Event::Key { key: crate::input::Key::Backspace, mods: crate::input::Mods::NONE, pressed: true });
    assert_eq!(a.overlay.as_ref().unwrap().query_text(), "db");
    a.on_event(crate::input::Event::Key { key: crate::input::Key::Backspace, mods: crate::input::Mods::NONE, pressed: true });
    assert_eq!(a.overlay.as_ref().unwrap().query_text(), "d");
}

#[test]
fn ctrl_enter_opens_the_chosen_profile_in_a_split() {
    let mut a = app(Vec::new());
    a.open_default_tab();
    a.run(crate::keymap::Action::ProfileSelector);
    a.overlay.as_mut().unwrap().insert("serial");
    a.on_event(crate::input::Event::Key { key: crate::input::Key::Enter, mods: crate::input::Mods { ctrl: true, ..crate::input::Mods::NONE }, pressed: true });
    assert!(a.overlay.is_none());
    assert_eq!(a.tabs.len(), 1);
    let panes = a.tabs[0].root.panes();
    assert_eq!(panes.len(), 2);
    assert_eq!(a.panes[&a.tabs[0].focus].profile.id, "serial:COM3");
    assert_eq!(a.recent.first().map(String::as_str), Some("serial:COM3"));
}
