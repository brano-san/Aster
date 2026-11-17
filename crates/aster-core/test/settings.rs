use super::*;
use crate::app::Effect;
use crate::input::Event;
use crate::keymap::Action;
use crate::profile::Profile;
use std::sync::Arc;

fn app() -> App {
    let mut alpha = Profile::new("local:a", "Alpha", "a.exe", &[]);
    alpha.env.insert("MARK".into(), "1".into());
    let profiles = vec![alpha, Profile::new("local:b", "Beta", "b.exe", &[])];
    let mut a = App::new(Config::default(), profiles, true, None, Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    a
}

fn key(a: &mut App, k: Key) {
    a.on_event(Event::Key { key: k, mods: Mods::NONE, pressed: true });
}

fn typed(a: &mut App, text: &str) {
    for c in text.chars() {
        a.on_event(Event::Char(c));
    }
}

fn saved(a: &mut App) -> Option<Config> {
    a.take_effects().into_iter().filter_map(|e| if let Effect::SaveConfig(t) = e { Config::parse(&t).ok() } else { None }).next_back()
}

#[test]
fn every_setting_points_at_a_config_value_of_the_matching_type() {
    let config = Config::default();
    for s in SETTINGS {
        let value = get(&config, s.key).unwrap_or_else(|| panic!("{} is missing", s.key));
        let fits = match s.control {
            Control::Toggle => value.is_bool(),
            Control::Number { .. } => value.is_integer() || value.is_float(),
            Control::Choice(_) | Control::Schemes | Control::Profiles | Control::Text => value.is_str(),
        };
        assert!(fits, "{} holds {value:?}", s.key);
        if let Control::Choice(options) = s.control {
            assert!(options.iter().any(|(v, _)| Some(*v) == value.as_str()), "{} default is not an option", s.key);
        }
    }
}

#[test]
fn setting_keys_are_unique() {
    let mut keys: Vec<_> = SETTINGS.iter().map(|s| s.key).collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), SETTINGS.len());
}

#[test]
fn values_are_written_by_path_and_type_checked() {
    let config = set(&Config::default(), "terminal.search-options.regex", Value::Boolean(true)).unwrap();
    assert!(config.terminal.search_options.regex);
    let config = set(&config, "appearance.dock", Value::String("left".into())).unwrap();
    assert_eq!(config.appearance.dock, crate::config::Dock::Left);
    assert!(set(&config, "appearance.dock", Value::String("middle".into())).is_err());
    assert!(set(&config, "terminal.font-size", Value::String("big".into())).is_err());
}

#[test]
fn numbers_print_without_float_noise() {
    assert_eq!(number_text(14.0), "14");
    assert_eq!(number_text(0.550000011920929), "0.55");
}

#[test]
fn the_query_filters_by_every_word_across_label_hint_and_section() {
    let mut page = SettingsPage::new();
    assert_eq!(page.shown.iter().filter(|&&i| i < SETTINGS.len()).count(), SETTINGS.len());
    assert!(page.shown.iter().any(|&i| shortcut(i).is_some()), "shortcuts are listed too");
    page.query = "dock top".into();
    page.refilter();
    let keys: Vec<_> = page.shown.iter().map(|&i| SETTINGS[i].key).collect();
    assert_eq!(keys, ["appearance.dock-always-on-top"]);
}

#[test]
fn enter_toggles_a_switch_and_saves_the_config() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "blinking");
    key(&mut a, Key::Enter);
    assert!(!a.config().terminal.cursor_blink);
    assert_eq!(saved(&mut a).map(|c| c.terminal.cursor_blink), Some(false));
    key(&mut a, Key::Escape);
    assert!(a.settings_open(), "the first escape clears the query");
    key(&mut a, Key::Escape);
    assert!(!a.settings_open());
}

#[test]
fn arrows_step_numbers_within_bounds_and_cycle_choices() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "font size");
    key(&mut a, Key::Right);
    assert_eq!(a.config().terminal.font_size, 15.0);
    for _ in 0..60 {
        key(&mut a, Key::Right);
    }
    assert_eq!(a.config().terminal.font_size, 48.0);
    a.settings = Some(SettingsPage::new());
    typed(&mut a, "cursor shape");
    key(&mut a, Key::Left);
    assert_eq!(a.config().terminal.cursor, crate::config::CursorStyle::Underline);
    key(&mut a, Key::Right);
    assert_eq!(a.config().terminal.cursor, crate::config::CursorStyle::Block);
}

#[test]
fn integer_settings_stay_integers() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "scrollback");
    key(&mut a, Key::Right);
    assert_eq!(a.config().terminal.scrollback_lines, 26_000);
    assert!(saved(&mut a).is_some());
}

