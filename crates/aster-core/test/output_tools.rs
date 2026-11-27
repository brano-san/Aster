use crate::app::{App, Effect};
use crate::config::Config;
use crate::input::{Event, Key, Mods};
use crate::keymap::Action;
use crate::profile::Profile;
use std::sync::Arc;
use std::time::Duration;

fn app() -> App {
    let mut app = App::new(Config::default(), vec![Profile::new("local:test", "Shell", "shell", &[])], true, None, Arc::new(|| {}));
    app.open_default_tab();
    app.take_effects();
    app
}

#[test]
fn quick_select_copies_a_visible_object_without_sending_input() {
    let mut app = app();
    app.focused_terminal().unwrap().lock().feed(b"https://example.test\r\n");
    app.run(Action::from_id("quick-select").unwrap());
    app.on_event(Event::Char('a'));
    let effects = app.take_effects();
    assert!(effects.iter().any(|e| matches!(e, Effect::SetClipboard(text) if text == "https://example.test")));
    assert!(!effects.iter().any(|e| matches!(e, Effect::Write { .. })));
}

#[test]
fn session_search_is_available_alongside_panel_search() {
    assert!(Action::from_id("search-sessions").is_some());
    assert!(Action::from_id("search").is_some());
}

#[test]
fn command_history_action_lists_more_than_the_latest_completed_command() {
    let mut app = app();
    let terminal = app.focused_terminal().unwrap();
    terminal.lock().feed(b"\x1b]133;A\x07$ \x1b]133;B\x07first\r\n\x1b]133;C\x07old output\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ \x1b]133;B\x07second\r\n\x1b]133;C\x07new output\r\n\x1b]133;D;7\x07");
    app.run(Action::from_id("command-history").expect("the command palette exposes command history"));
    let overlay = app.overlay.as_ref().unwrap();
    let labels: Vec<_> = overlay.items.iter().map(|item| item.label.as_str()).collect();
    assert_eq!(labels, ["second", "first"]);
}

fn command_history_app() -> App {
    let mut app = app();
    let terminal = app.focused_terminal().unwrap();
    terminal.lock().feed(b"\x1b]133;A\x07$ \x1b]133;B\x07first\r\nleft needle \x1b]133;C\x07inside needle one\x1b]133;D;0\x07 right needle\r\n\x1b]133;A\x07$ \x1b]133;B\x07second\r\n\x1b]133;C\x07newer needle\r\n\x1b]133;D;7\x07");
    app.run(Action::CommandHistory);
    app.overlay.as_mut().unwrap().select(1);
    app
}

fn enter(app: &mut App) {
    app.on_event(Event::Key { key: Key::Enter, mods: Mods::NONE, pressed: true });
    app.on_event(Event::Char('\r'));
}

fn ctrl_c(app: &mut App) {
    let mods = Mods { ctrl: true, ..Mods::NONE };
    app.on_event(Event::Key { key: Key::Char('C'), mods, pressed: true });
    app.on_event(Event::Char('\u{3}'));
}

#[test]
fn copy_uses_the_selected_command_output_and_keeps_ctrl_c_out_of_the_shell() {
    let mut app = command_history_app();
    ctrl_c(&mut app);
    let effects = app.take_effects();
    assert!(effects.iter().any(|effect| matches!(effect, Effect::SetClipboard(text) if text == "inside needle one")));
    assert!(effects.iter().all(|effect| !matches!(effect, Effect::Write { .. })));
}

#[test]
fn searching_an_old_command_excludes_text_outside_its_exact_cell_range() {
    let mut app = command_history_app();
    enter(&mut app);
    app.paste("needle".into());
    let reply = request(&mut app).run().unwrap();
    app.session_search_reply(reply);
    let overlay = app.overlay.as_ref().unwrap();
    assert_eq!(overlay.shown.len(), 1);
    assert_eq!(overlay.selected_pick().unwrap().label, "inside needle one");
    ctrl_c(&mut app);
    let effects = app.take_effects();
    assert!(effects.iter().any(|effect| matches!(effect, Effect::SetClipboard(text) if text == "needle")));
    assert!(effects.iter().all(|effect| !matches!(effect, Effect::Write { .. })));
}

