use super::*;
use crate::profile::{effective, Profile};

fn group(id: &str, name: &str, parent: &str) -> ProfileGroup {
    ProfileGroup { id: id.into(), name: name.into(), parent: (!parent.is_empty()).then(|| parent.into()), ..ProfileGroup::default() }
}

fn profile(id: &str, group: &str, custom: bool) -> ProfileRow {
    ProfileRow { id: id.into(), name: id.into(), custom, group: (!group.is_empty()).then(|| group.into()), ..ProfileRow::default() }
}

fn fixture() -> (Vec<ProfileGroup>, Vec<ProfileRow>) {
    let groups = vec![group("z", "Zulu", ""), group("rack", "Rack", "a"), group("a", "Alpha", "")];
    let profiles = vec![profile("serial:port", "", false), profile("test-host", "Rack", true), profile("shell", "", true), profile("system", "", false), profile("ssh:web", "", false)];
    (groups, profiles)
}

fn header(key: &str) -> TreeKind {
    TreeKind::Header(Header::Group(key.into()))
}

#[test]
fn tree_orders_groups_by_name_and_indents_nested_profiles() {
    let (groups, profiles) = fixture();
    let rows = tree(&groups, &profiles, &BTreeSet::new(), "");
    use TreeKind::*;
    assert_eq!(rows.iter().map(|r| r.kind.clone()).collect::<Vec<_>>(), [
        header("a"), header("rack"), Profile(1), header("z"), NewGroup,
        Header(super::Header::Ungrouped), Profile(2), NewProfile,
        Header(super::Header::Builtin), Profile(3), Header(super::Header::Imported), Profile(4), Header(super::Header::Serial), Profile(0),
        DefaultsHeader, Defaults("local"), Defaults("ssh"), Defaults("serial"), Defaults("socket"),
    ]);
    assert_eq!((rows[0].indent(1.5), rows[1].indent(1.5), rows[2].indent(1.5)), (0.0, 24.0, 48.0));
}

#[test]
fn search_reveals_matching_profiles_and_ancestors_without_changing_collapse() {
    let (groups, profiles) = fixture();
    let collapsed = BTreeSet::from([Header::Group("a".into()), Header::Imported]);
    let visible = tree(&groups, &profiles, &collapsed, "");
    assert!(!visible.iter().any(|r| r.kind == header("rack") || r.kind == TreeKind::Profile(4)));
    let matches = tree(&groups, &profiles, &collapsed, "host profile");
    assert_eq!(matches.into_iter().map(|r| r.kind).collect::<Vec<_>>(), [header("a"), header("rack"), TreeKind::Profile(1)]);
    let ssh = tree(&groups, &profiles, &collapsed, "ssh:web");
    assert_eq!(ssh[0].kind, TreeKind::Header(Header::Imported));
    assert_eq!(ssh[1].kind, TreeKind::Profile(4));
    assert_eq!(visible, tree(&groups, &profiles, &collapsed, ""));
}

#[test]
fn invalid_parent_links_keep_groups_and_profiles_reachable_once() {
    let groups = vec![group("a", "Alpha", "b"), group("b", "Beta", "Alpha"), group("c", "Gamma", "missing")];
    let profiles = vec![profile("shell", "Unknown", true), profile("remote", "Beta", true)];
    let rows = tree(&groups, &profiles, &BTreeSet::new(), "");
    for key in ["a", "b", "c"] {
        assert_eq!(rows.iter().filter(|r| r.kind == header(key)).count(), 1);
    }
    for i in 0..profiles.len() {
        assert_eq!(rows.iter().filter(|r| r.kind == TreeKind::Profile(i)).count(), 1);
    }
}

#[test]
fn revealing_a_profile_expands_only_its_path() {
    let (groups, profiles) = fixture();
    let other = BTreeSet::from([Header::Group("z".into()), Header::Imported]);
    let mut collapsed = other.clone();
    collapsed.extend([Header::Group("a".into()), Header::Group("rack".into())]);
    assert!(!reveal_profile("missing", &groups, &profiles, &mut collapsed));
    assert!(reveal_profile("test-host", &groups, &profiles, &mut collapsed));
    assert_eq!(collapsed, other);
    assert!(!reveal_profile("test-host", &groups, &profiles, &mut collapsed));
    assert!(tree(&groups, &profiles, &collapsed, "").iter().any(|r| r.kind == TreeKind::Profile(1)));
    assert!(reveal_profile("ssh:web", &groups, &profiles, &mut collapsed));
    assert_eq!(collapsed, BTreeSet::from([Header::Group("z".into())]));
}

