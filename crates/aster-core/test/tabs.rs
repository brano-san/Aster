use super::*;
use crate::config::Config;
use crate::input::{Event, MouseButton};
use crate::keymap::Action;
use crate::profile::Profile;
use std::sync::Arc;

fn app(tabs: usize) -> App {
    let mut a = App::new(Config::default(), vec![Profile::new("local:a", "Shell", "a.exe", &[])], true, None, Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    for _ in 0..tabs {
        a.open_default_tab();
    }
    a.chrome = a.tab_chrome();
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
fn saving_the_focused_profile_persists_defaults_and_selects_the_copy() {
    let mut a = app(0);
    a.set_config(Config::parse("[profile-defaults.local.env]\nWORK = 'test'").unwrap());
    a.open_default_tab();
    a.tabs[0].title = Some("Build".into());
    a.focused_terminal().unwrap().lock().set_cwd(Some(r"D:\src".into()));
    a.run(Action::SaveAsProfile);
    let saved = a.config.profiles.last().unwrap();
    assert_eq!((saved.name.as_str(), saved.cwd.as_deref(), saved.env["WORK"].as_str()), ("Build (saved)", Some(r"D:\src"), "test"));
    assert!(!saved.builtin && !saved.elevated && !saved.hold);
    let page = a.settings.as_ref().unwrap();
    assert_eq!(page.profile_at(page.shown[page.selected]).unwrap().id, saved.id);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::SaveConfig(text) if Config::parse(text).unwrap().profiles.len() == 1)));
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

#[test]
fn pinning_preserves_order_focus_and_the_window_title() {
    let mut a = app(4);
    for (i, title) in ["A", "B", "C", "D"].into_iter().enumerate() {
        a.tabs[i].title = Some(title.to_owned());
    }
    a.run(Action::PinTab);
    a.activate(2);
    a.run(Action::PinTab);
    assert_eq!((a.active, a.tab_title(0), a.tab_title(1)), (1, "D".into(), "B".into()));
    a.run(Action::MoveTabRight);
    assert_eq!(a.active, 1);
    a.mark_drawn(a.clock());
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Title(title) if title == "B")));
    a.activate(0);
    a.open_default_tab();
    assert_eq!(a.active, 2);
    a.activate(0);
    a.run(Action::PinTab);
    assert_eq!((a.active, a.tab_title(0), a.tab_title(1)), (1, "B".into(), "D".into()));
    assert!(a.tabs[0].pinned && !a.tabs[1].pinned);
}

#[test]
fn pinned_tabs_survive_group_closes_and_ask_before_individual_close() {
    for action in [Action::CloseTabsToLeft, Action::CloseTabsToRight, Action::CloseOtherTabs] {
        let mut a = app(4);
        a.run(Action::PinTab);
        let id = a.tabs[0].focus;
        a.activate(2);
        a.run(action);
        assert_eq!(a.tabs[0].focus, id);
        assert!(a.close_confirm().is_none());
        a.activate(0);
        key(&mut a, Key::Char('W'), Mods { ctrl: true, shift: true, ..Mods::NONE });
        assert!(a.close_confirm().is_some());
        key(&mut a, Key::Escape, Mods::NONE);
        assert_eq!(a.tabs[0].focus, id);
        a.run(Action::CloseTab);
        key(&mut a, Key::Enter, Mods::NONE);
        assert!(a.tabs.iter().all(|t| t.focus != id));
        a.run(Action::ReopenTab);
        assert!(a.tabs[0].pinned);
        assert_eq!(a.active, 0);
    }
}

#[test]
fn pinned_tabs_round_trip_and_older_sessions_default_to_unpinned() {
    let mut a = app(3);
    a.run(Action::PinTab);
    a.activate(1);
    let json = serde_json::to_value(a.snapshot()).unwrap();
    assert_eq!(json["tabs"][0]["pinned"], true);
    let mut legacy = json.clone();
    for tab in legacy["tabs"].as_array_mut().unwrap() {
        tab.as_object_mut().unwrap().remove("pinned");
    }
    for (value, pinned) in [(json, true), (legacy, false)] {
        let mut b = app(0);
        assert!(b.restore(&serde_json::from_value(value).unwrap()));
        assert_eq!((b.tabs[0].pinned, b.active), (pinned, 1));
    }
}

