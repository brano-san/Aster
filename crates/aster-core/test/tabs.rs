use super::*;
use crate::config::Config;
use crate::input::{Event, MouseButton};
use crate::keymap::Action;
use crate::profile::Profile;
use std::sync::Arc;

fn app(tabs: usize) -> App {
    let mut a = App::new(Config::default(), vec![Profile::new("local:a", "Shell", "a.exe", &[])], true, None, Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    a.chrome = crate::view::chrome(1000.0, 700.0, 1.0, tabs);
    for _ in 0..tabs {
        a.open_default_tab();
    }
    a.take_effects();
    a
}

fn key(a: &mut App, k: Key, mods: Mods) {
    a.on_event(Event::Key { key: k, mods, pressed: true });
}

fn typed(a: &mut App, text: &str) {
    for c in text.chars() {
        a.on_event(Event::Char(c));
    }
}

#[test]
fn renaming_replaces_the_dynamic_title_until_it_is_cleared() {
    let mut a = app(1);
    a.run(Action::RenameTab);
    assert_eq!(a.renaming().map(|r| r.text.as_str()), Some("Shell"));
    key(&mut a, Key::Backspace, Mods { ctrl: true, ..Mods::NONE });
    typed(&mut a, "build");
    key(&mut a, Key::Enter, Mods::NONE);
    assert_eq!(a.tab_title(0), "build");
    a.run(Action::RenameTab);
    key(&mut a, Key::Backspace, Mods { ctrl: true, ..Mods::NONE });
    key(&mut a, Key::Enter, Mods::NONE);
    assert_eq!(a.tab_title(0), "Shell", "an empty name returns to the shell title");
}

#[test]
fn escape_cancels_a_rename_and_typing_never_reaches_the_shell() {
    let mut a = app(1);
    a.run(Action::RenameTab);
    typed(&mut a, "x");
    key(&mut a, Key::Escape, Mods::NONE);
    assert_eq!(a.tab_title(0), "Shell");
    assert!(a.take_effects().iter().all(|e| !matches!(e, crate::app::Effect::Write { .. })));
}

#[test]
fn double_clicking_a_tab_starts_renaming_and_a_click_elsewhere_commits() {
    let mut a = app(2);
    let r = a.chrome.tabs[1].rect;
    a.on_event(Event::MouseDown { x: r.x + 10.0, y: r.y + 10.0, button: MouseButton::Left, clicks: 1 });
    a.on_event(Event::MouseUp { x: r.x + 10.0, y: r.y + 10.0, button: MouseButton::Left });
    a.on_event(Event::MouseDown { x: r.x + 10.0, y: r.y + 10.0, button: MouseButton::Left, clicks: 2 });
    assert_eq!(a.renaming().map(|r| r.tab), Some(1));
    typed(&mut a, "!");
    a.on_event(Event::MouseDown { x: 500.0, y: 400.0, button: MouseButton::Left, clicks: 1 });
    assert!(a.renaming().is_none());
    assert_eq!(a.tab_title(1), "Shell!");
}

#[test]
fn closing_tabs_around_the_active_one() {
    let mut a = app(5);
    a.activate(2);
    a.run(Action::CloseTabsToRight);
    assert_eq!((a.tab_count(), a.active), (3, 2));
    a.run(Action::CloseTabsToLeft);
    assert_eq!((a.tab_count(), a.active), (1, 0));
    let mut b = app(4);
    b.activate(1);
    b.run(Action::RenameTab);
    typed(&mut b, " kept");
    key(&mut b, Key::Enter, Mods::NONE);
    b.run(Action::CloseOtherTabs);
    assert_eq!((b.tab_count(), b.tab_title(0).as_str()), (1, "Shell kept"));
}

#[test]
fn custom_titles_survive_a_session_round_trip() {
    let mut a = app(1);
    a.run(Action::RenameTab);
    typed(&mut a, "!");
    key(&mut a, Key::Enter, Mods::NONE);
    let state = a.snapshot();
    let mut b = App::new(Config::default(), vec![Profile::new("local:a", "Shell", "a.exe", &[])], true, None, Arc::new(|| {}));
    assert!(b.restore(&state));
    assert_eq!(b.tab_title(0), "Shell!");
}