#[test]
fn create_rename_and_delete_group_preserve_profile_membership_and_child_links() {
    let mut config = Config::default();
    let mut new = GroupEditor::new(None, &config.groups);
    new.fields[0] = "Lab".into();
    new.fields[2] = "#aaccee".into();
    new.fields[3] = "L".into();
    new.defaults.insert("local".into(), ProfileDefaults { cwd: Some("/lab".into()), ..ProfileDefaults::default() });
    let lab = new.save(&mut config).unwrap();
    let id = config.groups[lab].id.clone();
    config.groups.push(group("arm", "Devices", "Lab"));
    config.profiles.push(Profile { name: "console".into(), group: Some("Lab".into()), ..Profile::default() });
    config.profiles.push(Profile { name: "test".into(), group: Some("Devices".into()), ..Profile::default() });
    let mut edit = GroupEditor::new(Some(lab), &config.groups);
    edit.fields[0] = "Office".into();
    edit.save(&mut config).unwrap();
    let mut config = Config::parse(&config.to_text()).unwrap();
    assert_eq!(config.groups[lab].id, id);
    assert_eq!(config.groups[1].parent.as_deref(), Some(id.as_str()));
    assert_eq!(config.profiles[0].group.as_deref(), Some("Office"));
    let launched = effective(&config.profiles[1], &config);
    assert_eq!(launched.cwd.as_deref(), Some("/lab"));
    assert_eq!(launched.color.as_deref(), Some("#aaccee"));
    assert_eq!(launched.icon.as_deref(), Some("L"));
    delete_group(&mut config, lab);
    assert_eq!(config.profiles[0].group, None);
    assert_eq!(config.profiles[1].group.as_deref(), Some("Devices"));
    assert_eq!(config.groups.len(), 1);
    assert_eq!(config.groups[0].parent, None);
    assert_eq!(effective(&config.profiles[1], &config).cwd, None);
}

#[test]
fn deleting_a_middle_group_promotes_its_children_one_level() {
    let mut config = Config { groups: vec![group("", "Root", ""), group("branch", "Branch", "Root"), group("leaf", "Leaf", "branch")], ..Config::default() };
    delete_group(&mut config, 1);
    assert_eq!(config.groups[1].parent.as_deref(), Some("Root"));
}

#[test]
fn group_validation_rejects_cycles_duplicates_and_invalid_colors_atomically() {
    let mut config = Config { groups: vec![group("ops", "Ops", ""), group("devices", "Devices", "ops"), group("rack", "Rack", "Devices")], ..Config::default() };
    let before = config.clone();
    let mut edit = GroupEditor::new(Some(0), &config.groups);
    assert_eq!(edit.parent_choices(&config.groups).len(), 1);
    for parent in ["ops", "devices", "rack", "missing"] {
        edit.fields[1] = parent.into();
        assert!(edit.save(&mut config).is_err());
        assert_eq!(config, before);
    }
    edit.fields[1].clear();
    edit.fields[0] = "Devices".into();
    assert!(edit.save(&mut config).is_err());
    edit.fields[0] = "Office".into();
    for color in ["red", "334455", "#ff990x", "#123", "★004"] {
        edit.fields[2] = color.into();
        assert!(edit.save(&mut config).is_err());
        assert_eq!(config, before);
    }
}

#[test]
fn editor_geometry_keeps_the_focused_field_and_actions_visible() {
    use aster_gfx::render::Rect;
    use layout::{editor_layout, EditorHit};
    for (width, height, s) in [(1100.0, 650.0, 1.0), (360.0, 375.0, 1.0), (360.0, 152.0, 1.0), (800.0, 480.0, 1.5)] {
        let area = Rect::new(0.0, 36.0, width, height);
        for focus in [0, 4, 17, 50] {
            let panel = editor_layout(area, s, 3, 51, focus);
            let hit = |r: Rect| panel.hit(r.x + r.w * 0.5, r.y + r.h * 0.5);
            assert_eq!(hit(panel.save), Some(EditorHit::Save));
            assert_eq!(hit(panel.cancel), Some(EditorHit::Cancel));
            for (i, tab) in panel.tabs.iter().enumerate() {
                assert_eq!(hit(*tab), Some(EditorHit::Tab(i)));
            }
            let &(_, slot) = panel.fields.iter().find(|(i, _)| *i == focus).expect("focused field visible");
            assert_eq!(hit(slot), Some(EditorHit::Field(focus)));
            assert!(slot.y >= panel.title.bottom());
            assert!(slot.bottom() <= panel.message.y);
            assert!(panel.frame.x >= area.x && panel.frame.bottom() <= area.bottom());
            assert!(panel.thumb.is_some());
        }
    }
}

