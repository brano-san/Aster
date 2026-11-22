use super::*;
use crate::config::Config;
use crate::input::Event;
use crate::keymap::Action;
use crate::profile::Profile;
use std::sync::Arc;

fn app() -> App {
    let profiles = vec![Profile::new("local:a", "Shell", "a.exe", &[]), Profile::new("local:b", "Another shell", "b.exe", &[])];
    let mut a = App::new(Config::default(), profiles, true, None, Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    a
}

fn key(a: &mut App, key: Key, mods: Mods) {
    a.on_event(Event::Key { key, mods, pressed: true });
}

fn typed(a: &mut App, text: &str) {
    for c in text.chars() {
        a.on_event(Event::Char(c));
    }
}

#[test]
fn pane_names_commit_inline_and_survive_a_session_round_trip() {
    let mut a = app();
    a.open_default_tab();
    let pane = a.focused_pane().unwrap();
    a.run(Action::RenamePane);
    typed(&mut a, "  Build output  ");
    key(&mut a, Key::Enter, Mods::NONE);
    assert_eq!(a.panes[&pane].name.as_deref(), Some("Build output"));
    let rect = Rect::new(20.0, 30.0, 120.0, 150.0);
    let header = a.pane_header(pane, rect);
    assert!(header.h > 0.0);
    assert_eq!(a.pane_terminal_rect(pane, rect).y, header.bottom());
    let state = serde_json::from_str(&serde_json::to_string(&a.snapshot()).unwrap()).unwrap();
    let mut restored = app();
    assert!(restored.restore(&state));
    let pane = restored.focused_pane().unwrap();
    assert_eq!(restored.panes[&pane].name.as_deref(), Some("Build output"));
    restored.run(Action::RenamePane);
    key(&mut restored, Key::Backspace, Mods { ctrl: true, ..Mods::NONE });
    key(&mut restored, Key::Enter, Mods::NONE);
    assert_eq!(restored.panes[&pane].name, None);
    assert_eq!(restored.pane_terminal_rect(pane, rect), rect);
    restored.config.appearance.pane_titles = true;
    assert!(restored.pane_header(pane, rect).h > 0.0);
}

#[test]
fn changing_a_busy_pane_profile_asks_asynchronously_and_keeps_its_slot() {
    let mut a = app();
    a.open_default_tab();
    a.chrome = a.tab_chrome();
    let first = a.focused_pane().unwrap();
    a.run(Action::SplitRight);
    let old = a.focused_pane().unwrap();
    a.set_pane_pid(old, 7);
    a.panes.get_mut(&old).unwrap().name = Some("Build output".into());
    let layout = a.tabs[0].root.layout(a.chrome.content, 1.0);
    for accept in [false, true] {
        a.run(Action::ChangePaneProfile);
        typed(&mut a, "Another shell");
        key(&mut a, Key::Enter, Mods::NONE);
        let request = a.take_effects().into_iter().find_map(|effect| match effect {
            Effect::ProbeCommand { request, pids } if pids == [7] => Some(request),
            _ => None,
        }).unwrap();
        assert!(a.close_confirm().is_none() && a.panes.contains_key(&old));
        a.command_probe_finished(request, Some(true));
        assert!(a.close_confirm().is_some());
        key(&mut a, if accept { Key::Enter } else { Key::Escape }, Mods::NONE);
    }
    let new = a.focused_pane().unwrap();
    assert_ne!(new, old);
    assert!(!a.panes.contains_key(&old));
    assert_eq!(a.tabs[0].root.panes(), [first, new]);
    assert_eq!(a.tabs[0].root.layout(a.chrome.content, 1.0).panes[1].1, layout.panes[1].1);
    assert_eq!(a.panes[&new].profile.id, "local:b");
    assert_eq!(a.panes[&new].name.as_deref(), Some("Build output"));
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Kill { pane, .. } if *pane == old)));
}
