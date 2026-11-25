use super::*;
use crate::config::Config;
use crate::input::{Event, Key, Mods};
use crate::overlay::{Overlay, OverlayKind};
use crate::profile::Profile;
use std::sync::Arc;

fn app() -> App {
    crate::i18n::set_language("en");
    App::new(Config::default(), vec![Profile::new("shell", "Shell", "shell", &[])], true, None, Arc::new(|| {}))
}

#[test]
fn palette_includes_the_context_menu_paths_and_runs_the_same_action() {
    let mut a = app();
    a.open_default_tab();
    a.open_default_tab();
    for (parent, entries) in [("Tab", a.tab_menu_entries(a.active)), ("Terminal", a.terminal_menu_entries())] {
        for entry in entries {
            if let Entry::Item { label, action, .. } = entry {
                assert!(a.command_picks().iter().any(|p| p.label == format!("{parent} > {label}") && p.value == action.id()));
            }
        }
    }
    a.run(Action::CommandSelector);
    let overlay = a.overlay.as_mut().unwrap();
    overlay.set_query("Tab > Close other tabs");
    assert_eq!(overlay.selected_value().as_deref(), Some("close-other-tabs"));
    a.on_event(Event::Key { key: Key::Enter, mods: Mods::NONE, pressed: true });
    assert_eq!(a.tabs.len(), 1);
}

#[test]
fn palette_search_finds_english_paths_in_russian_and_formats_nested_paths() {
    let a = app();
    crate::i18n::set_language("ru");
    let mut palette = Overlay::new(OverlayKind::Commands, a.command_picks());
    palette.set_query("Tab > Close other tabs");
    assert_eq!(palette.selected_pick().unwrap().label, "Вкладка > Закрыть другие вкладки");
    assert_eq!(menu_path(&["Tab", "Terminal"], "Copy", t), "Вкладка > Терминал > Копировать");
    crate::i18n::set_language("en");
}

#[test]
fn settings_run_buttons_and_picker_window_commands_are_listed_and_bindable() {
    use crate::settings::{Control, SETTINGS};
    let mut a = app();
    let mut actions: Vec<_> = SETTINGS.iter().filter_map(|setting| match setting.control {
        Control::Run { action, .. } => Some(action.clone()),
        _ => None,
    }).collect();
    actions.extend([Action::ManageProfiles, Action::ClearRecentProfiles, Action::MinimizeWindow, Action::ToggleMaximize, Action::CloseWindow, Action::Tab(20)]);
    if cfg!(windows) {
        actions.push(Action::Quit);
    }
    let picks = a.command_picks();
    for action in actions {
        assert!(Action::all().any(|candidate| candidate == action));
        assert_eq!(Action::from_id(&action.id()), Some(action.clone()));
        if let Action::DefaultTerminal(enable) = &action {
            use crate::default_terminal::State;
            assert!(!picks.iter().any(|pick| pick.value == action.id()));
            a.set_default_terminal_state(if *enable { State::Available } else { State::Selected });
            assert!(a.command_picks().iter().any(|pick| pick.value == action.id()));
        } else {
            assert!(picks.iter().any(|pick| pick.value == action.id()));
        }
        let config = std::collections::BTreeMap::from([(action.id(), vec!["F8".to_owned()])]);
        let mut keymap = crate::keymap::Keymap::new(&config);
        assert_eq!(keymap.press(crate::keymap::Chord::parse("F8").unwrap()), crate::keymap::Outcome::Action(action));
    }
    a.note_recent("shell");
    a.run(Action::ClearRecentProfiles);
    assert!(a.recent.is_empty());
    a.run(Action::ManageProfiles);
    assert!(a.settings_open());
}

#[cfg(windows)]
#[test]
fn quit_uses_the_platforms_existing_application_exit_workflow() {
    let mut a = app();
    a.run(Action::Quit);
    assert!(a.take_effects().iter().any(|effect| matches!(effect, crate::app::Effect::Quit)));
}

#[test]
fn hiding_an_action_removes_its_palette_aliases_and_preserves_the_binding() {
    let mut a = app();
    let mut config = Config::parse("[palette]\nhidden-commands = ['close-other-tabs', 'profile.shell']\n[hotkeys]\nclose-other-tabs = ['F8']").unwrap();
    a.set_config(config.clone());
    let picks = a.command_picks();
    assert!(!picks.iter().any(|p| matches!(p.value.as_str(), "close-other-tabs" | "profile.shell")));
    assert!(picks.iter().any(|p| p.label == "Tab > Close"));
    assert_eq!(a.shortcut_text(Action::CloseOtherTabs), "F8");
    config.palette.hidden_commands.clear();
    a.set_config(config);
    assert_eq!(a.command_picks().iter().filter(|p| p.value == "close-other-tabs").count(), 2);
}
