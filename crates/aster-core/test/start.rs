use crate::app::{App, Effect};
use crate::config::Config;
use crate::input::{Event, Key, Mods, MouseButton};
use crate::overlay::OverlayKind;
use crate::profile::Profile;
use aster_gfx::render::Rect;
use std::sync::Arc;

fn start_app() -> App {
    let mut config = Config::default();
    config.terminal.recent_profiles = 5;
    config.hotkeys.insert("tab-2".into(), vec!["Ctrl-J".into()]);
    let profiles = ["a", "b", "c", "d", "e", "f"].map(|name| Profile::new(name, name, "shell", &[]));
    let mut app = App::new(config, profiles.to_vec(), true, None, Arc::new(|| {}));
    app.set_recent(["removed", "f", "e", "d", "c", "b", "a"].map(String::from).to_vec());
    app.open_default_tab();
    app.close_tab(0);
    app
}

fn press(app: &mut App, key: Key, mods: Mods) {
    app.on_event(Event::Key { key, mods, pressed: true });
}

#[test]
fn closing_the_last_tab_leaves_the_default_launch_selected() {
    let mut app = start_app();
    assert!(app.tabs.is_empty());
    assert!(!app.take_effects().iter().any(|e| matches!(e, Effect::Close)));
    press(&mut app, Key::Enter, Mods::NONE);
    app.on_event(Event::Char('\r'));
    assert_eq!(app.tab_title(0), "a");
    assert!(app.panes[&app.tabs[0].focus].queued.is_empty());
}

#[test]
fn start_keys_open_five_existing_recents_then_the_selector_and_settings() {
    let mut app = start_app();
    press(&mut app, Key::Down, Mods::NONE);
    press(&mut app, Key::Enter, Mods::NONE);
    assert_eq!(app.tab_title(0), "f");
    app.close_tab(0);
    for _ in 0..6 {
        press(&mut app, Key::Down, Mods::NONE);
    }
    press(&mut app, Key::Enter, Mods::NONE);
    assert_eq!(app.overlay.as_ref().map(|o| o.kind), Some(OverlayKind::Profiles));
    press(&mut app, Key::Escape, Mods::NONE);
    press(&mut app, Key::Down, Mods::NONE);
    press(&mut app, Key::Enter, Mods::NONE);
    assert!(app.settings.is_some());
}

#[test]
fn a_rebound_numbered_shortcut_opens_the_corresponding_recent_profile() {
    let mut app = start_app();
    press(&mut app, Key::Char('J'), Mods { ctrl: true, ..Mods::NONE });
    assert_eq!(app.tab_title(0), "e");
    assert_eq!(app.recent[0], "e");
}

#[test]
fn shell_exit_discards_terminal_input_state_before_reopening() {
    let mut app = start_app();
    app.open_default_tab();
    let pane = app.tabs[0].focus;
    app.open_search();
    app.exited(pane, Some(0));
    app.on_event(Event::Pasted("one\ntwo".into()));
    assert!(app.search.is_none());
    assert!(app.paste_confirm.is_none());
    assert!(app.session_snapshot().tabs.is_empty());
    assert!(!app.take_effects().iter().any(|e| matches!(e, Effect::Close)));
    press(&mut app, Key::Enter, Mods::NONE);
    assert_eq!(app.tab_title(0), "a");
    assert_ne!(app.tabs[0].focus, pane);
}

#[test]
fn a_click_hits_the_same_centered_row_at_different_window_sizes() {
    let mut app = start_app();
    for (width, height) in [(820.0, 560.0), (220.0, 160.0)] {
        app.chrome.content = Rect::new(0.0, 36.0, width, height);
        let page = app.start_page();
        let first = page.row(0);
        let last = page.row(page.entries.len() - 1);
        assert!((first.x - (width - first.right())).abs() <= 1.0);
        assert!(((first.y - 36.0) - (36.0 + height - last.bottom())).abs() <= 1.0);
        let row = page.row(2);
        let (x, y) = (row.x + row.w / 2.0, row.y + row.h / 2.0);
        assert_eq!(page.hit(x, y), Some(2));
        assert_eq!(page.hit(first.x - 1.0, y), None);
        app.on_event(Event::MouseDown { x, y, button: MouseButton::Left, clicks: 1 });
        assert_eq!(app.tab_title(0), "e");
        app.close_tab(0);
        app.set_recent(["f", "e", "d", "c", "b", "a"].map(String::from).to_vec());
    }
}