#[test]
fn fullscreen_visibility_respects_the_setting_and_restores_the_bar() {
    let mut a = app(2);
    a.set_fullscreen(true);
    assert!(a.tab_bar_hidden());
    a.run(Action::ToggleTabBar);
    assert!(a.tab_bar_hidden());
    a.set_fullscreen(false);
    assert!(!a.tab_bar_hidden());
    let mut config = a.config().clone();
    config.appearance.tabs_in_fullscreen = true;
    a.set_config(config.clone());
    a.set_fullscreen(true);
    assert!(!a.tab_bar_hidden());
    config.appearance.tab_bar = crate::config::TabBar::Never;
    a.set_config(config);
    assert!(a.tab_bar_hidden());
}

#[test]
fn closing_the_last_tab_can_keep_its_window_for_another_profile() {
    let mut a = app(1);
    let old = a.tabs[0].focus;
    let mut config = a.config().clone();
    config.terminal.close_window_with_last_tab = false;
    a.set_config(config);
    a.run(Action::CloseTab);
    assert!(a.tabs.is_empty());
    assert!(a.overlay.is_none(), "the start page shows instead of a picker");
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::Close)));
    key(&mut a, Key::Enter, Mods::NONE);
    assert_eq!(a.tabs.len(), 1);
    assert_ne!(a.tabs[0].focus, old);
    a.set_config(Config::default());
    a.run(Action::CloseTab);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Close)));
}

fn alt() -> Mods {
    Mods { alt: true, ..Mods::NONE }
}

fn encloses(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x && inner.y >= outer.y && inner.right() <= outer.right() && inner.bottom() <= outer.bottom()
}

#[test]
fn each_bar_position_separates_panes_tabs_and_caption_buttons() {
    use crate::input::HitArea;
    use TabBarPosition::*;
    let mut a = app(3);
    for position in [Top, Bottom, Left, Right] {
        a.config.appearance.tab_bar_position = position;
        a.set_viewport(1100, 700, 1.25);
        a.chrome = a.tab_chrome();
        let c = &a.chrome;
        let viewport = Rect::new(0.0, 0.0, 1100.0, 700.0);
        let center = |r: Rect| (r.x + r.w / 2.0, r.y + r.h / 2.0);
        assert!(encloses(viewport, c.content) && encloses(viewport, c.tab_bar));
        match position {
            Top => assert_eq!(c.tab_bar.bottom(), c.content.y),
            Bottom => assert_eq!(c.content.bottom(), c.tab_bar.y),
            Left => assert_eq!((c.tab_bar.w, c.tab_bar.right()), (275.0, c.content.x)),
            Right => assert_eq!((c.tab_bar.w, c.tab_bar.x), (275.0, c.content.right())),
        }
        if position != Top {
            assert_eq!(c.caption.h, 40.0);
            assert_eq!(c.content.y, c.caption.bottom());
        }
        let (x, y) = if position == Top { (c.plus.right() + 1.0, 4.0) } else { center(c.caption) };
        assert_eq!(a.hit_test(x, y), HitArea::Caption);
        for (r, area) in [(c.min, HitArea::Minimize), (c.max, HitArea::Maximize), (c.close, HitArea::Close)] {
            assert!(encloses(c.caption, r));
            let (x, y) = center(r);
            assert_eq!(a.hit_test(x, y), area);
        }
        assert!(encloses(c.tab_bar, c.plus));
        assert!(encloses(c.tab_bar, c.tab_menu));
        assert!(c.settings.w > 0.0 && c.settings.h > 0.0 && encloses(c.tab_bar, c.settings));
        let (x, y) = center(c.tab_menu);
        assert_eq!(a.hit_test(x, y), HitArea::Client);
        assert!(if c.vertical { c.plus.bottom() <= c.tab_menu.y } else { c.plus.right() <= c.tab_menu.x });
        let (x, y) = center(c.settings);
        assert_eq!(a.hit_test(x, y), HitArea::Client);
        assert!(if c.vertical { c.tab_menu.bottom() <= c.settings.y } else { c.tab_menu.right() <= c.settings.x });
        for slot in &c.tabs {
            assert!(slot.rect.w > 0.0 && encloses(c.tab_bar, slot.rect) && encloses(slot.rect, slot.close));
            let (x, y) = center(slot.rect);
            assert_eq!(a.hit_test(x, y), HitArea::Client);
        }
        let last = c.tabs.last().unwrap().rect;
        assert!(if c.vertical { last.bottom() <= c.plus.y } else { last.right() <= c.plus.x });
    }
}

