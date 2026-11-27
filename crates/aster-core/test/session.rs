use super::*;
use crate::app::Effect;
use crate::config::Config;
use crate::input::{Event, Key, Mods};
use aster_gfx::text::{FontMetrics, FontStyle, Gfx, GlyphBitmap, GlyphKey, ShapedGlyph, ShapedText, TextBackend};
use std::sync::Arc;

struct Mono;

impl TextBackend for Mono {
    fn shape(&mut self, text: &str, _: FontStyle, out: &mut ShapedText) {
        out.glyphs.clear();
        for (i, (b, c)) in text.char_indices().enumerate() {
            out.glyphs.push(ShapedGlyph { font: 0, id: c as u16, x: i as f32 * 8.0, y: 0.0, cluster: b as u32, cluster_x: i as f32 * 8.0 });
        }
        out.width = out.glyphs.len() as f32 * 8.0;
    }
    fn rasterize(&mut self, _: GlyphKey, out: &mut GlyphBitmap) -> bool {
        *out = GlyphBitmap { left: 0, top: 10, width: 6, height: 10, data: vec![255; 60], color: false };
        true
    }
    fn metrics(&mut self, _: FontStyle) -> FontMetrics {
        FontMetrics { ascent: 12.0, descent: 4.0 }
    }
}

