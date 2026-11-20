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


#[test]
fn administrator_tabs_spawn_elevated_and_say_so_in_the_title() {
    let mut a = app(1);
    a.set_can_elevate(true);
    a.run(Action::DuplicateAsAdmin);
    assert_eq!(a.tab_count(), 2);
    assert_eq!(a.tab_title(1), "Administrator: Shell");
    let pane = a.focused_pane().unwrap();
    let launch = a.panes[&pane].pending.clone().expect("a pending launch");
    assert!(launch.elevated);
    let state = a.snapshot();
    let mut b = App::new(Config::default(), vec![Profile::new("local:a", "Shell", "a.exe", &[])], true, None, Arc::new(|| {}));
    assert!(b.restore(&state));
    assert_eq!(b.tab_title(1), "Shell", "a restored tab does not ask for elevation again");
}

#[test]
fn administrator_tabs_need_support_and_a_local_shell() {
    let mut a = app(1);
    a.run(Action::NewAdminTab);
    assert_eq!(a.tab_count(), 1, "nothing happens where elevation is unavailable");
    let mut ssh = Profile::new("ssh:box", "SSH: box", "ssh", &["box"]);
    ssh.ssh = Some("box".into());
    let mut b = App::new(Config::default(), vec![ssh], true, None, Arc::new(|| {}));
    b.set_can_elevate(true);
    b.open_default_tab();
    b.run(Action::DuplicateAsAdmin);
    assert_eq!(b.tab_count(), 1, "remote tabs are not elevated");
}

fn alt() -> Mods {
    Mods { alt: true, ..Mods::NONE }
}

#[test]
fn the_last_used_tab_toggles_back_and_forth() {
    let mut a = app(4);
    key(&mut a, Key::Char('1'), alt());
    key(&mut a, Key::Char('3'), alt());
    a.on_event(Event::Timer);
    a.run(Action::ToggleLastTab);
    a.on_event(Event::Timer);
    assert_eq!(a.active, 0);
    a.run(Action::ToggleLastTab);
    a.on_event(Event::Timer);
    assert_eq!(a.active, 2);
    key(&mut a, Key::Char('W'), Mods { ctrl: true, shift: true, ..Mods::NONE });
    a.run(Action::ToggleLastTab);
    assert_eq!(a.active, 0, "the last tab survives closing the current one");
}

#[test]
fn closed_tabs_reopen_with_their_title_and_layout_in_reverse_order() {
    let mut a = app(2);
    a.tabs[1].title = Some("logs".into());
    a.run(Action::SplitRight);
    a.panes.values().for_each(|p| p.terminal.lock().feed(b"tail line"));
    a.run(Action::CloseTab);
    a.run(Action::CloseTab);
    assert_eq!(a.tabs.len(), 0);
    assert_eq!(a.closed_tabs.len(), 2);
    let mut a = app(2);
    a.tabs[1].title = Some("logs".into());
    a.run(Action::SplitRight);
    a.panes.values().for_each(|p| p.terminal.lock().feed(b"tail line"));
    a.run(Action::CloseTab);
    a.take_effects();
    assert_eq!(a.tabs.len(), 1);
    a.run(Action::ReopenTab);
    assert_eq!(a.tabs.len(), 2);
    assert_eq!(a.active, 1);
    assert_eq!(a.tab_title(1), "logs");
    assert_eq!(a.tabs[1].root.panes().len(), 2);
    let text = a.focused_terminal().unwrap().lock().tail_lines(10).join("\n");
    assert!(text.contains("tail line"));
    assert_eq!(a.take_effects().iter().filter(|e| matches!(e, Effect::Spawn { .. })).count(), 0, "spawns wait for the next frame");
    a.run(Action::ReopenTab);
    assert_eq!(a.tabs.len(), 2, "nothing left to reopen");
}

#[test]
fn panes_explode_into_tabs_and_tabs_combine_into_one_without_restarting() {
    let mut a = app(2);
    a.run(Action::SplitRight);
    a.run(Action::SplitBottom);
    let panes = a.tabs[1].root.panes();
    let focus = a.tabs[1].focus;
    a.take_effects();
    a.run(Action::ExplodeTab);
    assert_eq!(a.tabs.len(), 4);
    assert_eq!(a.tabs[1..].iter().map(|t| t.focus).collect::<Vec<_>>(), panes);
    assert_eq!(a.tabs[a.active].focus, focus);
    a.run(Action::CombineTabs);
    assert_eq!(a.tabs.len(), 1);
    assert_eq!(a.tabs[0].root.panes().len(), 4);
    assert_eq!(a.tabs[0].focus, focus);
    assert!(a.take_effects().iter().all(|e| !matches!(e, Effect::Kill { .. } | Effect::Spawn { .. })));
    let widths: Vec<f32> = a.tabs[0].root.layout(aster_gfx::render::Rect::new(0.0, 0.0, 403.0, 100.0), 1.0).panes.iter().map(|(_, r)| r.w).collect();
    assert_eq!(widths, [100.0, 100.0, 100.0, 100.0]);
}