#[test]
fn fixed_width_shrinks_to_fit_and_pins_keep_their_compact_slot() {
    let mut a = app(3);
    a.tabs[0].pinned = true;
    let fit = a.tab_chrome();
    assert_eq!(fit.tabs[1].rect.w, 220.0);
    a.config.appearance.tab_width = TabWidth::Fixed;
    let fixed = a.tab_chrome();
    assert_eq!((fixed.tabs[0].rect.w, fixed.tabs[1].rect.w, fixed.tabs[2].rect.w), (48.0, 200.0, 200.0));
    assert_eq!(fixed.tabs[0].close.w, 0.0);
    a.set_viewport(480, 350, 1.0);
    let narrow = a.tab_chrome();
    assert_eq!(narrow.tabs[0].rect.w, 48.0);
    assert!((84.0..200.0).contains(&narrow.tabs[1].rect.w));
    assert_eq!(narrow.tabs[1].rect.w, narrow.tabs[2].rect.w);
    for _ in 0..12 {
        a.open_default_tab();
    }
    let many = a.tab_chrome();
    assert_eq!(many.tabs[0].rect.w, 48.0);
    assert_eq!(many.tabs[a.active].rect.w, 84.0);
    assert!(many.tab_menu.right() <= many.settings.x && many.settings.right() <= many.min.x);
}

#[test]
fn disabled_close_buttons_leave_the_slot_clickable_and_middle_click_still_closes() {
    let mut a = app(2);
    let old = a.chrome.tabs[0].close;
    a.config.appearance.tab_close_buttons = false;
    a.chrome = a.tab_chrome();
    assert!(a.chrome.tabs.iter().all(|slot| slot.close.w == 0.0));
    let (x, y) = (old.x + 1.0, old.y + 1.0);
    a.on_event(Event::MouseDown { x, y, button: MouseButton::Left, clicks: 1 });
    a.on_event(Event::MouseUp { x, y, button: MouseButton::Left });
    assert_eq!((a.active, a.tabs.len()), (0, 2));
    a.on_event(Event::MouseDown { x, y, button: MouseButton::Middle, clicks: 1 });
    assert_eq!(a.tabs.len(), 1);
}

#[test]
fn tab_menu_button_opens_the_active_tab_menu_and_can_be_hidden() {
    let mut a = app(2);
    let menu = a.chrome.tab_menu;
    assert!(menu.w > 0.0 && a.chrome.plus.right() <= menu.x && menu.right() <= a.chrome.min.x);
    let (x, y) = (menu.x + menu.w / 2.0, menu.y + menu.h / 2.0);
    assert_eq!(a.hit_test(x, y), crate::input::HitArea::Client);
    a.on_event(Event::MouseDown { x, y, button: MouseButton::Left, clicks: 1 });
    a.on_event(Event::MouseUp { x, y, button: MouseButton::Left });
    assert!(a.menu_open());
    assert_eq!(a.active, 1);
    a.close_menu();
    a.config.appearance.tab_menu_button = false;
    a.chrome = a.tab_chrome();
    assert_eq!(a.chrome.tab_menu, Rect::default());
    let tab = a.chrome.tabs[0].rect;
    a.on_event(Event::MouseDown { x: tab.x + 1.0, y: tab.y + 1.0, button: MouseButton::Right, clicks: 1 });
    assert!(a.menu_open());
    assert_eq!(a.active, 0);
}