#[test]
fn a_session_search_result_activates_its_hidden_workspace() {
    let mut app = app();
    let pane = app.focused_pane().unwrap();
    app.terminal(pane).unwrap().lock().feed(b"needle in the other workspace");
    app.run(Action::NewWorkspace);
    app.paste("Other project".into());
    enter(&mut app);
    app.open_default_tab();
    assert_ne!(app.focused_pane(), Some(pane));
    app.run(Action::SearchSessions);
    app.paste("needle".into());
    let reply = request(&mut app).run().unwrap();
    app.session_search_reply(reply);
    assert_eq!(app.overlay.as_ref().unwrap().shown.len(), 1);
    enter(&mut app);
    assert_eq!(app.focused_pane(), Some(pane));
    assert_eq!(app.terminal(pane).unwrap().lock().selection_text().as_deref(), Some("needle"));
}

#[test]
fn a_hidden_saved_buffer_is_searchable_before_its_first_launch() {
    let mut original = app();
    original.focused_terminal().unwrap().lock().feed(b"recoverable output in the other workspace");
    original.run(Action::NewWorkspace);
    original.paste("New project".into());
    enter(&mut original);
    let saved = original.snapshot();
    let mut restored = App::new(Config::default(), Vec::new(), true, None, Arc::new(|| {}));
    assert!(restored.restore(&saved));
    assert!(restored.focused_pane().is_none());
    restored.run(Action::SearchSessions);
    restored.paste("recoverable".into());
    let reply = request(&mut restored).run().unwrap();
    restored.session_search_reply(reply);
    assert_eq!(restored.overlay.as_ref().unwrap().shown.len(), 1);
    assert!(!restored.take_effects().iter().any(|effect| matches!(effect, Effect::Spawn { .. })));
    enter(&mut restored);
    assert_eq!(restored.focused_terminal().unwrap().lock().selection_text().as_deref(), Some("recoverable"));
}

#[test]
fn quick_select_copies_a_url_split_by_soft_wrap() {
    let mut app = app();
    let terminal = app.focused_terminal().unwrap();
    {
        let mut terminal = terminal.lock();
        terminal.resize(aster_vt::Size::new(18, 5));
        terminal.feed(b"https://example.test/long-path");
    }
    app.run(Action::QuickSelect);
    app.on_event(Event::Char('a'));
    assert!(app.take_effects().iter().any(|effect| matches!(effect, Effect::SetClipboard(text) if text == "https://example.test/long-path")));
}

#[test]
fn quick_open_keeps_the_path_and_editor_position() {
    let mut app = app();
    app.focused_terminal().unwrap().lock().feed(b"src/main.rs:42:7");
    app.run(Action::QuickOpen);
    app.on_event(Event::Char('a'));
    assert!(app.take_effects().iter().any(|effect| matches!(effect, Effect::OpenOutputPath { target, .. } if target == "src/main.rs:42:7")));
}