fn app() -> (App, Gfx<Mono>) {
    crate::i18n::set_language("en");
    let profiles = vec![Profile::new("local:a", "Shell A", "a.exe", &[]), Profile::new("local:b", "Shell B", "b.exe", &["-x"])];
    let mut a = App::new(Config::default(), profiles, true, Some("H".into()), Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    (a, Gfx::new(Mono, 1024))
}

fn spawns(e: &[Effect]) -> Vec<(String, Vec<String>, Option<String>)> {
    e.iter().filter_map(|e| if let Effect::Spawn { launch, .. } = e { Some((launch.command.clone(), launch.args.clone(), launch.cwd.clone())) } else { None }).collect()
}

#[test]
fn tabs_splits_profiles_folders_and_buffers_survive_a_restart() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.focused_terminal().unwrap().lock().feed(b"\x1b]7;file://pc/D:/work\x07hello from before\r\nsecond line\r\n");
    a.on_event(Event::Key { key: Key::Char('S'), mods: Mods { ctrl: true, shift: true, ..Mods::NONE }, pressed: true });
    let b = a.profiles()[1].clone();
    a.open_tab(b);
    a.activate(0);
    a.frame(&mut g, false);
    a.take_effects();

    let state = SessionState { windows: vec![a.snapshot()], recent_folders: vec!["D:/work".into()], ..SessionState::default() };
    let text = state.to_text();
    assert!(!text.contains("needle"));
    let parsed = SessionState::parse(&text).unwrap();
    assert_eq!(parsed.to_text(), text);
    assert!(!text.contains("workspace"));
    assert_eq!(parsed.recent_folders, ["D:/work"]);
    assert!(SessionState::parse(r#"{"recent":[]}"#).unwrap().recent_folders.is_empty());

    let (mut fresh, mut g2) = app();
    assert!(fresh.restore(&parsed.windows[0]));
    assert_eq!(fresh.tab_count(), 2);
    fresh.frame(&mut g2, false);
    assert_eq!(fresh.layout.panes.len(), 2, "the split comes back");
    let launched = spawns(&fresh.take_effects());
    assert_eq!(launched.len(), 2);
    assert!(launched.iter().all(|l| l.2.as_deref() == Some(r"D:\work")), "{launched:?}");
    {
        let mut t = fresh.panes.get(&fresh.layout.panes[0].0).unwrap().terminal.lock();
        assert_eq!(t.screen_text(), "");
        t.feed(b"prompt>");
        assert_eq!(t.screen_text(), "prompt>");
        assert_eq!(t.prompt_line().0, "prompt>");
        assert_eq!(t.history(), 2);
    }
    let pane = fresh.layout.panes[0];
    fresh.on_event(Event::MouseMove { x: pane.1.x + 20.0, y: pane.1.y + 40.0 });
    fresh.on_event(Event::Wheel { dx: 0.0, dy: 1.0 });
    assert_eq!(fresh.terminal(pane.0).unwrap().lock().screen_text(), "hello from before\nsecond line\nprompt>");
    fresh.activate(1);
    fresh.frame(&mut g2, false);
    let tab_b = spawns(&fresh.take_effects());
    assert_eq!(tab_b[0].0, "b.exe");
    assert_eq!(tab_b[0].1, ["-x"]);
}

#[test]
fn garbage_or_empty_state_restores_nothing() {
    assert!(SessionState::parse("not json").is_none());
    let (mut a, _) = app();
    assert!(!a.restore(&WindowState::default()));
    assert_eq!(a.tab_count(), 0);
}

fn palette_pick(app: &mut App, value: &str) {
    use crate::keymap::Action;
    app.run(Action::CommandSelector);
    let overlay = app.overlay.as_mut().unwrap();
    let row = overlay.shown.iter().position(|(i, _)| overlay.items[*i].value == value).unwrap();
    overlay.select(row);
    app.on_event(Event::Key { key: Key::Enter, mods: Mods::NONE, pressed: true });
}

fn type_workspace_name(app: &mut App, name: &str) {
    for character in name.chars() {
        app.on_event(Event::Char(character));
    }
    app.on_event(Event::Key { key: Key::Enter, mods: Mods::NONE, pressed: true });
}

fn switch_workspace(app: &mut App, name: &str) {
    palette_pick(app, "switch-workspace");
    let overlay = app.overlay.as_mut().unwrap();
    let row = overlay.shown.iter().position(|(i, _)| overlay.items[*i].label == name).unwrap();
    overlay.select(row);
    app.on_event(Event::Key { key: Key::Enter, mods: Mods::NONE, pressed: true });
}

#[test]
fn workspace_palette_preserves_the_live_split_focus_and_scrollback() {
    use crate::keymap::Action;
    let (mut app, mut gfx) = app();
    app.open_default_tab();
    app.run(Action::SplitRight);
    app.frame(&mut gfx, false);
    let focused = app.focused_pane().unwrap();
    let terminal = app.focused_terminal().unwrap();
    for line in 0..120 {
        terminal.lock().feed(format!("line {line}\r\n").as_bytes());
    }
    terminal.lock().scroll(aster_vt::Scroll::Delta(25));
    assert_eq!(terminal.lock().display_offset(), 25);
    let split = app.tabs[0].root.clone();
    app.set_pane_host_id(focused, Some(8));
    app.take_effects();

    palette_pick(&mut app, "new-workspace");
    type_workspace_name(&mut app, "Build");
    assert_eq!(app.tab_count(), 0);
    app.open_tab(Profile { name: "Remote".into(), ssh: Some("ann@server".into()), ..Default::default() });
    app.frame(&mut gfx, false);
    let remote = app.focused_pane().unwrap();
    let remote_terminal = app.focused_terminal().unwrap();
    app.take_effects();

    switch_workspace(&mut app, "Default workspace");
    app.frame(&mut gfx, false);
    assert_eq!(app.tabs[0].root, split);
    assert_eq!(app.focused_pane(), Some(focused));
    assert!(Arc::ptr_eq(&terminal, &app.focused_terminal().unwrap()));
    assert_eq!(terminal.lock().display_offset(), 25);
    assert_eq!(app.pane_host_id(focused), Some(8));
    assert!(!app.take_effects().iter().any(|effect| matches!(effect, Effect::Spawn { .. } | Effect::Kill { .. })));
    switch_workspace(&mut app, "Build");
    app.frame(&mut gfx, false);
    assert_eq!(app.focused_pane(), Some(remote));
    assert!(Arc::ptr_eq(&remote_terminal, &app.focused_terminal().unwrap()));
    assert!(!app.take_effects().iter().any(|effect| matches!(effect, Effect::Spawn { .. } | Effect::Kill { .. })));
}

#[test]
fn an_empty_active_workspace_restores_alongside_hidden_host_ids() {
    let (mut original, _) = app();
    original.open_default_tab();
    original.set_pane_host_id(original.focused_pane().unwrap(), Some(100));
    palette_pick(&mut original, "rename-workspace");
    type_workspace_name(&mut original, "Ops");
    palette_pick(&mut original, "new-workspace");
    type_workspace_name(&mut original, "Build");
    let state = SessionState { windows: vec![original.session_checkpoint()], ..SessionState::default() };
    let parsed = SessionState::parse(&state.to_text()).unwrap();
    let mut tabs = parsed.windows[0].all_tabs();
    let NodeState::Pane(pane) = &tabs.next().unwrap().root else { panic!("expected a pane") };
    assert_eq!(pane.host_id, Some(100));
    assert!(tabs.next().is_none());
    let (mut restored, mut gfx) = app();
    let mut config = Config::default();
    config.terminal.persistent_sessions = true;
    restored.set_config(config);
    assert!(restored.restore(&parsed.windows[0]));
    assert_eq!(restored.tab_count(), 0);
    restored.frame(&mut gfx, false);
    let spawned = restored.take_effects().into_iter().find_map(|effect| match effect {
        Effect::Spawn { pane, .. } => Some(pane),
        _ => None,
    }).unwrap();
    assert_eq!(restored.pane_host_id(spawned), Some(100));
    switch_workspace(&mut restored, "Ops");
    assert_eq!(restored.focused_pane(), Some(spawned));
    switch_workspace(&mut restored, "Build");
    assert_eq!(restored.tab_count(), 0);
}

#[test]
fn a_hidden_exit_removes_its_pane_and_close_kills_remaining_hidden_sessions() {
    use crate::keymap::Action;
    let (mut app, _) = app();
    app.open_default_tab();
    let survivor = app.focused_pane().unwrap();
    app.set_pane_host_id(survivor, Some(9));
    app.run(Action::SplitRight);
    let exiting = app.focused_pane().unwrap();
    palette_pick(&mut app, "new-workspace");
    type_workspace_name(&mut app, "Build");

    app.exited(exiting, Some(0));
    assert!(app.terminal(exiting).is_none());
    assert_eq!(app.tab_count(), 0);
    switch_workspace(&mut app, "Default workspace");
    assert_eq!(app.tabs[0].root, Node::Leaf(survivor));
    assert_eq!(app.focused_pane(), Some(survivor));
    switch_workspace(&mut app, "Build");
    app.run(Action::CloseWindow);
    let effects = app.take_effects();
    assert!(effects.iter().any(|effect| matches!(effect, Effect::Kill { pane, host_id: Some(9) } if *pane == survivor)));
    assert!(effects.iter().any(|effect| matches!(effect, Effect::Close)));
    assert!(app.terminal(survivor).is_none());
}

#[test]
fn closing_the_last_visible_tab_keeps_other_workspaces_and_window_close_checks_them() {
    use crate::keymap::Action;
    let (mut app, _) = app();
    let mut config = Config::default();
    config.terminal.close_window_with_last_tab = true;
    app.set_config(config);
    app.open_tab(Profile { name: "Remote".into(), ssh: Some("ann@server".into()), ..Default::default() });
    let remote = app.focused_pane().unwrap();
    palette_pick(&mut app, "new-workspace");
    type_workspace_name(&mut app, "Build");
    app.open_default_tab();
    app.run(Action::CloseTab);
    assert_eq!(app.tab_count(), 0);
    assert!(!app.take_effects().iter().any(|effect| matches!(effect, Effect::Close)));

    app.run(Action::CloseWindow);
    let confirm = app.close_confirm().unwrap();
    assert!(confirm.ssh);
    assert_eq!(confirm.programs, ["Remote"]);
    app.on_event(Event::Key { key: Key::Escape, mods: Mods::NONE, pressed: true });
    assert!(app.terminal(remote).is_some());
}

#[test]
fn last_tab_closes_the_window_when_other_workspaces_are_empty() {
    use crate::keymap::Action;
    let (mut app, _) = app();
    let mut config = Config::default();
    config.terminal.close_window_with_last_tab = true;
    app.set_config(config);
    palette_pick(&mut app, "new-workspace");
    type_workspace_name(&mut app, "Build");
    app.open_default_tab();
    app.run(Action::CloseTab);
    assert!(app.take_effects().iter().any(|effect| matches!(effect, Effect::Close)));
    let snapshot = app.session_snapshot();
    assert_eq!(snapshot.workspace.as_deref(), Some("Build"));
    assert_eq!(snapshot.workspaces.len(), 1);
    assert!(snapshot.all_tabs().next().is_none());
}

#[test]
fn background_activity_notifications_reopen_their_workspace() {
    use std::time::{Duration, Instant};
    let (mut app, _) = app();
    let mut config = Config::default();
    config.notifications.activity = true;
    config.notifications.activity_silence_seconds = 1;
    app.set_config(config);
    app.open_default_tab();
    let pane = app.focused_pane().unwrap();
    palette_pick(&mut app, "new-workspace");
    type_workspace_name(&mut app, "Build");
    app.take_effects();
    app.terminal(pane).unwrap().lock().feed(b"new output");
    app.poll_background_activity(Instant::now() + Duration::from_secs(2));
    let key = app.take_effects().into_iter().find_map(|effect| match effect {
        Effect::Notify(notification) => Some(notification.tab),
        _ => None,
    }).unwrap();
    app.on_event(Event::NotificationClicked(key));
    assert_eq!(app.focused_pane(), Some(pane));
}

#[test]
fn checkpoint_keeps_host_ids_and_restores_saved_text_only_on_fallback() {
    let (mut original, _) = app();
    original.open_default_tab();
    let pane = original.focused_pane().unwrap();
    original.set_pane_host_id(pane, Some(u64::MAX));
    original.terminal(pane).unwrap().lock().feed(b"saved text");
    let checkpoint = original.session_checkpoint();
    let NodeState::Pane(p) = &checkpoint.tabs[0].root else { panic!("expected a pane") };
    assert_eq!(p.host_id, Some(u64::MAX));
    assert!(p.buffer.is_empty());

    let text = SessionState { windows: vec![original.snapshot()], ..SessionState::default() }.to_text();
    let state = SessionState::parse(&text).unwrap();
    for reattached in [false, true] {
        let (mut restored, _) = app();
        let mut config = Config::default();
        config.terminal.persistent_sessions = true;
        restored.set_config(config);
        assert!(restored.restore(&state.windows[0]));
        let pane = restored.focused_pane().unwrap();
        assert_eq!(restored.pane_host_id(pane), Some(u64::MAX));
        assert_eq!(restored.terminal(pane).unwrap().lock().screen_text(), "");
        restored.finish_pane_restore(pane, reattached);
        let mut terminal = restored.terminal(pane).unwrap().lock();
        assert_eq!(terminal.screen_text(), "");
        assert_eq!(terminal.history(), usize::from(!reattached));
        terminal.scroll(aster_vt::Scroll::Top);
        assert_eq!(terminal.screen_text(), if reattached { "" } else { "saved text" });
    }
}

#[test]
fn saved_host_ids_launch_in_inactive_tabs_during_restore() {
    let (mut original, _) = app();
    original.open_default_tab();
    original.set_pane_host_id(original.focused_pane().unwrap(), Some(10));
    original.open_tab(original.profiles()[1].clone());

    let (mut restored, mut gfx) = app();
    let mut config = Config::default();
    config.terminal.persistent_sessions = true;
    restored.set_config(config);
    assert!(restored.restore(&original.snapshot()));
    restored.frame(&mut gfx, false);
    let launches = spawns(&restored.take_effects());
    assert_eq!(launches.iter().map(|(command, _, _)| command.as_str()).collect::<Vec<_>>(), ["a.exe", "b.exe"]);
}

#[test]
fn saved_layout_reopens_the_tree_with_current_profiles_and_leaf_folders() {
    use crate::keymap::Action;
    let (mut a, mut gfx) = app();
    a.open_default_tab();
    a.run(Action::ProfileSelector);
    a.overlay.as_mut().unwrap().insert("Shell B");
    a.on_event(Event::Key { key: Key::Enter, mods: Mods { ctrl: true, ..Mods::NONE }, pressed: true });
    a.run(Action::SplitBottom);
    let ids = a.tabs[0].root.panes();
    for (id, dir) in ids.iter().zip([r"D:\one", r"D:\two", r"D:\three"]) {
        a.terminal(*id).unwrap().lock().set_cwd(Some(dir.into()));
    }
    a.tabs[0].root.set_ratio(&[], 0.35);
    a.run(Action::SaveLayout);
    let text = a.config.to_text();
    assert!(!text.contains("buffer ="));
    let mut config = Config::parse(&text).unwrap();
    let saved = config.profiles.last().unwrap().clone();
    let pick = a.profile_picks().into_iter().find(|p| p.value == saved.id).unwrap();
    assert_eq!((pick.badge.as_str(), pick.hint.as_str()), ("▦", "Layout"));
    let (mut b, _) = app();
    config.profile_defaults.entry("local".into()).or_default().env.insert("NEW".into(), "yes".into());
    b.set_config(config);
    b.open_tab(saved);
    b.frame(&mut gfx, false);
    let effects = b.take_effects();
    let dirs: Vec<_> = effects.iter().filter_map(|e| match e {
        Effect::Spawn { launch, .. } => { assert_eq!(launch.env["NEW"], "yes"); launch.cwd.as_deref() }
        _ => None,
    }).collect();
    assert_eq!(dirs, [r"D:\one", r"D:\two", r"D:\three"]);
    let programs = spawns(&effects).into_iter().map(|(program, _, _)| program).collect::<Vec<_>>();
    assert_eq!(programs, ["a.exe", "b.exe", "b.exe"]);
    assert!(matches!(&b.tabs[0].root, Node::Split { axis: Axis::Horizontal, ratio, b, .. } if (*ratio - 0.35).abs() < 1e-6 && matches!(**b, Node::Split { axis: Axis::Vertical, .. })));
}

#[test]
fn layouts_with_a_missing_leaf_leave_the_existing_tab_intact() {
    let (mut a, _) = app();
    a.open_default_tab();
    let old = a.focused_pane();
    let leaf = |id: &str| Box::new(ProfileLayout::Pane { profile: id.into(), cwd: None });
    let layout = ProfileLayout::Split { vertical: false, ratio: 0.5, a: leaf("local:a"), b: leaf("missing") };
    a.open_tab(Profile { layout: Some(layout), ..Default::default() });
    assert_eq!((a.tab_count(), a.panes.len(), a.focused_pane()), (1, 1, old));
    assert!(a.tab_toast.as_ref().unwrap().text.contains("missing"));
}

#[test]
fn restored_sessions_resolve_defaults_against_the_new_configuration() {
    let (mut a, _) = app();
    a.set_config(Config::parse("[profile-defaults.local.env]\nVERSION='first'").unwrap());
    a.open_default_tab();
    let snapshot = a.snapshot();
    let (mut b, _) = app();
    b.set_config(Config::parse("[profile-defaults.local.env]\nVERSION='second'").unwrap());
    assert!(b.restore(&snapshot));
    assert_eq!(b.panes[&b.tabs[0].focus].pending.as_ref().unwrap().env["VERSION"], "second");
}

#[test]
fn a_split_remembers_the_folder_it_inherited_before_the_shell_reports_one() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.focused_terminal().unwrap().lock().feed(b"\x1b]7;file://pc/D:/work\x07");
    a.on_event(Event::Key { key: Key::Char('D'), mods: Mods { ctrl: true, shift: true, ..Mods::NONE }, pressed: true });
    a.frame(&mut g, false);
    let state = a.snapshot();
    let NodeState::Split { b, .. } = &state.tabs[0].root else { panic!("split") };
    let NodeState::Pane(p) = b.as_ref() else { panic!("pane") };
    assert_eq!(p.cwd.as_deref(), Some(r"D:\work"));
}