#[test]
fn group_defaults_and_profile_actions_use_their_drawn_hit_areas() {
    use aster_gfx::render::Rect;
    use layout::{choice_step, editor_layout, row_action, EditorHit};
    let area = Rect::new(0.0, 36.0, 1000.0, 600.0);
    let panel = editor_layout(area, 1.0, 0, 8, 4);
    let heading = panel.group_defaults.unwrap();
    assert!(panel.tabs.is_empty());
    for &(i, r) in &panel.fields {
        assert_eq!(panel.hit(r.x + 1.0, r.y + 1.0), Some(EditorHit::Field(i)));
        if i >= 4 {
            assert!(r.y >= heading.bottom());
        } else {
            assert!(r.bottom() <= heading.y);
        }
        assert_eq!(choice_step(r, 1.0, r.x + 10.0), 0);
        assert_eq!(choice_step(r, 1.0, r.right() - 32.0), -1);
        assert_eq!(choice_step(r, 1.0, r.right() - 9.0), 1);
    }
    let row = Rect::new(230.0, 80.0, 760.0, 36.0);
    let action = row_action(row, 1.0);
    assert!(row.contains(action.x, action.y));
    assert!(action.right() <= row.right() && action.bottom() <= row.bottom());
}

fn model(p: Profile) -> editor::ProfileEditor {
    editor::ProfileEditor::profile(p, None, &Config::default(), &["Night".into()], &["COM7".into()])
}

fn focus(editor: &mut editor::ProfileEditor, label: &str) {
    let (tab, field) = editor.tabs.iter().enumerate().find_map(|(i, t)| {
        t.fields.iter().position(|f| f.label == label).map(|k| (i, k))
    }).expect("editor field");
    editor.select_tab(tab);
    editor.focus = field;
}

fn set_field(editor: &mut editor::ProfileEditor, label: &str, value: &str) {
    focus(editor, label);
    editor.tabs[editor.tab].fields[editor.focus].value = value.into();
}

#[test]
fn editor_tabs_follow_the_connection_and_keep_keyboard_focus_inside_the_tab() {
    use crate::input::{Key, Mods};
    let local = Profile::new("id", "Shell", "cmd", &[]);
    for (profile, expected) in [
        (local.clone(), vec!["General", "Command"]),
        (Profile { ssh: Some("host".into()), ..local.clone() }, vec!["General", "SSH", "Stream"]),
        (Profile { serial: Some("COM3".into()), ..local.clone() }, vec!["General", "Serial", "Stream"]),
        (Profile { socket: Some("tcp://host:3000".into()), ..local }, vec!["General", "Network", "Stream"]),
    ] {
        let mut e = model(profile);
        assert_eq!(e.tabs.iter().map(|tab| tab.label.as_str()).collect::<Vec<_>>(), expected);
        e.key(Key::Tab, Mods { ctrl: true, ..Mods::NONE });
        assert_eq!((e.tab, e.focus), (1, 0));
        e.key(Key::Tab, Mods { shift: true, ..Mods::NONE });
        assert_eq!(e.focus + 1, e.tabs[1].fields.len());
        e.key(Key::Tab, Mods { ctrl: true, shift: true, ..Mods::NONE });
        assert_eq!((e.tab, e.focus), (0, 0));
    }
}