#[test]
fn clicking_the_settings_button_opens_and_reuses_the_settings_tab() {
    let mut a = app(1);
    a.config.appearance.tab_menu_button = false;
    a.chrome = a.tab_chrome();
    let x = a.chrome.min.x - 26.0;
    let y = a.chrome.caption.h / 2.0;
    let click = |a: &mut App| {
        assert_eq!(a.hit_test(x, y), crate::input::HitArea::Client);
        a.on_event(Event::MouseDown { x, y, button: MouseButton::Left, clicks: 1 });
        a.on_event(Event::MouseUp { x, y, button: MouseButton::Left });
    };
    click(&mut a);
    assert!(a.settings.is_some());
    assert_eq!(a.tab_count(), 2);
    a.activate(0);
    a.chrome = a.tab_chrome();
    click(&mut a);
    assert!(a.settings.is_some());
    assert_eq!(a.tab_count(), 2);
}

#[test]
fn numbered_and_pinned_tab_labels_preserve_the_full_window_title() {
    let mut a = app(2);
    a.tabs[1].title = Some("Build output".into());
    assert_eq!(a.tab_label(1), "Build output");
    a.config.appearance.tab_index = true;
    assert_eq!(a.tab_label(1), "2  Build output");
    a.toggle_pin();
    assert_eq!(a.tab_label(0), "1");
    a.config.appearance.tab_index = false;
    assert_eq!(a.tab_label(0), "");
    a.config.appearance.tab_icon = false;
    assert_eq!(a.tab_label(0), "Build output");
    a.mark_drawn(a.clock());
    assert!(a.take_effects().iter().any(|effect| matches!(effect, Effect::Title(text) if text == "Build output")));
}

#[test]
fn dragging_sidebar_tabs_uses_the_vertical_axis_and_respects_pins() {
    let mut a = app(3);
    a.config.appearance.tab_bar_position = TabBarPosition::Right;
    a.tabs[0].pinned = true;
    a.chrome = a.tab_chrome();
    let tab = a.tabs[2].focus;
    let (last, before, pin) = (a.chrome.tabs[2].rect, a.chrome.tabs[1].rect, a.chrome.tabs[0].rect);
    let x = last.x + 12.0;
    a.on_event(Event::MouseDown { x, y: last.y + 10.0, button: MouseButton::Left, clicks: 1 });
    a.on_event(Event::MouseMove { x: x + 30.0, y: last.y + 10.0 });
    assert_eq!(a.active, 2);
    a.on_event(Event::MouseMove { x, y: before.y + 10.0 });
    assert_eq!((a.active, a.tabs[1].focus), (1, tab));
    a.on_event(Event::MouseMove { x, y: pin.y + 10.0 });
    assert_eq!((a.active, a.tabs[1].focus), (1, tab));
}