#[test]
fn text_and_numbers_are_edited_in_place() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "interface font");
    key(&mut a, Key::Enter);
    typed(&mut a, "Inter");
    key(&mut a, Key::Enter);
    assert_eq!(a.config().appearance.ui_font, "Inter");
    a.settings = Some(SettingsPage::new());
    typed(&mut a, "line padding");
    key(&mut a, Key::Enter);
    key(&mut a, Key::Backspace);
    typed(&mut a, "7");
    key(&mut a, Key::Escape);
    assert_eq!(a.config().terminal.line_padding, 0.0, "escape drops the edit");
    key(&mut a, Key::Enter);
    key(&mut a, Key::Backspace);
    typed(&mut a, "99");
    key(&mut a, Key::Enter);
    assert_eq!(a.config().terminal.line_padding, 20.0, "typed numbers are clamped");
}

#[test]
fn the_default_profile_offers_the_automatic_choice_and_every_profile() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "default profile");
    key(&mut a, Key::Right);
    assert_eq!(a.config().terminal.profile, "local:a");
    key(&mut a, Key::Right);
    key(&mut a, Key::Right);
    assert_eq!(a.config().terminal.profile, "");
}

#[test]
fn keys_do_not_reach_the_terminal_while_settings_are_open() {
    let mut a = app();
    a.open_default_tab();
    a.run(Action::Settings);
    typed(&mut a, "ls");
    key(&mut a, Key::Enter);
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::Write { .. })));
}

#[test]
fn moving_the_selection_scrolls_it_into_view() {
    let mut a = app();
    a.set_viewport(1000, 360, 1.0);
    a.chrome = crate::view::chrome(1000.0, 360.0, 1.0, 1);
    a.run(Action::Settings);
    let rows = a.settings.as_ref().unwrap().shown.len();
    for _ in 0..rows {
        key(&mut a, Key::Down);
    }
    let geo = a.settings_geometry().unwrap();
    let page = a.settings.as_ref().unwrap();
    assert_eq!(page.selected, rows - 1);
    let row = geo.rows.iter().find(|r| r.kind == RowKind::Item(page.selected)).unwrap();
    assert!(row.rect.bottom() <= geo.list.bottom() + 0.5 && row.rect.y >= geo.list.y);
    assert!(page.scroll > 0.0);
}

#[test]
fn clicking_a_switch_row_toggles_it() {
    let mut a = app();
    a.chrome = crate::view::chrome(1000.0, 700.0, 1.0, 1);
    a.run(Action::Settings);
    typed(&mut a, "copy on select");
    let row = a.settings_geometry().unwrap().rows.into_iter().find(|r| matches!(r.kind, RowKind::Item(_))).unwrap();
    a.settings_click(row.rect.x + 20.0, row.rect.y + 10.0);
    assert!(!a.config().terminal.copy_on_select);
}

#[test]
fn each_section_is_listed_in_one_piece() {
    let mut seen: Vec<&str> = Vec::new();
    for s in SETTINGS {
        if seen.last() != Some(&s.section) {
            assert!(!seen.contains(&s.section), "{} appears in two places", s.section);
            seen.push(s.section);
        }
    }
}

fn ctrl_shift(c: char) -> Mods {
    let _ = c;
    Mods { ctrl: true, shift: true, ..Mods::NONE }
}

fn select_shortcut(a: &mut App, query: &str) {
    a.run(Action::Settings);
    typed(a, query);
    let page = a.settings.as_ref().unwrap();
    let at = page.shown.iter().position(|&i| shortcut(i).is_some()).expect("a shortcut row");
    a.settings.as_mut().unwrap().selected = at;
}

#[test]
fn a_captured_shortcut_replaces_the_binding_and_frees_it_elsewhere() {
    let mut a = app();
    select_shortcut(&mut a, "new tab");
    key(&mut a, Key::Enter);
    assert!(a.settings.as_ref().unwrap().capture.is_some());
    a.on_event(Event::Key { key: Key::Char('P'), mods: ctrl_shift('P'), pressed: true });
    let config = saved(&mut a).expect("saved");
    assert_eq!(config.hotkeys.get("new-tab"), Some(&vec!["Ctrl-Shift-P".to_owned()]));
    assert_eq!(config.hotkeys.get("command-selector"), Some(&Vec::<String>::new()), "the old owner loses it");
    assert!(a.settings.as_ref().unwrap().notice.as_deref().is_some_and(|n| n.contains("Show command selector")));
    assert_eq!(a.shortcut_text(Action::NewTab), "Ctrl-Shift-P");
}

