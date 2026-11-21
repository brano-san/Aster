use super::*;
use crate::app::Effect;
use crate::config::{Config, RightClick};
use crate::input::{Event, Mods, MouseButton};
use crate::profile::Profile;
use std::sync::Arc;

fn app(tabs: usize) -> App {
    let mut a = App::new(Config::default(), vec![Profile::new("local:a", "A", "a.exe", &[])], true, None, Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    for _ in 0..tabs {
        a.open_default_tab();
    }
    a.chrome = a.tab_chrome();
    a.take_effects();
    a
}

fn key(a: &mut App, k: Key) {
    a.on_event(Event::Key { key: k, mods: Mods::NONE, pressed: true });
}

fn right_click(a: &mut App, x: f32, y: f32) {
    a.on_event(Event::MouseDown { x, y, button: MouseButton::Right, clicks: 1 });
}

fn labels(a: &App) -> Vec<(&'static str, bool)> {
    a.menu.as_ref().unwrap().entries.iter().filter_map(|e| if let Entry::Item { label, enabled, .. } = e { Some((*label, *enabled)) } else { None }).collect()
}

#[test]
fn right_clicking_a_tab_activates_it_and_offers_only_possible_actions() {
    let mut a = app(3);
    let r = a.chrome.tabs[0].rect;
    right_click(&mut a, r.x + 10.0, r.y + 10.0);
    assert_eq!(a.active, 0);
    let items = labels(&a);
    assert!(items.contains(&("Move left", false)));
    assert!(items.contains(&("Close tabs to the left", false)));
    assert!(items.contains(&("Close tabs to the right", true)));
}

#[test]
fn keyboard_navigation_skips_separators_and_disabled_items() {
    let mut a = app(1);
    let r = a.chrome.tabs[0].rect;
    right_click(&mut a, r.x + 10.0, r.y + 10.0);
    assert!(!labels(&a).iter().any(|(label, _)| *label == "Save the layout as a profile"));
    for action in [Action::NewTab, Action::DuplicateTab, Action::RenameTab, Action::PinTab, Action::SetTabColor, Action::SaveAsProfile, Action::CloseTab] {
        key(&mut a, Key::Down);
        let m = a.menu.as_ref().unwrap();
        assert_eq!(m.action(m.selected.unwrap()), Some(action));
    }
    key(&mut a, Key::Up);
    let m = a.menu.as_ref().unwrap();
    assert_eq!(m.action(m.selected.unwrap()), Some(Action::SaveAsProfile));
    key(&mut a, Key::Escape);
    assert!(!a.menu_open());
}

#[test]
fn choosing_an_item_runs_its_action_and_closes_the_menu() {
    let mut a = app(3);
    let r = a.chrome.tabs[1].rect;
    right_click(&mut a, r.x + 10.0, r.y + 10.0);
    let geo = a.menu_geometry().unwrap();
    let (_, row) = geo.rows.iter().find(|(i, _)| a.menu.as_ref().unwrap().action(*i) == Some(Action::CloseOtherTabs)).copied().unwrap();
    a.on_event(Event::MouseDown { x: row.x + 5.0, y: row.y + 5.0, button: MouseButton::Left, clicks: 1 });
    assert!(!a.menu_open());
    assert_eq!(a.tab_count(), 1);
}

#[test]
fn a_click_outside_only_closes_the_menu() {
    let mut a = app(2);
    let r = a.chrome.tabs[0].rect;
    right_click(&mut a, r.x + 10.0, r.y + 10.0);
    a.on_event(Event::MouseDown { x: 900.0, y: 650.0, button: MouseButton::Left, clicks: 1 });
    assert!(!a.menu_open());
    assert_eq!(a.tab_count(), 2);
}

#[test]
fn the_pin_menu_reflects_state_and_skips_protected_close_targets() {
    let mut a = app(3);
    a.open_tab_menu(2, 20.0, 20.0);
    assert!(labels(&a).contains(&("Pin tab", true)));
    a.run(Action::PinTab);
    a.open_tab_menu(0, 20.0, 20.0);
    assert!(labels(&a).contains(&("Unpin tab", true)));
    assert!(labels(&a).contains(&("Move right", false)));
    a.open_tab_menu(1, 20.0, 20.0);
    assert!(labels(&a).contains(&("Close tabs to the left", false)));
}

#[test]
fn the_menu_stays_inside_the_window() {
    let menu = Menu { entries: vec![Entry::Separator; 3], at: (990.0, 690.0), selected: None };
    let geo = geometry(&menu, 1.0, (1000.0, 700.0));
    assert!(geo.frame.right() <= 1000.0 && geo.frame.bottom() <= 700.0);
}

#[test]
fn a_sidebar_menu_fits_a_short_window_and_owns_caption_clicks() {
    use crate::input::HitArea;
    let mut a = app(1);
    a.config.appearance.tab_bar_position = crate::config::TabBarPosition::Right;
    a.set_viewport(720, 350, 1.0);
    a.chrome = a.tab_chrome();
    let r = a.chrome.tabs[0].rect;
    right_click(&mut a, r.right() - 2.0, r.y + 12.0);
    let geo = a.menu_geometry().unwrap();
    assert!(geo.frame.right() <= 720.0 && geo.frame.bottom() <= 350.0);
    assert!(geo.rows.iter().all(|(_, row)| row.h >= 16.0 && row.bottom() <= geo.frame.bottom()));
    let (_, first) = geo.rows[0];
    let (x, y) = (first.right() - 4.0, first.y + first.h / 2.0);
    assert!(a.chrome.close.contains(x, y));
    assert_eq!(a.hit_test(x, y), HitArea::Client);
    a.on_event(Event::MouseDown { x, y, button: MouseButton::Left, clicks: 1 });
    assert_eq!(a.tab_count(), 2);
}

#[test]
fn the_terminal_menu_appears_when_right_click_is_set_to_menu() {
    let mut a = app(1);
    let mut config = Config::default();
    config.terminal.right_click = RightClick::Menu;
    a.set_config(config);
    let pane = a.focused_pane().unwrap();
    a.layout = crate::panes::Layout { panes: vec![(pane, a.chrome.content)], splitters: Vec::new() };
    right_click(&mut a, 500.0, 400.0);
    assert_eq!(labels(&a)[0], ("Copy", false));
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::RequestPaste)));
}