#[test]
fn dragging_a_tab_inserts_its_whole_tree_and_pane_to_tab_keeps_the_session() {
    let mut a = app(2);
    a.activate(0);
    a.run(Action::SplitBottom);
    let source = a.tabs[0].root.clone();
    let moved_focus = a.tabs[0].focus;
    a.activate(1);
    let original_focus = a.tabs[1].focus;
    a.chrome = a.tab_chrome();
    a.layout = a.tabs[1].root.layout(a.chrome.content, 1.0);
    a.take_effects();
    let tab = a.chrome.tabs[0].rect;
    a.on_event(Event::MouseDown { x: tab.x + 12.0, y: tab.y + 10.0, button: MouseButton::Left, clicks: 1 });
    assert_eq!(a.focused_pane(), Some(original_focus));
    let target = a.layout.panes[0].1;
    let (x, y) = (target.x + 5.0, target.y + target.h / 2.0);
    a.on_event(Event::MouseMove { x, y });
    assert_eq!(a.tab_drag_hover().map(|(_, zone)| zone), Some(crate::rearrange::DropZone::Side(crate::panes::Side::Left)));
    a.on_event(Event::MouseUp { x, y, button: MouseButton::Left });
    assert_eq!(a.tab_count(), 1);
    assert!(matches!(&a.tabs[0].root, crate::panes::Node::Split { a, .. } if **a == source));
    assert_eq!(a.focused_pane(), Some(moved_focus));
    a.run(Action::PaneToTab);
    assert_eq!(a.tab_count(), 2);
    assert_eq!(a.tabs[1].root, crate::panes::Node::Leaf(moved_focus));
    assert!(a.tabs[0].root.contains(original_focus));
    assert_eq!(a.panes.len(), 3);
    assert!(a.closed_tabs.is_empty());
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::Spawn { .. } | Effect::Kill { .. })));
}

#[test]
fn search_controls_stay_inside_a_narrow_pane_beside_the_tab_bar() {
    let mut a = app(1);
    a.config.appearance.tab_bar_position = TabBarPosition::Left;
    a.set_viewport(360, 260, 1.0);
    let content = a.tab_chrome().content;
    let pane = Rect::new(content.x, content.y, content.w / 2.0, content.h);
    let search = crate::search::geometry(pane, 1.0);
    assert!(encloses(pane, search.frame));
    assert!(search.buttons.iter().all(|(_, button)| encloses(search.frame, *button)));
}

#[test]
fn mru_walk_wraps_both_ways_and_updates_history_only_on_visit() {
    let mut mru = MruTabs::default();
    for key in [11, 22, 33, 11] {
        mru.visit(key);
    }
    let live = [11, 22, 33, 44];
    mru.step(&live, true);
    assert_eq!(mru.selection(), Some(([11, 33, 22, 44].as_slice(), 3)));
    mru.step(&live, false);
    mru.step(&live, false);
    mru.visit(11);
    assert_eq!(mru.finish(), Some(33));
    mru.visit(33);
    mru.step(&live, false);
    assert_eq!(mru.finish(), Some(11));
    mru.step(&live, true);
    mru.remove(44);
    assert_eq!(mru.selection(), None);
}

#[test]
fn mru_shortcuts_commit_on_ctrl_release_and_keep_tab_identity() {
    let mut a = app(4);
    let ctrl = Mods { ctrl: true, ..Mods::NONE };
    let reverse = Mods { shift: true, ..ctrl };
    let mut config = a.config().clone();
    config.appearance.mru_tabs = true;
    a.set_config(config);
    a.activate(0);
    a.activate(2);
    let from = a.tabs[2].key;
    let previous = a.tabs[0].key;
    a.run(Action::SplitRight);
    a.run(Action::MoveTabRight);
    key(&mut a, Key::Tab, ctrl);
    a.on_event(Event::Char('\t'));
    assert_eq!(a.tabs[a.active].key, from);
    assert!(a.mru.selection().is_some_and(|(order, i)| order[i] == previous));
    key(&mut a, Key::Tab, ctrl);
    key(&mut a, Key::Tab, reverse);
    a.on_event(Event::Key { key: Key::Tab, mods: ctrl, pressed: false });
    assert_eq!(a.tabs[a.active].key, from);
    a.on_event(Event::Key { key: Key::Control, mods: Mods::NONE, pressed: false });
    assert_eq!(a.tabs[a.active].key, previous);
    assert_eq!(a.mru.selection(), None);
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::Write { .. })));
    key(&mut a, Key::Tab, ctrl);
    a.on_event(Event::Modifiers(Mods::NONE));
    assert_eq!(a.tabs[a.active].key, from);
    key(&mut a, Key::Tab, ctrl);
    a.on_event(Event::Focus(false));
    assert_eq!(a.mru.selection(), None);
}