#[test]
fn escape_cancels_capture_and_delete_clears_or_restores_the_default() {
    let mut a = app();
    select_shortcut(&mut a, "new tab");
    key(&mut a, Key::Enter);
    key(&mut a, Key::Escape);
    assert!(a.settings.as_ref().unwrap().capture.is_none());
    assert!(a.settings_open(), "escape only ends the capture");
    key(&mut a, Key::Delete);
    assert_eq!(a.shortcut_text(Action::NewTab), "None");
    a.on_event(Event::Key { key: Key::Delete, mods: Mods { shift: true, ..Mods::NONE }, pressed: true });
    assert_eq!(a.shortcut_text(Action::NewTab), "Ctrl-Shift-T");
    assert!(!a.config().hotkeys.contains_key("new-tab"));
}

#[test]
fn the_settings_shortcut_itself_can_be_captured() {
    let mut a = app();
    select_shortcut(&mut a, "open settings");
    key(&mut a, Key::Enter);
    a.on_event(Event::Key { key: Key::Char(','), mods: Mods { ctrl: true, ..Mods::NONE }, pressed: true });
    assert!(a.settings_open());
    assert_eq!(a.shortcut_text(Action::Settings), "Ctrl-,");
}

fn select_profile(a: &mut App, name: &str) {
    a.run(Action::Settings);
    let page = a.settings.as_ref().unwrap();
    let at = page.shown.iter().position(|&i| match page.profile_at(i) {
        Some(p) => p.name == name,
        None => name.is_empty() && page.is_new_profile(i),
    }).expect("a profile row");
    a.settings.as_mut().unwrap().selected = at;
}

fn ctrl(k: Key) -> Event {
    Event::Key { key: k, mods: Mods { ctrl: true, ..Mods::NONE }, pressed: true }
}