#[test]
fn keyboard_resizing_moves_the_nearest_divider() {
    let mut a = app(1);
    a.run(Action::SplitRight);
    a.run(Action::PaneResizeLeft);
    assert!(matches!(a.tabs[0].root, crate::panes::Node::Split { ratio, .. } if (ratio - 0.4).abs() < 1e-6));
    a.run(Action::PaneResizeDown);
    assert!(matches!(a.tabs[0].root, crate::panes::Node::Split { ratio, .. } if (ratio - 0.4).abs() < 1e-6), "no vertical divider to move");
}

#[test]
fn tab_colors_come_from_the_picker_or_the_profile_and_survive_the_session() {
    let mut a = app(2);
    assert_eq!(a.tab_color(1), None);
    a.run(Action::SetTabColor);
    typed(&mut a, "green");
    key(&mut a, Key::Enter, Mods::NONE);
    assert_eq!(a.tabs[1].color.as_deref(), Some("#57AB5A"));
    assert!(a.tab_color(1).is_some());
    let state = a.snapshot();
    assert_eq!(state.tabs[1].color.as_deref(), Some("#57AB5A"));
    a.run(Action::SetTabColor);
    typed(&mut a, "no color");
    key(&mut a, Key::Enter, Mods::NONE);
    assert_eq!(a.tabs[1].color, None);
    let pane = a.tabs[0].focus;
    a.panes.get_mut(&pane).unwrap().profile.color = Some("#4C8DF6".into());
    assert!(a.tab_color(0).is_some());
}

#[test]
fn command_tabs_stay_after_success_and_are_not_restored() {
    let mut a = app(1);
    a.run_command(&["make".to_owned(), "-j".to_owned(), "8".to_owned()], Some("/src".into()));
    assert_eq!(a.tabs.len(), 2);
    let pane = a.tabs[1].focus;
    assert_eq!((a.panes[&pane].profile.command.as_str(), a.panes[&pane].profile.args.clone()), ("make", vec!["-j".to_owned(), "8".to_owned()]));
    a.exited(pane, Some(0));
    assert_eq!(a.tabs.len(), 2, "a finished command keeps its output");
    assert_eq!(a.snapshot().tabs.len(), 1, "command tabs never run again from a saved session");
    assert_eq!(a.find_profile("shell").map(|p| p.id), Some("local:a".to_owned()));
    assert_eq!(a.find_profile("local:a").map(|p| p.name), Some("Shell".to_owned()));
    assert!(a.find_profile("nope").is_none());
}

#[test]
fn the_tab_bar_hides_by_setting_or_shortcut_and_tab_switches_show_their_name() {
    let mut a = app(1);
    assert!(!a.tab_bar_hidden());
    let mut config = Config::default();
    config.appearance.tab_bar = crate::config::TabBar::Auto;
    a.set_config(config.clone());
    assert!(a.tab_bar_hidden(), "one tab hides the bar in auto mode");
    a.open_default_tab();
    assert!(!a.tab_bar_hidden());
    config.appearance.tab_bar = crate::config::TabBar::Never;
    a.set_config(config);
    assert!(a.tab_bar_hidden());
    key(&mut a, Key::Char('1'), Mods { alt: true, ..Mods::NONE });
    key(&mut a, Key::Char('2'), Mods { alt: true, ..Mods::NONE });
    assert!(a.tab_toast.as_ref().is_some_and(|(text, _)| text.starts_with("2 / 2")));
    assert_eq!(a.hit_test(400.0, 2.0), crate::input::HitArea::Client);
    key(&mut a, Key::Char('B'), Mods { ctrl: true, shift: true, ..Mods::NONE });
    assert!(!a.tab_bar_hidden(), "the shortcut overrides the setting");
    let c = crate::view::bare_chrome(800.0, 600.0);
    assert_eq!((c.content.y, c.content.h, c.tabs.len()), (0.0, 600.0, 0));
}

#[test]
fn settings_live_in_their_own_tab_and_come_back_where_they_were() {
    let mut a = app(1);
    key(&mut a, Key::Char(','), Mods { ctrl: true, ..Mods::NONE });
    assert_eq!(a.tabs.len(), 2);
    assert_eq!(a.tab_title(1), "Settings");
    assert!(a.settings.is_some());
    key(&mut a, Key::PageDown, Mods { ctrl: true, ..Mods::NONE });
    typed(&mut a, "x");
    key(&mut a, Key::Char('1'), Mods { alt: true, ..Mods::NONE });
    assert_eq!(a.active, 0, "tab shortcuts work beside the settings");
    assert!(a.settings.is_none(), "the terminal tab gets the keys again");
    key(&mut a, Key::Char('2'), Mods { alt: true, ..Mods::NONE });
    let page = a.settings.as_ref().expect("settings come back");
    assert_eq!((page.page, page.query.as_str()), (1, "x"));
    key(&mut a, Key::Char(','), Mods { ctrl: true, ..Mods::NONE });
    assert_eq!(a.tabs.len(), 1, "the shortcut closes the settings tab");
    assert!(a.settings.is_none());
    a.run(Action::Settings);
    a.run(Action::SplitRight);
    a.run(Action::CombineTabs);
    assert_eq!(a.tabs.len(), 2, "the settings tab is never split or combined");
    assert_eq!(a.snapshot().tabs.len(), 1, "the settings tab is not saved in the session");
}
