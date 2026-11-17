use super::*;
use crate::config::Config;
use crate::input::{Event, Key, Mods};
use crate::keymap::Action;
use crate::profile::Profile;
use std::sync::Arc;

fn app(tabs: usize) -> App {
    let mut a = App::new(Config::default(), vec![Profile::new("local:a", "Shell", "a.exe", &[])], true, None, Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    for _ in 0..tabs {
        a.open_default_tab();
    }
    a.set_children_probe(Arc::new(|pid| if pid == 7 { vec!["ping.exe".to_owned(), "vim.exe".to_owned()] } else { Vec::new() }));
    a.take_effects();
    a
}

fn busy(a: &mut App, tab: usize) {
    let pane = a.tabs[tab].focus;
    a.set_pane_pid(pane, 7);
}

fn key(a: &mut App, k: Key) {
    a.on_event(Event::Key { key: k, mods: Mods::NONE, pressed: true });
}

#[test]
fn idle_tabs_close_at_once_and_busy_ones_ask_first() {
    let mut a = app(2);
    a.run(Action::CloseTab);
    assert_eq!(a.tabs.len(), 1);
    assert!(a.close_confirm().is_none());
    busy(&mut a, 0);
    a.run(Action::CloseTab);
    assert_eq!(a.tabs.len(), 1);
    assert_eq!(a.close_confirm().map(|c| c.programs.clone()), Some(vec!["ping.exe".to_owned(), "vim.exe".to_owned()]));
    key(&mut a, Key::Escape);
    assert!(a.close_confirm().is_none());
    assert_eq!(a.tabs.len(), 1);
    a.run(Action::CloseTab);
    key(&mut a, Key::Enter);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Close)));
}

#[test]
fn closing_other_tabs_asks_once_for_all_of_them() {
    let mut a = app(4);
    busy(&mut a, 0);
    busy(&mut a, 2);
    a.activate(3);
    a.run(Action::CloseOtherTabs);
    assert_eq!(a.tabs.len(), 4);
    assert_eq!(a.close_confirm().map(|c| c.target.clone()).map(|t| matches!(t, CloseTarget::Tabs(r) if r.len() == 3)), Some(true));
    let geo = a.close_geometry().unwrap();
    let (x, y) = (geo.buttons[0].x + 4.0, geo.buttons[0].y + 4.0);
    a.on_event(Event::MouseDown { x, y, button: crate::input::MouseButton::Left, clicks: 1 });
    assert_eq!(a.tabs.len(), 1);
}

#[test]
fn the_window_close_button_asks_and_the_setting_turns_it_off() {
    let mut a = app(1);
    busy(&mut a, 0);
    a.on_event(Event::CloseRequested);
    assert!(a.close_confirm().is_some());
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::Close)));
    key(&mut a, Key::Escape);
    let mut config = Config::default();
    config.terminal.confirm_close = false;
    a.set_config(config);
    a.on_event(Event::CloseRequested);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Close)));
}

#[test]
fn ending_the_session_never_asks() {
    let mut a = app(1);
    busy(&mut a, 0);
    a.on_event(Event::SessionEnding);
    assert!(a.close_confirm().is_none());
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Close)));
}

#[test]
fn a_confirmed_window_close_keeps_the_tabs_for_the_session_file() {
    let mut a = app(2);
    busy(&mut a, 1);
    a.on_event(Event::CloseRequested);
    assert_eq!(a.session_snapshot().tabs.len(), 2);
    key(&mut a, Key::Enter);
    assert!(a.tabs.is_empty());
    assert_eq!(a.session_snapshot().tabs.len(), 2);
    let mut b = app(1);
    b.run(Action::CloseTab);
    assert!(b.session_snapshot().tabs.is_empty(), "closing the last tab leaves an empty session");
}
