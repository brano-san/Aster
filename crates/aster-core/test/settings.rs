use super::*;
use crate::app::Effect;
use crate::input::Event;
use crate::keymap::Action;
use crate::profile::Profile;
use std::sync::Arc;

fn app() -> App {
    let profiles = vec![Profile::new("local:a", "Alpha", "a.exe", &[]), Profile::new("local:b", "Beta", "b.exe", &[])];
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
    assert_eq!(page.shown.len(), SETTINGS.len());
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
    for _ in 0..SETTINGS.len() {
        key(&mut a, Key::Down);
    }
    let geo = a.settings_geometry().unwrap();
    let page = a.settings.as_ref().unwrap();
    assert_eq!(page.selected, SETTINGS.len() - 1);
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