#[test]
fn command_notification_menu_follows_the_tab_shell_and_watch_state() {
    let mut a = app(1);
    a.open_tab_menu(0, 25.0, 20.0);
    assert!(labels(&a).contains(&("Notify when the command finishes", false)));
    a.set_pane_pid(a.focused_pane().unwrap(), 56);
    a.open_tab_menu(0, 25.0, 20.0);
    assert!(labels(&a).contains(&("Notify when the command finishes", true)));
    let i = a.menu.as_ref().unwrap().entries.iter().position(|e| matches!(e, Entry::Item { action: Action::NotifyCommand, .. })).unwrap();
    a.run_menu(i);
    a.open_tab_menu(0, 25.0, 20.0);
    assert!(labels(&a).contains(&("Cancel command notification", true)));
}

#[test]
fn the_terminal_menu_offers_line_controls_only_for_serial_panes() {
    let mut a = app(1);
    a.open_terminal_menu(200.0, 150.0);
    let actions = ["Change the serial port speed", "Send a break on the serial line"];
    assert!(labels(&a).iter().all(|(label, _)| !actions.contains(label)));
    let mut profile = Profile::new("serial:COM4", "COM4", "", &[]);
    profile.serial = Some("COM4:9600".into());
    a.open_tab(profile);
    let pane = a.focused_pane().unwrap();
    a.panes.get_mut(&pane).unwrap().pending = None;
    a.open_terminal_menu(200.0, 150.0);
    let items = labels(&a);
    assert!(actions.iter().all(|label| items.contains(&(*label, true))));
}