#[test]
fn environment_and_identity_lists_insert_and_delete_individual_rows() {
    use crate::input::{Key, Mods};
    let mut e = model(Profile::new("id", "Shell", "cmd", &[]));
    focus(&mut e, "Environment (key=value)");
    e.insert("A=first=second ");
    e.key(Key::Enter, Mods::NONE);
    e.insert("B=two");
    assert_eq!(e.profile_result().unwrap().env["A"], "first=second ");
    assert_eq!(e.profile_result().unwrap().env["B"], "two");
    e.key(Key::Delete, Mods::NONE);
    assert_eq!(e.profile_result().unwrap().env.len(), 1);
    e.key(Key::Enter, Mods::NONE);
    e.insert("A=duplicate");
    assert!(e.profile_result().is_err());
    let mut e = model(Profile { name: "Host".into(), ssh: Some("host".into()), ..Profile::default() });
    focus(&mut e, "Identity files");
    e.insert("first");
    e.key(Key::Enter, Mods::NONE);
    e.insert("second file");
    assert_eq!(e.profile_result().unwrap().ssh_options.unwrap().identity_files.unwrap(), ["first", "second file"]);
    e.key(Key::Up, Mods::NONE);
    e.key(Key::Delete, Mods::NONE);
    assert_eq!(e.profile_result().unwrap().ssh_options.unwrap().identity_files.unwrap(), ["second file"]);
}

#[test]
fn login_script_steps_round_trip_with_whitespace_and_delete_as_units() {
    use crate::input::{Key, Mods};
    let mut e = model(Profile { name: "Console".into(), socket: Some("telnet://console".into()), ..Profile::default() });
    focus(&mut e, "Login script · Expect");
    e.insert("login: ");
    e.key(Key::Tab, Mods::NONE);
    e.insert(r"operator\r");
    e.key(Key::Tab, Mods::NONE);
    e.key(Key::Space, Mods::NONE);
    e.key(Key::Tab, Mods::NONE);
    e.key(Key::Space, Mods::NONE);
    e.key(Key::Enter, Mods::NONE);
    e.insert("next");
    let config = Config { profiles: vec![e.profile_result().unwrap()], ..Config::default() };
    let parsed = Config::parse(&config.to_text()).unwrap();
    let launched = effective(&parsed.profiles[0], &parsed);
    let steps = launched.stream.unwrap().options().login;
    assert_eq!(steps.len(), 2);
    assert_eq!((&*steps[0].expect, &*steps[0].send, steps[0].regex, steps[0].optional), ("login: ", r"operator\r", true, true));
    e.key(Key::Tab, Mods::NONE);
    e.key(Key::Delete, Mods::NONE);
    assert_eq!(e.profile_result().unwrap().stream.unwrap().login.unwrap().len(), 1);
}

#[test]
fn serial_fields_compose_a_spec_and_preserve_sparse_defaults() {
    let mut e = model(Profile { name: "Board".into(), serial: Some("COM7".into()), ..Profile::default() });
    assert_eq!(e.profile_result().unwrap().serial.as_deref(), Some("COM7"));
    set_field(&mut e, "Speed", "?");
    set_field(&mut e, "Data bits", "7");
    let partial = e.profile_result().unwrap();
    assert_eq!(partial.serial.as_deref(), Some("COM7:?,7??"));
    let reopened = model(partial);
    assert_eq!(reopened.tabs[1].fields.iter().find(|f| f.label == "Parity").unwrap().value, "");
    set_field(&mut e, "Parity", "even");
    set_field(&mut e, "Stop bits", "2");
    set_field(&mut e, "Flow control", "hardware");
    set_field(&mut e, "Slow send", "true");
    let p = e.profile_result().unwrap();
    assert_eq!(p.serial.as_deref(), Some("COM7:?,7E2,rtscts,slow"));
    let mut def = editor::ProfileEditor::defaults("serial", &ProfileDefaults::default(), &Config::default(), &[], &[]);
    set_field(&mut def, "Speed", "9600");
    set_field(&mut def, "Slow send", "true");
    let d = def.defaults_result().unwrap();
    let config = Config { profile_defaults: [("serial".into(), d)].into(), ..Config::default() };
    let config = Config::parse(&config.to_text()).unwrap();
    let bare = Profile { serial: Some("COM7".into()), ..p.clone() };
    assert_eq!(effective(&bare, &config).serial.as_deref(), Some("COM7:9600,8N1,slow"));
    assert_eq!(effective(&p, &config).serial, p.serial);
    set_field(&mut e, "Slow send", "false");
    assert!(!aster_proto::serial::parse(&effective(&e.profile_result().unwrap(), &config).serial.unwrap()).unwrap().slow);
    set_field(&mut e, "Speed", "banana");
    assert!(e.profile_result().is_err());
}