#[test]
fn arguments_split_on_spaces_and_respect_quotes() {
    assert_eq!(split_args(r#"-NoLogo -File "C:\My Scripts\a.ps1" ''"#), ["-NoLogo", "-File", r"C:\My Scripts\a.ps1", ""]);
    assert_eq!(join_args(&["-d".into(), "My Distro".into()]), r#"-d "My Distro""#);
}

#[test]
fn a_detected_profile_is_copied_into_an_editable_custom_one() {
    let mut a = app();
    select_profile(&mut a, "Alpha");
    key(&mut a, Key::Enter);
    let editor = a.settings.as_ref().unwrap().editor.clone().expect("editor");
    assert_eq!(editor.fields[0], "Alpha (copy)");
    assert_eq!(editor.original, None);
    key(&mut a, Key::Tab);
    key(&mut a, Key::Tab);
    typed(&mut a, r#"-l "two words""#);
    key(&mut a, Key::Tab);
    typed(&mut a, r"D:\work");
    a.on_event(ctrl(Key::Enter));
    let config = saved(&mut a).expect("saved");
    let custom = config.profiles.iter().find(|p| p.name == "Alpha (copy)").expect("custom profile");
    assert_eq!(custom.id, "custom:alpha-copy");
    assert_eq!(custom.args, ["-l", "two words"]);
    assert_eq!(custom.cwd.as_deref(), Some(r"D:\work"));
    assert_eq!(custom.env.get("MARK").map(String::as_str), Some("1"), "the copy keeps the environment of its source");
    let page = a.settings.as_ref().unwrap();
    assert!(page.editor.is_none());
    assert_eq!(page.profile_at(page.shown[page.selected]).map(|p| p.custom), Some(true));
}

#[test]
fn a_new_ssh_profile_needs_only_a_name_and_a_destination() {
    let mut a = app();
    select_profile(&mut a, "");
    key(&mut a, Key::Enter);
    a.on_event(ctrl(Key::Enter));
    assert!(a.settings.as_ref().unwrap().editor.as_ref().unwrap().error.is_some(), "a nameless profile is refused");
    typed(&mut a, "Prod");
    for _ in 0..4 {
        key(&mut a, Key::Tab);
    }
    typed(&mut a, "deploy@prod");
    a.on_event(ctrl(Key::Enter));
    let config = saved(&mut a).expect("saved");
    let p = config.profiles.iter().find(|p| p.name == "Prod").unwrap();
    assert_eq!((p.ssh.as_deref(), p.command.as_str(), p.args.as_slice()), (Some("deploy@prod"), "ssh", &["deploy@prod".to_owned()][..]));
}

#[test]
fn ctrl_enter_makes_a_profile_the_default_and_custom_ones_delete_after_a_second_press() {
    let mut a = app();
    select_profile(&mut a, "Beta");
    a.on_event(ctrl(Key::Enter));
    assert_eq!(a.config().terminal.profile, "local:b");
    assert!(a.settings.as_ref().unwrap().profiles.iter().any(|p| p.default && p.name == "Beta"));
    key(&mut a, Key::Delete);
    assert!(a.settings.as_ref().unwrap().notice.as_deref().is_some_and(|n| n.contains("cannot be deleted")));

    select_profile(&mut a, "");
    key(&mut a, Key::Enter);
    typed(&mut a, "Mine");
    key(&mut a, Key::Tab);
    typed(&mut a, "bash");
    a.on_event(ctrl(Key::Enter));
    saved(&mut a);
    let page = a.settings.as_ref().unwrap();
    assert_eq!(page.profile_at(page.shown[page.selected]).map(|p| p.name.as_str()), Some("Mine"));
    a.on_event(ctrl(Key::Enter));
    key(&mut a, Key::Delete);
    assert!(a.config().profiles.iter().any(|p| p.name == "Mine"), "the first press only asks");
    key(&mut a, Key::Delete);
    assert!(!a.config().profiles.iter().any(|p| p.name == "Mine"));
    assert_eq!(a.config().terminal.profile, "", "deleting the default profile falls back to automatic");
}

#[test]
fn a_serial_profile_needs_only_a_name_and_a_port() {
    let mut a = app();
    select_profile(&mut a, "");
    key(&mut a, Key::Enter);
    typed(&mut a, "Router console");
    for _ in 0..5 {
        key(&mut a, Key::Tab);
    }
    typed(&mut a, "COM3:9600,8N1");
    a.on_event(ctrl(Key::Enter));
    let config = saved(&mut a).expect("saved");
    let p = config.profiles.iter().find(|p| p.name == "Router console").unwrap();
    assert_eq!(p.serial.as_deref(), Some("COM3:9600,8N1"));
    assert!(a.settings.as_ref().unwrap().profiles.iter().any(|r| r.detail == "serial COM3:9600,8N1"));
}

#[test]
fn switching_the_language_setting_translates_the_interface_at_once() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "language");
    key(&mut a, Key::Right);
    assert_eq!(a.config().appearance.language, "en");
    assert_eq!(saved(&mut a).map(|c| c.appearance.language), Some("en".to_owned()));
    key(&mut a, Key::Right);
    assert_eq!(a.config().appearance.language, "ru");
    assert_eq!(crate::i18n::language(), "ru");
    let setting = a.settings.as_ref().unwrap().current().unwrap();
    assert_eq!(a.setting_text(setting), "Русский");
    assert_eq!(a.shortcut_text(Action::SplitLeft), "Нет");
    key(&mut a, Key::Right);
    assert_eq!(crate::i18n::language(), "en", "automatic follows the English system language");
    assert_eq!(a.setting_text(setting), "Automatic");
    crate::i18n::set_language("en");
}

#[test]
fn a_network_profile_stores_a_normalized_address() {
    let mut a = app();
    select_profile(&mut a, "");
    key(&mut a, Key::Enter);
    typed(&mut a, "Switch");
    for _ in 0..6 {
        key(&mut a, Key::Tab);
    }
    typed(&mut a, "tcp://switch");
    key(&mut a, Key::Enter);
    assert!(a.settings.as_ref().unwrap().editor.as_ref().unwrap().error.is_some(), "TCP needs a port");
    for _ in 0..12 {
        key(&mut a, Key::Backspace);
    }
    typed(&mut a, "switch.lan");
    key(&mut a, Key::Enter);
    let config = saved(&mut a).expect("saved");
    let p = config.profiles.iter().find(|p| p.name == "Switch").unwrap();
    assert_eq!(p.socket.as_deref(), Some("telnet://switch.lan:23"));
    assert!(!p.is_local());
    assert!(a.settings.as_ref().unwrap().profiles.iter().any(|r| r.detail == "telnet://switch.lan:23"));
}

#[test]
fn the_first_letter_typed_after_opening_settings_by_shortcut_is_kept() {
    let mut a = app();
    a.on_event(ctrl(Key::Char(',')));
    assert!(a.settings.is_some());
    a.on_event(Event::Char('n'));
    a.on_event(Event::Char('e'));
    assert_eq!(a.settings.as_ref().unwrap().query, "ne");
    let mut b = app();
    b.on_event(Event::Key { key: Key::Char(','), mods: Mods { ctrl: true, ..Mods::NONE }, pressed: true });
    b.on_event(Event::Char(','));
    b.on_event(Event::Char('\u{1b}'));
    b.on_event(Event::Char('x'));
    assert_eq!(b.settings.as_ref().unwrap().query, "x", "the chord's own character is still swallowed");
}