#[test]
fn quick_open_recognizes_one_quoted_path_with_spaces_and_a_suffix() {
    let mut app = app();
    app.focused_terminal().unwrap().lock().feed(br#""src/with spaces/main.rs":42:7"#);
    app.run(Action::QuickOpen);
    assert_eq!(app.quick_selection.as_ref().unwrap().targets.len(), 1);
    app.on_event(Event::Char('a'));
    assert!(app.take_effects().iter().any(|effect| matches!(effect, Effect::OpenOutputPath { target, .. } if target == "src/with spaces/main.rs:42:7")));
}

#[test]
fn quick_open_keeps_a_seeded_directory_separate_from_an_osc_report() {
    let mut app = app();
    let terminal = app.focused_terminal().unwrap();
    terminal.lock().set_cwd(Some("/initial".to_owned()));
    terminal.lock().feed(b"main.rs:42");
    app.run(Action::QuickOpen);
    app.on_event(Event::Char('a'));
    assert!(app.take_effects().iter().any(|effect| matches!(
        effect,
        Effect::OpenOutputPath { reported_cwd: None, initial_cwd: Some(cwd), .. } if cwd == "/initial"
    )));
    terminal.lock().feed(b"\x1b]7;file:///current\x07");
    app.run(Action::QuickOpen);
    app.on_event(Event::Char('a'));
    assert!(app.take_effects().iter().any(|effect| matches!(
        effect,
        Effect::OpenOutputPath { reported_cwd: Some(cwd), .. } if cwd == "/current"
    )));
}

#[test]
fn quick_select_rejects_a_label_when_the_output_changes_under_it() {
    let mut app = app();
    let terminal = app.focused_terminal().unwrap();
    terminal.lock().feed(b"https://old.example.test");
    app.run(Action::QuickSelect);
    terminal.lock().feed(b"\r\x1b[2Khttps://new.example.test");
    app.on_event(Event::Char('a'));
    assert!(app.take_effects().iter().all(|effect| !matches!(effect, Effect::SetClipboard(_))));
}

fn request(app: &mut App) -> crate::search_sessions::Request {
    app.request_session_search(app.clock() + Duration::from_secs(1));
    app.take_effects().into_iter().find_map(|effect| match effect {
        Effect::SearchSessions(request) => Some(request),
        _ => None,
    }).unwrap()
}

#[test]
fn session_search_finds_a_background_split_and_focuses_it() {
    let mut app = app();
    app.run(Action::SplitRight);
    let pane = app.focused_pane().unwrap();
    app.terminal(pane).unwrap().lock().feed(b"other pane: HELLO from history\r\n");
    app.run(Action::NewTab);
    app.run(Action::SearchSessions);
    app.paste("hello".into());
    let reply = request(&mut app).run().unwrap();
    app.session_search_reply(reply);
    let overlay = app.overlay.as_ref().unwrap();
    assert_eq!(overlay.shown.len(), 1);
    assert!(overlay.selected_pick().unwrap().label.contains("HELLO from history"));
    app.on_event(Event::Key { key: Key::Enter, mods: Mods::NONE, pressed: true });
    assert_eq!(app.focused_pane(), Some(pane));
    assert_eq!(app.terminal(pane).unwrap().lock().selection_text().as_deref(), Some("HELLO"));
}

#[test]
fn session_search_cancels_old_requests_and_restores_focus_on_escape() {
    let mut app = app();
    let pane = app.focused_pane();
    app.run(Action::SearchSessions);
    app.paste("old".into());
    let old = request(&mut app);
    app.paste(" new".into());
    let newest = request(&mut app);
    assert!(old.run().is_none());
    app.on_event(Event::Key { key: Key::Escape, mods: Mods::NONE, pressed: true });
    assert!(newest.run().is_none());
    assert_eq!(app.focused_pane(), pane);
    assert!(app.overlay.is_none());
}

#[test]
fn session_search_highlights_the_full_match_across_a_scan_chunk_and_soft_wrap() {
    let mut app = app();
    let terminal = app.focused_terminal().unwrap();
    {
        let mut terminal = terminal.lock();
        terminal.resize(aster_vt::Size::new(8, 4));
        terminal.feed(b"prefixwrapped-marker\r\n");
        for _ in 0..126 {
            terminal.feed(b"filler\r\n");
        }
    }
    app.run(Action::SearchSessions);
    app.paste("wrapped-marker".into());
    let reply = request(&mut app).run().unwrap();
    app.session_search_reply(reply);
    assert_eq!(app.overlay.as_ref().unwrap().shown.len(), 1);
    app.on_event(Event::Key { key: Key::Enter, mods: Mods::NONE, pressed: true });
    assert_eq!(terminal.lock().selection_text().as_deref(), Some("wrapped-marker"));
}

#[test]
fn session_search_chunk_bounds_are_safe_after_a_pane_shrinks() {
    let app = app();
    let terminal = app.focused_terminal().unwrap();
    let mut terminal = terminal.lock();
    terminal.resize(aster_vt::Size::new(300, 40));
    assert_eq!(terminal.buffer_chunk_start(92, 27, 20), 13);
}

#[test]
fn neither_output_picker_sends_dropped_files_or_motion_to_the_shell() {
    let mut app = app();
    app.focused_terminal().unwrap().lock().feed(b"https://example.test\x1b[?1003h");
    for action in [Action::QuickSelect, Action::SearchSessions] {
        app.run(action);
        app.on_event(Event::MouseMove { x: 15.0, y: 55.0 });
        app.on_event(Event::DroppedFiles(vec!["secret.txt".into()]));
        assert!(app.take_effects().iter().all(|effect| !matches!(effect, Effect::Write { .. })));
        app.on_event(Event::Key { key: Key::Escape, mods: Mods::NONE, pressed: true });
    }
}
