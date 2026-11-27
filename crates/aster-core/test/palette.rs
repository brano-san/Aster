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

#[test]
fn snippets_match_name_tags_and_body_and_paste_only_into_the_focused_pane() {
    let mut a = app();
    let config = Config::parse("[snippets.'Working tree']\ntext = \"git status\\nnext\\n\"\ntags = ['repo-maintenance']").unwrap();
    a.set_config(config);
    a.open_default_tab();
    a.run(Action::SplitRight);
    a.run(Action::BroadcastToTab);
    let focus = a.focused_pane().unwrap();
    let other = a.tabs[a.active].root.panes()[0];
    a.run(Action::CommandSelector);
    let palette = a.overlay.as_mut().unwrap();
    for query in ["Working tree", "repo-maintenance", "git status"] {
        palette.set_query(query);
        assert_eq!(palette.selected_pick().unwrap().label, "Working tree");
    }
    a.on_event(Event::Key { key: Key::Enter, mods: Mods::NONE, pressed: true });
    a.on_event(Event::Char('\r'));
    assert_eq!(a.panes[&focus].queued, b"git status next");
    assert!(a.panes[&other].queued.is_empty());
}

#[test]
fn saving_selection_writes_a_new_named_snippet_to_the_config() {
    let mut a = app();
    a.set_config(Config::parse("[snippets.'echo abc']\ntext = 'keep me'").unwrap());
    a.open_default_tab();
    let terminal = a.focused_terminal().unwrap();
    terminal.lock().feed(b"echo abc");
    terminal.lock().select_all();
    a.run(Action::SaveSelectionAsSnippet);
    let saved = a.take_effects().into_iter().find_map(|effect| match effect {
        crate::app::Effect::SaveConfig(text) => Some(Config::parse(&text).unwrap()),
        _ => None,
    }).unwrap();
    assert_eq!(saved.snippets["echo abc"].text, "keep me");
    assert_eq!(saved.snippets["echo abc (2)"].text, "echo abc");
}

#[test]
fn bracketed_snippet_input_preserves_lines_without_a_final_enter_or_control_bytes() {
    let mut a = app();
    a.set_config(Config::parse("[snippets.Script]\ntext = \"one\\ntwo\\u0017\\r\\n\"").unwrap());
    a.open_default_tab();
    a.focused_terminal().unwrap().lock().feed(b"\x1b[?2004h");
    a.run(Action::CommandSelector);
    a.overlay.as_mut().unwrap().set_query("Script");
    a.on_event(Event::Key { key: Key::Enter, mods: Mods::NONE, pressed: true });
    a.on_event(Event::Char('\r'));
    assert_eq!(a.panes[&a.focused_pane().unwrap()].queued, b"\x1b[200~one\rtwo\x1b[201~");
}