#[test]
fn mru_defaults_follow_the_setting_and_respect_custom_shortcuts() {
    let mut a = app(3);
    a.activate(0);
    let ctrl = Mods { ctrl: true, ..Mods::NONE };
    key(&mut a, Key::Tab, ctrl);
    assert_eq!((a.active, a.mru.selection()), (1, None));
    let mut config = a.config().clone();
    config.appearance.mru_tabs = true;
    config.hotkeys.insert(Action::Tab(3).id(), vec!["Ctrl-Tab".into()]);
    config.hotkeys.insert(Action::NextMruTab.id(), vec!["F8".into()]);
    a.set_config(config);
    key(&mut a, Key::Tab, ctrl);
    assert_eq!((a.active, a.mru.selection()), (2, None));
    key(&mut a, Key::F(8), Mods::NONE);
    assert_eq!((a.active, a.mru.selection()), (1, None));
}

#[test]
fn mru_shortcuts_can_be_cleared_and_restored_in_settings() {
    let mut a = app(1);
    let mut config = a.config().clone();
    config.appearance.mru_tabs = true;
    a.set_config(config);
    a.run(Action::Settings);
    let page = a.settings.as_mut().unwrap();
    page.show_page(crate::settings::PAGES.iter().position(|p| *p == crate::settings::SHORTCUTS).unwrap());
    page.selected = page.shown.iter().position(|&i| crate::settings::shortcut(i) == Some(Action::NextMruTab)).unwrap();
    assert_eq!(a.shortcut_text(Action::NextMruTab), "Ctrl-Tab");
    key(&mut a, Key::Delete, Mods::NONE);
    assert_eq!(a.keymap.bindings_of(&Action::NextMruTab).count(), 0);
    key(&mut a, Key::Delete, Mods { shift: true, ..Mods::NONE });
    assert_eq!(a.shortcut_text(Action::NextMruTab), "Ctrl-Tab");
    assert_eq!(a.shortcut_text(Action::NextTab), "Ctrl-Shift-Right");
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
fn keyboard_resizing_uses_the_configured_step_and_clamps_the_divider() {
    let mut a = app(1);
    a.run(Action::SplitRight);
    a.config.panes.resize_step = 0.3;
    a.run(Action::PaneResizeLeft);
    assert!(matches!(a.tabs[0].root, crate::panes::Node::Split { ratio, .. } if (ratio - 0.2).abs() < 1e-6));
    a.config.panes.resize_step = 2.0;
    assert_eq!(a.config.panes.resize_step(), 0.9);
    a.run(Action::PaneResizeLeft);
    assert!(matches!(a.tabs[0].root, crate::panes::Node::Split { ratio, .. } if ratio == 0.05));
    a.config.panes.resize_step = f32::NAN;
    a.run(Action::PaneResizeRight);
    assert!(matches!(a.tabs[0].root, crate::panes::Node::Split { ratio, .. } if (ratio - 0.15).abs() < 1e-6));
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
    assert!(a.tab_toast.as_ref().is_some_and(|toast| toast.text.starts_with("2 / 2")));
    assert_eq!(a.hit_test(400.0, 2.0), crate::input::HitArea::Client);
    key(&mut a, Key::Char('B'), Mods { ctrl: true, shift: true, ..Mods::NONE });
    assert!(!a.tab_bar_hidden(), "the shortcut overrides the setting");
    let c = crate::view::bare_chrome(800.0, 600.0);
    assert_eq!((c.content.y, c.content.h, c.tabs.len()), (0.0, 600.0, 0));
}

#[test]
fn animations_off_suppresses_tab_toasts_after_a_switch() {
    let mut a = app(2);
    a.set_config(Config::parse("[appearance]\ntab-bar = 'never'\nanimations = false").unwrap());
    let alt = Mods { alt: true, ..Mods::NONE };
    key(&mut a, Key::Char('1'), alt);
    key(&mut a, Key::Char('2'), alt);
    assert_eq!(a.active, 1);
    assert!(a.tab_toast.is_none());
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
