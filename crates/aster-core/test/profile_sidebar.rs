use super::*;
use crate::config::Config;
use crate::input::Event;
use crate::keymap::Action;
use crate::profile::{Profile, ProfileGroup};
use std::sync::Arc;

fn app() -> App {
    let mut config = Config::default();
    config.appearance.profile_sidebar = true;
    config.groups = vec![
        ProfileGroup { id: "ops".into(), name: "Operations".into(), ..ProfileGroup::default() },
        ProfileGroup { id: "child".into(), name: "Servers".into(), parent: Some("ops".into()), ..ProfileGroup::default() },
    ];
    let mut visible = Profile { builtin: false, ..Profile::new("custom:alpha", "Alpha", "alpha.exe", &[]) };
    visible.group = Some("Servers".into());
    let mut hidden = Profile { builtin: false, ..Profile::new("custom:bravo", "Bravo", "bravo.exe", &[]) };
    hidden.hidden = true;
    hidden.group = visible.group.clone();
    config.profiles = vec![visible, hidden];
    let mut app = App::new(config, Vec::new(), true, None, Arc::new(|| {}));
    app.set_viewport(1000, 700, 1.0);
    app.chrome = app.tab_chrome();
    app
}

#[test]
fn sidebar_filter_keeps_ancestors_and_respects_hidden_profiles_and_collapse() {
    let mut a = app();
    let browser = &mut a.profile_sidebar;
    assert!(browser.profiles.iter().all(|p| p.id != "custom:bravo"));
    assert!(browser.rows.iter().any(|r| matches!(r.kind, TreeKind::Profile(0)) && r.depth == 2));
    browser.collapsed.insert(Header::Group("ops".into()));
    browser.refilter(&a.config.groups);
    assert!(!browser.rows.iter().any(|r| matches!(r.kind, TreeKind::Profile(_))));
    browser.query = "alpha.exe".into();
    browser.refilter(&a.config.groups);
    assert_eq!(browser.rows.iter().map(|r| r.depth).collect::<Vec<_>>(), [0, 1, 2]);
    browser.query = "bravo".into();
    browser.refilter(&a.config.groups);
    assert!(browser.rows.is_empty());
}

#[test]
fn sidebar_keyboard_launches_and_resize_persists_logical_width() {
    let mut a = app();
    let panel = a.chrome.profile_sidebar;
    a.on_event(Event::MouseDown { x: panel.x + 14.0, y: panel.y + 16.0, button: MouseButton::Left, clicks: 1 });
    for c in "alpha".chars() {
        a.on_event(Event::Char(c));
    }
    a.on_event(Event::Key { key: Key::Enter, mods: Mods::NONE, pressed: true });
    assert_eq!(a.tab_count(), 1);
    assert_eq!(a.tab_title(0), "Alpha");
    a.on_event(Event::MouseDown { x: panel.right() - 1.0, y: panel.y + 80.0, button: MouseButton::Left, clicks: 1 });
    a.on_event(Event::MouseMove { x: panel.x + 320.0, y: panel.y + 80.0 });
    a.on_event(Event::MouseUp { x: panel.x + 320.0, y: panel.y + 80.0, button: MouseButton::Left });
    assert_eq!(a.tab_chrome().profile_sidebar.w, 320.0);
    assert!(a.take_effects().into_iter().any(|e| matches!(e, Effect::SaveConfig(text) if Config::parse(&text).unwrap().appearance.profile_sidebar_width == 320.0)));
    a.run(Action::ToggleProfileSidebar);
    assert_eq!(a.tab_chrome().profile_sidebar.w, 0.0);
}

#[test]
fn sidebar_context_edit_opens_the_profile_editor_in_settings() {
    let mut a = app();
    a.open_sidebar_profile_menu("custom:alpha".into(), 42.0, 80.0);
    a.on_event(Event::Key { key: Key::End, mods: Mods::NONE, pressed: true });
    a.on_event(Event::Key { key: Key::Enter, mods: Mods::NONE, pressed: true });
    assert!(a.settings.as_ref().is_some_and(|page| page.editor.is_some()));
    assert_eq!(a.tab_chrome().profile_sidebar.w, 0.0);
}