#[test]
fn defaults_editor_merges_group_and_type_values_after_config_round_trip() {
    let mut config = Config { groups: vec![group("ops", "Ops", "")], ..Config::default() };
    let mut e = editor::ProfileEditor::defaults("ssh", &ProfileDefaults::default(), &config, &["Night".into()], &[]);
    assert!(e.tabs.iter().flat_map(|tab| &tab.fields).all(|f| !matches!(f.label.as_str(), "Name" | "Hidden" | "Group" | "Command line")));
    set_field(&mut e, "User", "alice");
    set_field(&mut e, "Port", "2222");
    set_field(&mut e, "Input mode", "line-hex");
    set_field(&mut e, "Color scheme", "night");
    config.profile_defaults.insert("ssh".into(), e.defaults_result().unwrap());
    let mut e = editor::ProfileEditor::defaults("ssh", &ProfileDefaults::default(), &config, &["Night".into()], &[]);
    set_field(&mut e, "Port", "2200");
    set_field(&mut e, "SSH destination", "example.test");
    set_field(&mut e, "Remember folder", "false");
    config.groups[0].defaults.insert("ssh".into(), e.defaults_result().unwrap());
    let source = Profile { name: "Host".into(), ssh: Some("old.test".into()), command: "ssh".into(), args: vec!["old.test".into()], ..Profile::default() };
    let mut e = editor::ProfileEditor::profile(source, None, &config, &["Night".into()], &[]);
    set_field(&mut e, "Group", "Ops");
    set_field(&mut e, "SSH destination", "");
    config.profiles.push(e.profile_result().unwrap());
    let config = Config::parse(&config.to_text()).unwrap();
    let p = effective(&config.profiles[0], &config);
    assert_eq!(p.ssh.as_deref(), Some("example.test"));
    assert_eq!(p.args, ["example.test"]);
    assert_eq!(p.scheme.as_deref(), Some("Night"));
    let options = p.ssh_options.unwrap();
    assert_eq!((options.user.as_deref(), options.port, options.remember_cwd), (Some("alice"), Some(2200), Some(false)));
    assert_eq!(p.stream.unwrap().input, Some(crate::profile::InputMode::LineHex));
}

#[test]
fn editor_rejects_invalid_address_and_scheme_and_keeps_unsupported_profile_values() {
    let mut e = model(Profile { name: "Lab".into(), socket: Some("tcp://server".into()), ..Profile::default() });
    assert!(e.profile_result().is_err());
    set_field(&mut e, "Telnet or TCP address", "tcp://server:8080");
    set_field(&mut e, "Color scheme", "wrong");
    assert!(e.profile_result().is_err());
    let mut p = Profile { name: "Host".into(), ssh: Some("example.test".into()), ..Profile::default() };
    p.ssh_options = Some(crate::profile::SshOptions {
        algorithms: Some(aster_proto::ssh::AlgorithmOptions { kex: Some(vec!["test".into()]), ..Default::default() }),
        ..Default::default()
    });
    p.cwd = Some("/remote".into());
    p.extra.insert("future-option".into(), toml::Value::Array(vec![toml::Value::Integer(23)]));
    let mut e = model(p.clone());
    set_field(&mut e, "Name", "Renamed");
    let edited = e.profile_result().unwrap();
    assert_eq!(edited.ssh_options, p.ssh_options);
    assert_eq!(edited.cwd, p.cwd);
    let config = Config { profiles: vec![edited], ..Config::default() };
    assert_eq!(Config::parse(&config.to_text()).unwrap().profiles[0].extra, p.extra);
}

#[test]
fn admin_preference_can_inherit_or_explicitly_disable_an_elevated_local_default() {
    let config = Config { profile_defaults: [("local".into(), ProfileDefaults { admin: Some(true), ..ProfileDefaults::default() })].into(), ..Config::default() };
    let mut p = Profile::new("local", "Shell", "shell", &[]);
    assert_eq!(effective(&p, &config).elevated, cfg!(windows));
    p.admin = Some(false);
    let p: Profile = toml::from_str(&toml::to_string(&p).unwrap()).unwrap();
    assert_eq!(p.admin, Some(false));
    assert!(!effective(&p, &config).elevated);
}
