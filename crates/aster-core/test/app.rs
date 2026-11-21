use super::*;
use crate::config::Config;
use aster_gfx::text::{FontMetrics, FontStyle, GlyphBitmap, GlyphKey, Gfx, ShapedGlyph, ShapedText, TextBackend};

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

fn profile(id: &str) -> Profile {
    Profile::new(id, id, "shell.exe", &[])
}

fn app() -> (App, Gfx<Mono>) {
    let mut a = App::new(Config::default(), vec![profile("local:a"), profile("local:b")], true, Some("C:\\home".into()), Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    (a, Gfx::new(Mono, 1024))
}

fn spawns(effects: &[Effect]) -> Vec<(PaneId, Launch)> {
    effects.iter().filter_map(|e| if let Effect::Spawn { pane, launch, .. } = e { Some((*pane, launch.clone())) } else { None }).collect()
}

#[test]
fn the_config_page_wraps_full_paths_and_errors_in_a_narrow_window() {
    let (mut a, mut gfx) = app();
    a.set_viewport(350, 640, 1.0);
    let path = format!("data/{}/config.toml", "файлы/".repeat(16));
    let error = format!("Line 3, column 9: {}", "unexpected key ".repeat(7));
    a.set_config_path(Some(path.clone()));
    a.set_config_status(Some(error.clone()));
    a.run(Action::Settings);
    a.on_event(Event::Key { key: Key::PageUp, mods: Mods { ctrl: true, ..Mods::NONE }, pressed: true });
    a.frame(&mut gfx, false);
    let page = a.settings.as_ref().unwrap();
    let geo = a.settings_geometry().unwrap();
    for (n, expected) in [(0, path), (3, error)] {
        let lines = &page.info_rows[&page.shown[n]];
        assert!(lines.len() > 1);
        assert_eq!(lines.concat(), expected);
        let row = geo.rows.iter().find(|r| r.kind == crate::settings::RowKind::Item(n)).unwrap();
        assert!(lines.iter().all(|line| line.chars().count() as f32 * 8.0 <= row.rect.w - 28.0));
        assert!(row.rect.h >= 58.0 + (lines.len() - 1) as f32 * 16.0);
    }
}

fn take_command_probe(a: &mut App) -> (u64, Vec<u32>) {
    a.take_effects().into_iter().find_map(|e| match e {
        Effect::ProbeCommand { request, pids } => Some((request, pids)),
        _ => None,
    }).expect("a scheduled command probe")
}

#[test]
fn command_notification_polls_until_busy_then_idle_only_once() {
    let (mut a, _) = app();
    a.config.terminal.cursor_blink = false;
    a.open_default_tab();
    a.set_pane_pid(a.focused_pane().unwrap(), 72);
    a.run(Action::SplitRight);
    a.set_pane_pid(a.focused_pane().unwrap(), 91);
    a.run(Action::NotifyCommand);
    let (request, pids) = take_command_probe(&mut a);
    assert_eq!(pids, [72, 91]);
    let before = Instant::now();
    a.command_probe_finished(request, Some(false));
    let effects = a.take_effects();
    assert!(effects.iter().any(|e| matches!(e, Effect::Timer(1..=2000))));
    assert!(!effects.iter().any(|e| matches!(e, Effect::Attention)));
    let next = a.tabs[0].command_watch.deadline().unwrap();
    assert!(next >= before + Duration::from_secs(2));
    a.poll_commands(next - Duration::from_millis(1));
    assert!(a.take_effects().is_empty());
    a.poll_commands(next);
    let (request, _) = take_command_probe(&mut a);
    a.command_probe_finished(request, Some(true));
    a.open_default_tab();
    a.poll_commands(a.tabs[0].command_watch.deadline().unwrap());
    let (request, _) = take_command_probe(&mut a);
    a.command_probe_finished(request, Some(false));
    assert!(a.tab_activity(0));
    a.command_probe_finished(request, Some(false));
    a.poll_commands(Instant::now() + Duration::from_secs(5));
    let effects = a.take_effects();
    assert_eq!(effects.iter().filter(|e| matches!(e, Effect::Attention)).count(), 1);
    assert!(!effects.iter().any(|e| matches!(e, Effect::ProbeCommand { .. })));
    a.activate(0);
    assert!(!a.tab_activity(0));
}

#[test]
fn command_notification_discards_cancelled_and_closed_tab_replies() {
    let (mut a, _) = app();
    a.open_default_tab();
    a.set_pane_pid(a.focused_pane().unwrap(), 44);
    a.run(Action::NotifyCommand);
    let (first, _) = take_command_probe(&mut a);
    a.run(Action::NotifyCommand);
    a.run(Action::NotifyCommand);
    let (second, _) = take_command_probe(&mut a);
    a.command_probe_finished(first, Some(true));
    a.command_probe_finished(second, Some(false));
    assert!(!a.tab_activity(0));
    a.poll_commands(a.tabs[0].command_watch.deadline().unwrap());
    let (request, _) = take_command_probe(&mut a);
    a.open_default_tab();
    a.close_tab(0);
    a.command_probe_finished(request, Some(true));
    assert!(!a.tabs[0].command_watch.armed());
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::Attention)));
}

#[test]
fn failed_command_probe_retries_without_clearing_the_busy_state() {
    let (mut a, _) = app();
    a.open_default_tab();
    a.set_pane_pid(a.focused_pane().unwrap(), 10);
    a.run(Action::NotifyCommand);
    let (request, _) = take_command_probe(&mut a);
    a.command_probe_finished(request, Some(true));
    a.poll_commands(a.tabs[0].command_watch.deadline().unwrap());
    let (request, _) = take_command_probe(&mut a);
    a.command_probe_finished(request, None);
    assert!(!a.tab_activity(0));
    a.poll_commands(a.tabs[0].command_watch.deadline().unwrap());
    let (request, _) = take_command_probe(&mut a);
    a.command_probe_finished(request, Some(false));
    assert!(a.tab_activity(0));
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Attention)));
}

#[test]
fn turning_animations_off_clears_transients_and_stops_the_cursor_timer() {
    let (mut a, mut gfx) = app();
    a.open_default_tab();
    let pane = a.focused_pane().unwrap();
    a.blink_on = false;
    a.bell = Some((pane, Instant::now()));
    a.tab_toast = Some((String::new(), Instant::now()));
    a.set_config(Config::parse("[appearance]\nanimations = false").unwrap());
    a.frame(&mut gfx, false);
    assert!(a.blink_on);
    assert!(a.tab_toast.is_none() && a.bell.is_none());
    assert!(!a.take_effects().iter().any(|effect| matches!(effect, Effect::Timer(_))));
    a.terminal(pane).unwrap().lock().feed(b"\x07");
    a.woken();
    assert!(!a.bell_flash(pane));
    assert_eq!(a.take_effects().iter().filter(|effect| matches!(effect, Effect::Attention)).count(), 1);
}

#[test]
fn pane_shapes_ligatures_only_across_adjacent_cells_with_the_same_style() {
    #[derive(Default)]
    struct Recorder(Vec<String>);

    impl TextBackend for Recorder {
        fn shape(&mut self, text: &str, style: FontStyle, out: &mut ShapedText) {
            if style.ligatures {
                self.0.push(text.to_owned());
            }
            Mono.shape(text, style, out);
        }
        fn rasterize(&mut self, key: GlyphKey, out: &mut GlyphBitmap) -> bool {
            Mono.rasterize(key, out)
        }
        fn metrics(&mut self, style: FontStyle) -> FontMetrics {
            Mono.metrics(style)
        }
    }

    let (mut app, _) = app();
    let mut g = Gfx::new(Recorder::default(), 1024);
    app.open_default_tab();
    app.frame(&mut g, false);
    let terminal = app.focused_terminal().unwrap();
    terminal.lock().feed("=>界e\u{301}\x1b[31m!=\x1b[0m\r\n->".as_bytes());
    app.frame(&mut g, false);
    assert!(g.text.0.is_empty());
    app.config.terminal.ligatures = true;
    app.frame(&mut g, false);
    assert_eq!(g.text.0, ["=>界e\u{301}", "!=", "->"]);
    let mut vt = terminal.lock();
    vt.start_selection(SelectionType::Simple, vt_point(0), aster_vt::Side::Left);
    vt.update_selection(vt_point(0), aster_vt::Side::Right);
    drop(vt);
    g.text.0.clear();
    app.frame(&mut g, false);
    assert_eq!(g.text.0, ["=", ">界e\u{301}"]);

    fn vt_point(col: usize) -> aster_vt::Point {
        aster_vt::Point::new(aster_vt::Line(0), aster_vt::Column(col))
    }
}

#[test]
fn density_scales_the_tab_bar_without_changing_terminal_cell_metrics() {
    let (mut app, mut gfx) = app();
    app.set_viewport(1000, 700, 1.5);
    app.open_default_tab();
    app.frame(&mut gfx, false);
    let original = app.metrics.unwrap();
    for density in [0.7, 1.3] {
        let config = Config::parse(&format!("[appearance]\ndensity = {density}")).unwrap();
        app.set_config(config);
        app.frame(&mut gfx, false);
        assert_eq!(app.chrome.tab_bar.h, (view::TAB_H * 1.5 * density).round());
        let metrics = app.metrics.unwrap();
        assert_eq!((metrics.px, metrics.w, metrics.h, metrics.baseline), (original.px, original.w, original.h, original.baseline));
    }
}

fn writes(effects: &[Effect]) -> Vec<Vec<u8>> {
    effects.iter().filter_map(|e| if let Effect::Write { bytes, .. } = e { Some(bytes.clone()) } else { None }).collect()
}

#[test]
fn alt_letters_use_the_configured_meta_prefix() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    let mods = Mods { alt: true, ..Mods::NONE };
    for (enabled, expected) in [(true, "\x1bж"), (false, "ж")] {
        a.set_config(Config::parse(&format!("[terminal]\nalt-is-meta = {enabled}")).unwrap());
        key(&mut a, Key::Char('J'), mods);
        a.on_event(Event::AltChar('ж'));
        assert_eq!(writes(&a.take_effects()), [expected.as_bytes()]);
    }
}

const CTRL_SHIFT: Mods = Mods { ctrl: true, shift: true, alt: false, meta: false };

fn key(a: &mut App, k: Key, mods: Mods) {
    a.on_event(Event::Key { key: k, mods, pressed: true });
}

#[test]
fn the_first_frame_spawns_the_default_profile_sized_to_the_pane() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    let s = spawns(&a.take_effects());
    assert_eq!(s.len(), 1);
    let launch = &s[0].1;
    assert_eq!(launch.command, "shell.exe");
    assert_eq!(launch.cwd.as_deref(), Some("C:\\home"));
    assert_eq!(launch.env.get("COLORTERM").map(String::as_str), Some("truecolor"));
    assert_eq!((launch.cols, launch.rows), (123, 40));
    assert!(!g.dl.instances.is_empty());
}

fn serial_profile(spec: &str) -> Profile {
    Profile { serial: Some(spec.into()), ..profile("serial:test") }
}

#[test]
fn serial_profile_waits_for_a_valid_speed_before_spawning() {
    let (mut a, mut gfx) = app();
    a.open_tab(serial_profile("dev:port:?,7E2,rtscts,slow"));
    let pane = a.focused_pane().unwrap();
    a.frame(&mut gfx, false);
    let picker = a.overlay.as_ref().unwrap();
    assert_eq!(picker.kind, OverlayKind::SerialSpeed(pane));
    assert_eq!(picker.selected_value().as_deref(), Some("115200"));
    assert!(spawns(&a.take_effects()).is_empty());
    a.on_event(Event::Char('1'));
    key(&mut a, Key::Enter, Mods::NONE);
    a.frame(&mut gfx, false);
    assert!(spawns(&a.take_effects()).is_empty());
    for c in "2345".chars() {
        a.on_event(Event::Char(c));
    }
    key(&mut a, Key::Enter, Mods::NONE);
    a.on_event(Event::Char('\r'));
    a.frame(&mut gfx, false);
    let effects = a.take_effects();
    let launches = spawns(&effects);
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].0, pane);
    assert_eq!(launches[0].1.serial.as_deref(), Some("dev:port:12345,7E2,rtscts,slow"));
    assert_eq!((launches[0].1.cols, launches[0].1.rows), (123, 40));
    assert!(writes(&effects).is_empty());
}

#[test]
fn cancelling_serial_speed_closes_only_an_unlaunched_pane() {
    let (mut a, mut gfx) = app();
    a.open_default_tab();
    let original = a.focused_pane();
    a.split_with(Side::Right, serial_profile("COM3 : ? "));
    a.frame(&mut gfx, false);
    let first = spawns(&a.take_effects());
    assert_eq!(first.len(), 1);
    assert_eq!(Some(first[0].0), original);
    key(&mut a, Key::Escape, Mods::NONE);
    assert_eq!(a.focused_pane(), original);
    assert_eq!(a.panes.len(), 1);
    a.open_tab(serial_profile("COM3:9600"));
    a.frame(&mut gfx, false);
    a.run(Action::SerialSpeed);
    key(&mut a, Key::Escape, Mods::NONE);
    assert_eq!(a.panes.len(), 2);
    assert!(a.overlay.is_none());
    assert!(a.take_effects().iter().all(|e| !matches!(e, Effect::SerialControl { .. })));
}

#[test]
fn restored_serial_panes_wait_for_their_own_speed_selection() {
    let (mut original, _) = app();
    original.open_tab(serial_profile("COM1:?"));
    original.split_with(Side::Right, serial_profile("COM2:?"));
    let (mut restored, mut gfx) = app();
    assert!(restored.restore(&original.snapshot()));
    restored.frame(&mut gfx, false);
    assert!(spawns(&restored.take_effects()).is_empty());
    key(&mut restored, Key::Enter, Mods::NONE);
    restored.frame(&mut gfx, false);
    let launches = spawns(&restored.take_effects());
    assert_eq!(launches.len(), 1);
    assert_eq!(launches[0].1.serial.as_deref(), Some("COM2:115200"));
    restored.run(Action::PaneNavPrevious);
    restored.frame(&mut gfx, false);
    assert!(spawns(&restored.take_effects()).is_empty());
    assert_eq!(restored.overlay.as_ref().map(|o| o.kind), restored.focused_pane().map(OverlayKind::SerialSpeed));
    key(&mut restored, Key::Escape, Mods::NONE);
    assert_eq!(restored.focused_pane(), Some(launches[0].0));
}

#[test]
fn serial_actions_target_only_the_focused_serial_session() {
    let (mut a, mut gfx) = app();
    a.open_tab(serial_profile("COM1:9600"));
    a.frame(&mut gfx, false);
    a.take_effects();
    let pane = a.focused_pane().unwrap();
    a.run(Action::SerialSpeed);
    a.on_event(Event::Pasted("57600".into()));
    key(&mut a, Key::Enter, Mods::NONE);
    a.run(Action::SerialBreak);
    let controls: Vec<_> = a.take_effects().into_iter().filter_map(|e| match e {
        Effect::SerialControl { pane, control } => Some((pane, control)),
        _ => None,
    }).collect();
    assert_eq!(controls, [(pane, SerialControl::Baud(57600)), (pane, SerialControl::Break)]);
    a.open_default_tab();
    a.frame(&mut gfx, false);
    a.run(Action::SerialSpeed);
    a.run(Action::SerialBreak);
    assert!(a.overlay.is_none());
    assert!(a.take_effects().iter().all(|e| !matches!(e, Effect::SerialControl { .. })));
    a.run(Action::CommandSelector);
    assert!(!a.overlay.unwrap().items.iter().any(|p| matches!(p.value.as_str(), "serial-speed" | "serial-break")));
}

#[test]
fn terminal_identification_in_spawn_preserves_explicit_environment() {
    for mode in ["aster", "xterm"] {
        for overrides in [false, true] {
            let (mut a, mut gfx) = app();
            if mode != "aster" {
                a.set_config(toml::from_str("[terminal]\nidentification = 'xterm'").unwrap());
            }
            let mut p = profile("local");
            if overrides {
                a.config.terminal.environment.insert("TERM_PROGRAM_VERSION".into(), "custom-version".into());
                a.config.terminal.environment.insert("TERM_PROGRAM".into(), "global".into());
                p.env.insert("TERM_PROGRAM".into(), "profile".into());
            }
            a.open_tab(p);
            a.frame(&mut gfx, false);
            let launches = spawns(&a.take_effects());
            let env = &launches[0].1.env;
            assert_eq!(env["TERM"], "xterm-256color");
            assert_eq!(env["COLORTERM"], "truecolor");
            let (name, version) = if overrides { (Some("profile"), Some("custom-version")) }
                else if mode == "aster" { (Some("Aster"), Some(env!("CARGO_PKG_VERSION"))) }
                else { (None, None) };
            assert_eq!(env.get("TERM_PROGRAM").map(String::as_str), name);
            assert_eq!(env.get("TERM_PROGRAM_VERSION").map(String::as_str), version);
        }
    }
}

#[test]
fn debug_actions_target_the_focused_terminal() {
    let (mut a, mut gfx) = app();
    a.open_default_tab();
    a.focused_terminal().unwrap().lock().feed(b"first pane");
    a.run(Action::SplitRight);
    a.frame(&mut gfx, false);
    a.take_effects();
    a.focused_terminal().unwrap().lock().feed(b"second pane");
    let pane = a.focused_pane().unwrap();
    a.run(Action::DebugCopyState);
    a.run(Action::DebugSaveOutput);
    let effects = a.take_effects();
    assert!(effects.iter().any(|e| matches!(e, Effect::SetClipboard(s) if s.contains("second pane") && !s.contains("first pane"))));
    assert!(effects.iter().any(|e| matches!(e, Effect::SaveOutput { pane: id } if *id == pane)));
}

#[test]
fn export_buffer_snapshots_the_focused_panes_history_and_screen() {
    let (mut a, _) = app();
    a.open_default_tab();
    a.focused_terminal().unwrap().lock().feed(b"other pane");
    a.run(Action::SplitRight);
    let id = a.focused_pane().unwrap();
    let terminal = a.focused_terminal().unwrap();
    let mut vt = terminal.lock();
    vt.resize(Size::new(5, 2));
    vt.feed("abcdef\r\n\x1b[31me\u{301}界\r\nlast".as_bytes());
    vt.scroll(Scroll::Top);
    drop(vt);
    a.run(Action::from_id("export-buffer").unwrap());
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::ExportBuffer { pane, text } if *pane == id && text == "abcdef\ne\u{301}界\nlast")));
}

#[test]
fn copy_as_html_keeps_plain_text_and_styles_only_the_selected_cells() {
    let (mut a, _) = app();
    a.open_default_tab();
    let terminal = a.focused_terminal().unwrap();
    let mut vt = terminal.lock();
    vt.resize(Size::new(6, 2));
    vt.feed("x\x1b[1;38;2;1;2;3;48;2;4;5;6m<&>界e\u{301}\x1b[0m\r\nend\r\nlast".as_bytes());
    vt.scroll(Scroll::Top);
    let start = vt.viewport_point(0, 1);
    vt.start_selection(SelectionType::Simple, start, aster_vt::Side::Left);
    vt.update_selection(aster_vt::Point::new(aster_vt::Line(0), aster_vt::Column(2)), aster_vt::Side::Right);
    drop(vt);
    a.run(Action::Copy);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::SetClipboard(text) if text == "<&>界e\u{301}\nend")));
    a.config.terminal.copy_as_html = true;
    a.run(Action::Copy);
    let effects = a.take_effects();
    let (plain, html) = effects.iter().find_map(|e| match e { Effect::SetClipboardHtml { text, html } => Some((text, html)), _ => None }).unwrap();
    assert_eq!(plain, "<&>界e\u{301}\nend");
    assert!(html.starts_with("<pre><span style=\"color:#010203;background-color:#040506;font-weight:bold\">&lt;&amp;&gt;界e\u{301}"));
    assert!(html.ends_with("end</span></pre>"));
    assert_eq!(html.matches("<span ").count(), 2);
}

#[test]
fn typing_writes_to_the_focused_pane_and_hotkeys_swallow_their_char() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    a.take_effects();
    a.on_event(Event::Char('x'));
    key(&mut a, Key::Enter, Mods::NONE);
    a.on_event(Event::Char('\r'));
    key(&mut a, Key::Char('T'), CTRL_SHIFT);
    a.on_event(Event::Char('\u{14}'));
    assert_eq!(writes(&a.take_effects()), [b"x".to_vec(), b"\r".to_vec()]);
    assert_eq!(a.tab_count(), 2);
    assert_eq!(a.active, 1);
}

#[test]
fn splitting_and_closing_panes() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    key(&mut a, Key::Char('S'), CTRL_SHIFT);
    a.frame(&mut g, false);
    let s = spawns(&a.take_effects());
    assert_eq!(s.len(), 2);
    assert!(s[0].1.cols < 70, "the split halves the width");
    assert_eq!(a.layout.panes.len(), 2);
    let second = s[1].0;
    assert_eq!(a.focused_pane(), Some(second));
    a.exited(second, Some(0));
    assert_eq!(a.layout.panes.len(), 2, "layout refreshes on the next frame");
    a.frame(&mut g, false);
    assert_eq!(a.layout.panes.len(), 1);
    let first = s[0].0;
    a.exited(first, Some(0));
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Close)));
}

#[test]
fn input_typed_before_the_shell_starts_is_sent_after_the_spawn() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.take_effects();
    a.on_event(Event::Char('l'));
    a.on_event(Event::Char('s'));
    assert!(a.take_effects().is_empty());
    a.frame(&mut g, false);
    let e = a.take_effects();
    let at_spawn = e.iter().position(|e| matches!(e, Effect::Spawn { .. })).unwrap();
    let at_write = e.iter().position(|e| matches!(e, Effect::Write { bytes, .. } if bytes == b"ls")).unwrap();
    assert!(at_spawn < at_write);
}

#[test]
fn a_failing_exit_keeps_the_pane_until_a_key_is_pressed() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    let pane = spawns(&a.take_effects())[0].0;
    a.exited(pane, Some(1));
    assert_eq!(a.tab_count(), 1);
    a.on_event(Event::Char('q'));
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Close)));
}

#[test]
fn reconnect_reuses_the_launch_and_preserves_output_in_a_background_pane() {
    let (mut a, mut g) = app();
    a.config.appearance.animations = false;
    a.open_tab(Profile { on_exit: crate::profile::OnExit::Reconnect, ..serial_profile("device:?,8N1") });
    key(&mut a, Key::Enter, Mods::NONE);
    a.frame(&mut g, false);
    let (pane, launch) = spawns(&a.take_effects()).pop().unwrap();
    assert_eq!(launch.serial.as_deref(), Some("device:115200,8N1"));
    let terminal = a.terminal(pane).unwrap().clone();
    terminal.lock().feed(b"prior output\x1b[?2004h");
    a.exited(pane, Some(0));
    let deadline = a.panes[&pane].reconnect_at.unwrap();
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Timer(ms) if (1..=1000).contains(ms))));
    a.open_default_tab();
    a.frame(&mut g, false);
    a.take_effects();
    a.resume_connections(deadline - Duration::from_millis(1));
    assert!(spawns(&a.take_effects()).is_empty());
    a.resume_connections(deadline);
    let effects = a.take_effects();
    assert_eq!(spawns(&effects), [(pane, launch)]);
    assert!(effects.iter().any(|e| matches!(e, Effect::Spawn { reconnecting: true, .. })));
    terminal.lock().feed(b"late output\x1b]0;stale title\x07\x1b[6n");
    a.woken();
    let current = a.terminal(pane).unwrap();
    assert!(current.lock().buffer_text().contains("prior output"));
    assert!(!current.lock().buffer_text().contains("late output"));
    assert!(!current.lock().mode().contains(TermMode::BRACKETED_PASTE));
    assert!(a.panes[&pane].title.is_none());
    assert!(writes(&a.take_effects()).is_empty());
    a.resume_connections(deadline);
    assert!(spawns(&a.take_effects()).is_empty());
}

#[test]
fn a_failed_reconnect_reports_the_error_and_schedules_another_attempt() {
    let (mut a, mut g) = app();
    a.open_tab(Profile { ssh: Some("host".into()), on_exit: crate::profile::OnExit::Reconnect, ..Profile::default() });
    a.frame(&mut g, false);
    let pane = spawns(&a.take_effects())[0].0;
    for _ in 0..2 {
        a.spawn_failed(pane, "connection failed");
        assert!(a.terminal(pane).unwrap().lock().buffer_text().contains("connection failed"));
        a.resume_connections(a.panes[&pane].reconnect_at.unwrap());
        assert_eq!(spawns(&a.take_effects()).len(), 1);
    }
}

#[test]
fn a_key_cancels_reconnect_and_keeps_the_pane() {
    let (mut a, mut g) = app();
    a.open_tab(Profile { socket: Some("tcp://host:23".into()), on_exit: crate::profile::OnExit::Reconnect, ..Profile::default() });
    a.frame(&mut g, false);
    let pane = spawns(&a.take_effects())[0].0;
    a.exited(pane, Some(1));
    let deadline = a.panes[&pane].reconnect_at.unwrap();
    key(&mut a, Key::Char('T'), CTRL_SHIFT);
    a.on_event(Event::Char('\u{14}'));
    a.resume_connections(deadline);
    a.frame(&mut g, false);
    assert_eq!(a.tab_count(), 1);
    assert_eq!(a.focused_pane(), Some(pane));
    assert!(spawns(&a.take_effects()).is_empty());
    assert!(a.panes[&pane].exited.is_some());
    a.on_event(Event::Char('q'));
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Close)));
}

#[test]
fn osc_titles_name_the_tab_and_the_window() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    let pane = spawns(&a.take_effects())[0].0;
    a.panes[&pane].terminal.lock().feed(b"\x1b]0;vim notes.txt\x07");
    a.woken();
    assert_eq!(a.tab_title(0), "vim notes.txt");
    a.frame(&mut g, false);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Title(t) if t == "vim notes.txt")));
}

#[test]
fn the_command_selector_runs_the_chosen_action() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    key(&mut a, Key::Char('P'), CTRL_SHIFT);
    assert!(a.overlay.is_some());
    for c in "split bottom".chars() {
        a.on_event(Event::Char(c));
    }
    key(&mut a, Key::Enter, Mods::NONE);
    assert!(a.overlay.is_none());
    a.frame(&mut g, false);
    assert_eq!(a.layout.panes.len(), 2);
    assert!(a.layout.panes[0].1.h < a.layout.panes[0].1.w, "split to the bottom stacks the panes");
}

#[test]
fn the_tab_bar_is_a_caption_except_on_tabs_and_buttons() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    assert_eq!(a.hit_test(600.0, 10.0), HitArea::Caption);
    assert_eq!(a.hit_test(990.0, 10.0), HitArea::Close);
    assert_eq!(a.hit_test(30.0, 20.0), HitArea::Client);
    assert_eq!(a.hit_test(500.0, 300.0), HitArea::Client);
}

#[test]
fn ctrl_c_copies_a_selection_or_interrupts() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    let pane = spawns(&a.take_effects())[0].0;
    key(&mut a, Key::Char('C'), Mods { ctrl: true, ..Mods::NONE });
    assert_eq!(writes(&a.take_effects()), [vec![3u8]]);
    {
        let mut t = a.panes[&pane].terminal.lock();
        t.feed(b"hello");
        t.select_all();
    }
    key(&mut a, Key::Char('C'), Mods { ctrl: true, ..Mods::NONE });
    let e = a.take_effects();
    assert!(e.iter().any(|e| matches!(e, Effect::SetClipboard(t) if t.starts_with("hello"))));
    assert!(writes(&e).is_empty());
}

#[test]
fn the_scheme_selector_previews_restores_on_escape_and_saves_on_enter() {
    let (mut a, _) = app();
    let before = a.theme.background;
    a.run(Action::SelectColorScheme);
    for c in "dracula".chars() {
        a.on_event(Event::Char(c));
    }
    assert_ne!(a.theme.background, before, "the selection is previewed");
    key(&mut a, Key::Escape, Mods::NONE);
    assert_eq!(a.theme.background, before);
    a.run(Action::SelectColorScheme);
    for c in "dracula".chars() {
        a.on_event(Event::Char(c));
    }
    key(&mut a, Key::Enter, Mods::NONE);
    let saved = a.take_effects().into_iter().find_map(|e| if let Effect::SaveConfig(t) = e { Some(t) } else { None }).unwrap();
    assert!(saved.contains("color-scheme = \"Dracula\""), "{saved}");
}

#[test]
fn new_panes_start_in_the_working_directory_of_the_focused_pane() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    let pane = spawns(&a.take_effects())[0].0;
    a.panes[&pane].terminal.lock().feed(b"\x1b]7;file://pc/D:/work\x07");
    key(&mut a, Key::Char('D'), CTRL_SHIFT);
    a.frame(&mut g, false);
    let s = spawns(&a.take_effects());
    assert_eq!(s[0].1.cwd.as_deref(), Some(r"D:\work"));
}

#[test]
fn a_shell_that_does_not_report_its_directory_is_asked_through_its_process() {
    let (mut a, mut g) = app();
    a.set_cwd_probe(Arc::new(|pid| (pid == 42).then(|| r"E:\probed".to_owned())));
    a.open_default_tab();
    a.frame(&mut g, false);
    let pane = spawns(&a.take_effects())[0].0;
    a.set_pane_pid(pane, 42);
    key(&mut a, Key::Char('D'), CTRL_SHIFT);
    a.frame(&mut g, false);
    assert_eq!(spawns(&a.take_effects())[0].1.cwd.as_deref(), Some(r"E:\probed"));
    a.panes[&pane].terminal.lock().feed(b"\x1b]7;file://pc/D:/work\x07");
    let snapshot = a.snapshot();
    let text = format!("{snapshot:?}");
    assert!(text.contains(r"D:\\work") && text.contains(r"E:\\probed"), "{text}");
}

#[test]
fn native_ssh_duplicate_and_restore_inherit_only_the_remote_cwd() {
    let (mut a, mut g) = app();
    let mut remote = profile("remote");
    remote.ssh = Some("work".into());
    remote.ssh_options = Some(crate::profile::SshOptions { remember_cwd: Some(true), ..Default::default() });
    a.open_default_tab();
    a.frame(&mut g, false);
    a.take_effects();
    a.focused_terminal().unwrap().lock().set_cwd(Some("/local".into()));
    a.open_tab(remote.clone());
    a.frame(&mut g, false);
    assert_eq!(spawns(&a.take_effects())[0].1.cwd, None);
    a.focused_terminal().unwrap().lock().feed(b"\x1b]7;file://work/srv/app\x07");
    key(&mut a, Key::Char('D'), CTRL_SHIFT);
    a.frame(&mut g, false);
    let duplicate = &spawns(&a.take_effects())[0].1;
    assert_eq!(duplicate.cwd.as_deref(), Some("/srv/app"));
    assert_eq!(duplicate.ssh_options.as_deref(), remote.ssh_options.as_ref());
    let (mut restored, mut graphics) = app();
    assert!(restored.restore(&a.snapshot()));
    restored.frame(&mut graphics, false);
    assert!(spawns(&restored.take_effects()).iter().any(|(_, l)| l.ssh == remote.ssh && l.cwd.as_deref() == Some("/srv/app")));
    remote.ssh = Some("elsewhere".into());
    a.open_tab(remote);
    a.frame(&mut g, false);
    assert_eq!(spawns(&a.take_effects())[0].1.cwd, None);
}

#[test]
fn a_saved_system_ssh_folder_is_passed_to_the_remote_shell() {
    let (mut app, _) = app();
    app.set_config(Config::parse("[ssh]\nclient='system'").unwrap());
    let mut profile = crate::ssh::host_profile("build", "ssh");
    profile.cwd = Some("/srv/team folder".into());
    app.open_tab(profile);
    let launch = app.panes[&app.focused_pane().unwrap()].pending.as_ref().unwrap();
    assert_eq!(launch.cwd.as_deref(), Some(r"C:\home"));
    assert_eq!(launch.args, ["-t", "build", "cd '/srv/team folder' && exec \"$SHELL\" -l"]);
}

#[test]
fn wsl_profiles_receive_a_unix_directory_through_their_cd_argument() {
    let wsl = Profile::new("local:wsl:u", "WSL", "wsl.exe", &["-d", "Ubuntu", "--cd", "~"]);
    let (args, cwd) = inherit_cwd(&wsl, Some("/srv/app".into()), Some(r"C:\home".into()));
    assert_eq!(args, ["-d", "Ubuntu", "--cd", "/srv/app"]);
    assert_eq!(cwd.as_deref(), Some(r"C:\home"));
    let saved = Profile { id: "custom:saved".into(), cwd: Some("/saved".into()), ..wsl };
    assert_eq!(dropped_text(&saved, &[r"C:\src".into()]), "/mnt/c/src");
    assert_eq!(inherit_cwd(&saved, None, Some(r"C:\home".into())), (vec!["-d".to_owned(), "Ubuntu".into(), "--cd".into(), "/saved".into()], Some(r"C:\home".into())));
    let cmd = Profile::new("local:cmd", "CMD", "cmd.exe", &[]);
    assert_eq!(inherit_cwd(&cmd, Some("/srv/app".into()), None).1, None);
    assert_eq!(inherit_cwd(&cmd, None, Some("H".into())).1.as_deref(), Some("H"));
}

#[test]
fn hovering_a_link_underlines_it_and_a_plain_click_opens_it() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    let pane = spawns(&a.take_effects())[0].0;
    a.panes[&pane].terminal.lock().feed(b"see https://aster.dev/x ok");
    a.frame(&mut g, false);
    let inner = view::pane_inner(a.layout.panes[0].1, 1.0);
    let (x, y) = (inner.x + 8.0 * 6.5, inner.y + 8.0);
    a.on_event(Event::MouseMove { x, y });
    assert_eq!(a.hover_link.as_ref().map(|l| (l.start, l.end)), Some((4, 23)));
    a.on_event(Event::MouseDown { x, y, button: MouseButton::Left, clicks: 1 });
    a.on_event(Event::MouseUp { x, y, button: MouseButton::Left });
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Open(t) if t == "https://aster.dev/x")));
    assert_eq!(open_target("C:/src/main.rs:12:5"), "C:/src/main.rs");
    assert_eq!(open_target("https://a.b:8080"), "https://a.b:8080");
}

#[test]
fn links_use_the_configured_modifier_at_mouse_down() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    a.focused_terminal().unwrap().lock().feed(b"see https://aster.dev/x ok");
    let ctrl = Mods { ctrl: true, ..Mods::NONE };
    let shift = Mods { shift: true, ..Mods::NONE };
    for (setting, required, wrong) in [("ctrl", ctrl, shift), ("shift", shift, ctrl)] {
        a.set_config(Config::parse(&format!("[terminal]\nlink-modifier = '{setting}'")).unwrap());
        a.frame(&mut g, false);
        let inner = view::pane_inner(a.layout.panes[0].1, 1.0);
        let (x, y) = (inner.x + 8.0 * 6.5, inner.y + 8.0);
        for (down, up, opens) in [(Mods::NONE, required, false), (wrong, wrong, false), (required, Mods::NONE, true)] {
            a.on_event(Event::Modifiers(down));
            a.on_event(Event::MouseDown { x, y, button: MouseButton::Left, clicks: 1 });
            a.on_event(Event::Modifiers(up));
            a.on_event(Event::MouseUp { x, y, button: MouseButton::Left });
            assert_eq!(a.take_effects().iter().any(|e| matches!(e, Effect::Open(t) if t == "https://aster.dev/x")), opens);
        }
    }
}

#[test]
fn executable_path_titles_fall_back_to_the_profile_name() {
    assert!(is_program_path("C:/WINDOWS/System32/OpenSSH/ssh.exe"));
    assert!(!is_program_path("vim notes.txt"));
    assert!(!is_program_path("C:/work"));
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    a.focused_terminal().unwrap().lock().feed(b"\x1b]0;C:/Windows/system32/cmd.exe\x07");
    a.woken();
    assert_eq!(a.tab_title(0), "local:a");
}

#[test]
fn opacity_reaches_the_backgrounds_only_where_the_window_can_show_it() {
    let (mut a, _) = app();
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Surface(crate::config::Backdrop::None))));
    let mut config = Config::default();
    config.appearance.opacity = 0.5;
    config.appearance.backdrop = crate::config::Backdrop::Mica;
    a.set_config(config);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Surface(crate::config::Backdrop::Mica))));
    assert_eq!(a.theme.background.0 >> 24, 0xFF, "an opaque window keeps opaque backgrounds");
    a.set_surface(true, true);
    assert_eq!((a.theme.background.0 >> 24, a.theme.chrome.0 >> 24, a.theme.surface.0 >> 24), (128, 0, 0xFF));
    a.set_surface(true, false);
    assert_eq!(a.theme.chrome.0 >> 24, 128, "without a backdrop the tab bar is as clear as the terminal");
}

#[test]
fn the_enter_that_picks_a_profile_does_not_reach_the_new_tab() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    a.take_effects();
    key(&mut a, Key::Char('E'), CTRL_SHIFT);
    a.on_event(Event::Char('\u{5}'));
    for c in "local:b".chars() {
        a.on_event(Event::Char(c));
    }
    key(&mut a, Key::Enter, Mods::NONE);
    a.on_event(Event::Char('\r'));
    a.frame(&mut g, false);
    let e = a.take_effects();
    assert_eq!(spawns(&e).len(), 1);
    assert!(writes(&e).is_empty(), "{:?}", writes(&e));
}

#[test]
fn a_waiting_transfer_takes_typed_paths_drops_and_escape_instead_of_the_shell() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    let pane = spawns(&a.take_effects())[0].0;
    a.transfer_offered(pane, PaneTransfer::AwaitingFiles);
    for c in r"C:\\notes.txt".chars() {
        a.on_event(Event::Char(c));
    }
    assert_eq!(a.offer(pane), Some(r"C:\\notes.txt"));
    key(&mut a, Key::Enter, Mods::NONE);
    a.on_event(Event::Char('\r'));
    let e = a.take_effects();
    assert!(writes(&e).is_empty(), "nothing reaches the remote shell");
    assert!(e.iter().any(|e| matches!(e, Effect::SendFiles { files, .. } if files == &[std::path::PathBuf::from(r"C:\\notes.txt")])));
    a.on_event(Event::DroppedFiles(vec!["/tmp/a".into(), "/tmp/b".into()]));
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::SendFiles { files, .. } if files.len() == 2)));
    key(&mut a, Key::Escape, Mods::NONE);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::CancelTransfer { .. })));
    assert_eq!(a.offer(pane), None);
    a.on_event(Event::Char('x'));
    assert_eq!(writes(&a.take_effects()), [b"x".to_vec()], "typing goes to the shell again");
}

const CTRL: Mods = Mods { ctrl: true, shift: false, alt: false, meta: false };

fn transfer_pane() -> (App, Gfx<Mono>, PaneId) {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    let pane = spawns(&a.take_effects())[0].0;
    (a, g, pane)
}

#[test]
fn incoming_files_wait_for_enter_before_they_are_received() {
    let (mut a, mut g, pane) = transfer_pane();
    a.transfer_offered(pane, PaneTransfer::Incoming);
    assert_eq!(a.transfer(pane), PaneTransfer::Incoming);
    assert_eq!(a.offer(pane), None, "no path field while files come in");
    a.frame(&mut g, false);
    a.on_event(Event::Char('y'));
    key(&mut a, Key::Enter, Mods::NONE);
    a.on_event(Event::Char('\r'));
    let e = a.take_effects();
    assert!(writes(&e).is_empty(), "nothing reaches the remote side");
    assert!(e.iter().any(|e| matches!(e, Effect::AcceptTransfer { pane: p } if *p == pane)));
    assert_eq!(a.transfer(pane), PaneTransfer::Running);
}

#[test]
fn escape_refuses_incoming_files() {
    let (mut a, _, pane) = transfer_pane();
    a.transfer_offered(pane, PaneTransfer::Incoming);
    key(&mut a, Key::Escape, Mods::NONE);
    let e = a.take_effects();
    assert!(e.iter().any(|e| matches!(e, Effect::CancelTransfer { pane: p } if *p == pane)));
    assert!(!e.iter().any(|e| matches!(e, Effect::AcceptTransfer { .. })));
    assert_eq!(a.transfer(pane), PaneTransfer::None);
    a.on_event(Event::Char('x'));
    assert_eq!(writes(&a.take_effects()), [b"x".to_vec()]);
}

#[test]
fn escape_cancels_a_running_transfer() {
    let (mut a, mut g, pane) = transfer_pane();
    a.transfer_offered(pane, PaneTransfer::Running);
    a.frame(&mut g, false);
    key(&mut a, Key::Escape, Mods::NONE);
    a.on_event(Event::Char('\u{1b}'));
    let e = a.take_effects();
    assert!(writes(&e).is_empty());
    assert!(e.iter().any(|e| matches!(e, Effect::CancelTransfer { pane: p } if *p == pane)));
    assert_eq!(a.transfer(pane), PaneTransfer::None);
}

#[test]
fn ctrl_c_cancels_a_running_transfer_without_reaching_the_remote_side() {
    let (mut a, _, pane) = transfer_pane();
    a.transfer_offered(pane, PaneTransfer::Running);
    key(&mut a, Key::Char('C'), CTRL);
    a.on_event(Event::Char('\u{3}'));
    let e = a.take_effects();
    assert!(writes(&e).is_empty(), "{:?}", writes(&e));
    assert!(e.iter().any(|e| matches!(e, Effect::CancelTransfer { pane: p } if *p == pane)));
}

#[test]
fn other_keys_typing_pastes_and_drops_are_swallowed_while_a_transfer_runs() {
    let (mut a, _, pane) = transfer_pane();
    a.transfer_offered(pane, PaneTransfer::Running);
    for k in [Key::Enter, Key::Backspace, Key::Tab, Key::Up] {
        key(&mut a, k, Mods::NONE);
    }
    for c in "ls\r\t".chars() {
        a.on_event(Event::Char(c));
    }
    a.on_event(Event::Pasted("echo hi".into()));
    a.on_event(Event::DroppedFiles(vec!["/tmp/a".into()]));
    let e = a.take_effects();
    assert!(writes(&e).is_empty(), "{:?}", writes(&e));
    assert!(!e.iter().any(|e| matches!(e, Effect::CancelTransfer { .. } | Effect::SendFiles { .. } | Effect::AcceptTransfer { .. })));
    assert_eq!(a.transfer(pane), PaneTransfer::Running);
}

#[test]
fn the_end_of_a_transfer_removes_the_strip_and_returns_the_keyboard() {
    let (mut a, mut g, pane) = transfer_pane();
    a.transfer_offered(pane, PaneTransfer::Running);
    a.transfer_offered(pane, PaneTransfer::None);
    assert_eq!(a.transfer(pane), PaneTransfer::None);
    a.frame(&mut g, false);
    a.on_event(Event::Char('x'));
    assert_eq!(writes(&a.take_effects()), [b"x".to_vec()]);
}

#[test]
fn awaiting_files_keeps_the_typed_path_when_the_same_state_repeats() {
    let (mut a, mut g, pane) = transfer_pane();
    a.transfer_offered(pane, PaneTransfer::AwaitingFiles);
    a.on_event(Event::Char('a'));
    a.transfer_offered(pane, PaneTransfer::AwaitingFiles);
    assert_eq!(a.offer(pane), Some("a"));
    a.frame(&mut g, false);
    a.transfer_offered(pane, PaneTransfer::Running);
    assert_eq!(a.offer(pane), None);
}

#[test]
fn a_translucent_window_paints_each_background_once_over_a_clear_frame() {
    let (mut a, mut g) = app();
    let mut config = Config::default();
    config.appearance.opacity = 0.6;
    a.set_config(config);
    a.set_surface(true, false);
    a.open_default_tab();
    a.frame(&mut g, false);
    assert_eq!(g.dl.clear.0 >> 24, 0, "the frame starts fully transparent");
    let alpha = (0.6f32 * 255.0).round() as u32;
    let content = a.chrome.content;
    let covering: Vec<_> = g.dl.instances.iter().filter(|i| i.color >> 24 == alpha && i.rect[0] <= content.x && i.rect[1] <= content.y + 1.0 && i.rect[2] >= content.w && i.rect[3] >= content.h).collect();
    assert_eq!(covering.len(), 1, "the terminal background is drawn once");
}

fn select_with(a: &mut App, mods: Mods, from: (usize, usize), to: (usize, usize), clicks: u8) {
    let m = a.metrics.unwrap();
    let inner = view::pane_inner(a.layout.panes[0].1, 1.0);
    let at = |(col, row): (usize, usize), part: f32| (inner.x + m.w * (col as f32 + part), inner.y + m.h * (row as f32 + 0.5));
    let (x, y) = at(from, 0.2);
    a.on_event(Event::Modifiers(mods));
    a.on_event(Event::MouseDown { x, y, button: MouseButton::Left, clicks });
    let (x, y) = at(to, 0.8);
    a.on_event(Event::MouseMove { x, y });
    a.on_event(Event::MouseUp { x, y, button: MouseButton::Left });
    a.on_event(Event::Modifiers(Mods::NONE));
}

fn selected(a: &App) -> String {
    a.focused_terminal().unwrap().lock().selection_text().unwrap_or_default()
}

#[test]
fn alt_drag_selects_a_block_and_shift_click_extends_the_selection() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    a.focused_terminal().unwrap().lock().feed(b"abcdef\r\nghijkl\r\nmnopqr");
    a.frame(&mut g, false);
    select_with(&mut a, Mods { alt: true, ..Mods::NONE }, (1, 0), (3, 2), 1);
    assert_eq!(selected(&a), "bcd\nhij\nnop");
    select_with(&mut a, Mods::NONE, (0, 0), (2, 0), 1);
    assert_eq!(selected(&a), "abc");
    select_with(&mut a, Mods { shift: true, ..Mods::NONE }, (4, 1), (4, 1), 1);
    assert_eq!(selected(&a), "abcdef\nghijk");
}

#[test]
fn double_click_words_stop_at_the_configured_separators() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    a.focused_terminal().unwrap().lock().feed(b"path/to:file.rs end");
    a.frame(&mut g, false);
    select_with(&mut a, Mods::NONE, (3, 0), (3, 0), 2);
    assert_eq!(selected(&a), "path/to:file.rs");
    let mut config = Config::default();
    config.terminal.word_separator = " /:".into();
    a.set_config(config);
    select_with(&mut a, Mods::NONE, (9, 0), (9, 0), 2);
    assert_eq!(selected(&a), "file.rs");
}

#[test]
fn broadcast_input_reaches_every_pane_in_scope_until_its_bar_is_clicked() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.run(Action::SplitRight);
    a.frame(&mut g, false);
    a.open_default_tab();
    a.frame(&mut g, false);
    a.take_effects();
    let targets = |effects: &[Effect]| {
        let mut panes: Vec<PaneId> = effects.iter().filter_map(|e| if let Effect::Write { pane, bytes } = e { (bytes == b"x").then_some(*pane) } else { None }).collect();
        panes.sort();
        panes
    };
    a.run(Action::BroadcastToAllTabs);
    a.on_event(Event::Char('x'));
    assert_eq!(targets(&a.take_effects()).len(), 3);
    a.run(Action::BroadcastToTab);
    a.on_event(Event::Char('x'));
    assert_eq!(targets(&a.take_effects()).len(), 1, "the active tab has one pane");
    a.activate(0);
    a.frame(&mut g, false);
    a.on_event(Event::Char('x'));
    assert_eq!(targets(&a.take_effects()).len(), 2);
    let (_, r) = a.layout.panes[1];
    a.on_event(Event::MouseDown { x: r.x + 20.0, y: r.bottom() - 5.0, button: MouseButton::Left, clicks: 1 });
    a.on_event(Event::MouseUp { x: r.x + 20.0, y: r.bottom() - 5.0, button: MouseButton::Left });
    assert_eq!(a.broadcast, None);
    a.on_event(Event::Char('x'));
    assert_eq!(targets(&a.take_effects()).len(), 1);
}

#[test]
fn background_output_marks_the_tab_and_progress_reaches_the_taskbar() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    a.open_default_tab();
    a.frame(&mut g, false);
    a.take_effects();
    assert!(!a.tab_activity(0));
    a.tabs[0].root.panes().iter().for_each(|p| a.panes[p].terminal.lock().feed(b"build done"));
    assert!(a.tab_activity(0));
    assert!(!a.tab_activity(1));
    a.focused_terminal().unwrap().lock().feed(b"\x1b]9;4;1;30\x07");
    a.frame(&mut g, false);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Progress(Some(p)) if p.percent == 30)));
    a.frame(&mut g, false);
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::Progress(_))), "unchanged progress is not sent again");
    a.activate(0);
    a.frame(&mut g, false);
    assert!(!a.tab_activity(0));
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Progress(None))));
    assert_eq!(a.tab_progress(1).map(|p| p.state), Some(ProgressState::Normal));
}

#[test]
fn focus_follows_the_mouse_only_when_enabled() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.run(Action::SplitRight);
    a.frame(&mut g, false);
    let (left, right) = (a.layout.panes[0], a.layout.panes[1]);
    assert_eq!(a.focused_pane(), Some(right.0));
    a.on_event(Event::MouseMove { x: left.1.x + 10.0, y: left.1.y + 10.0 });
    assert_eq!(a.focused_pane(), Some(right.0));
    let mut config = Config::default();
    config.terminal.focus_follows_mouse = true;
    a.set_config(config);
    a.on_event(Event::MouseMove { x: left.1.x + 10.0, y: left.1.y + 10.0 });
    assert_eq!(a.focused_pane(), Some(left.0));
}

#[test]
fn reporting_a_problem_opens_a_prefilled_issue_with_the_version() {
    let (mut a, _) = app();
    a.run(Action::ReportProblem);
    let url = a.take_effects().into_iter().find_map(|e| if let Effect::Open(u) = e { Some(u) } else { None }).unwrap();
    assert!(url.starts_with("https://github.com/brano-san/Aster/issues/new?body="));
    assert!(url.contains(&format!("Aster%20{}%20on%20{}", env!("CARGO_PKG_VERSION"), std::env::consts::OS)));
    assert!(!url.contains(' ') && !url.contains('\n'));
    a.run(Action::OpenLog);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::OpenLog)));
}

#[test]
fn text_padding_shrinks_the_grid_from_every_edge() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.frame(&mut g, false);
    let pane = a.tabs[0].focus;
    let before = a.panes[&pane].size;
    let mut config = Config::default();
    config.terminal.padding = 40.0;
    a.set_config(config);
    a.frame(&mut g, false);
    let after = a.panes[&pane].size;
    assert!((8..=9).contains(&(before.cols - after.cols)), "68 px less width at 8 px per column");
    assert!(after.rows < before.rows);
}

#[test]
fn panes_use_the_profile_color_scheme_or_the_window_palette() {
    let (mut a, _) = app();
    let scheme = a.schemes.iter().find(|s| s.palette.background != a.palette.background).unwrap();
    let background = scheme.palette.background;
    a.palette.bold_is_bright = !scheme.palette.bold_is_bright;
    let mut themed = profile("custom:themed");
    themed.scheme = Some(scheme.name.clone());
    a.open_tab(themed);
    a.open_default_tab();
    let mut unknown = profile("custom:unknown");
    unknown.scheme = Some("unknown scheme".into());
    a.open_tab(unknown);

    let palette = a.pane_palette(a.tabs[0].focus);
    assert_eq!(palette.background, background);
    assert_ne!(palette.background, a.palette.background);
    assert_eq!(palette.bold_is_bright, a.palette.bold_is_bright);
    assert_eq!(a.pane_palette(a.tabs[1].focus).background, a.palette.background);
    assert_eq!(a.pane_palette(a.tabs[2].focus).background, a.palette.background);
}

#[test]
fn launching_a_settings_profile_by_keyboard_or_mouse_starts_it_in_the_background() {
    let (mut a, mut gfx) = app();
    a.frame(&mut gfx, false);
    a.select_profile_settings("local:b");
    a.frame(&mut gfx, false);
    for mouse in [false, true] {
        if mouse {
            let page = a.settings.as_ref().unwrap();
            let row = a.settings_geometry().unwrap().rows.into_iter().find(|r| r.kind == crate::settings::RowKind::Item(page.selected)).unwrap();
            let button = crate::profiles_page::layout::row_action(row.rect, a.ui());
            a.on_event(Event::MouseDown { x: button.x + button.w / 2.0, y: button.y + button.h / 2.0, button: MouseButton::Left, clicks: 1 });
        } else {
            a.on_event(Event::Key { key: Key::F(5), mods: Mods::NONE, pressed: true });
        }
        a.frame(&mut gfx, false);
        let launched = spawns(&a.take_effects());
        assert_eq!(launched.len(), 1, "mouse launch = {mouse}");
        assert_eq!(a.panes[&launched[0].0].source_profile.id, "local:b");
        assert!(launched[0].1.cols > 2);
        assert_eq!(a.focused_pane(), Some(SETTINGS_PANE));
        assert!(a.settings_open());
    }
}

#[test]
fn mouse_reporting_wins_except_for_bound_modified_wheel_chords() {
    let (mut a, mut g) = app();
    let mut config = a.config().clone();
    config.hotkeys.insert("zoom-in".to_owned(), vec!["MouseMiddle".to_owned(), "WheelDown".to_owned(), "Ctrl+WheelUp".to_owned()]);
    a.set_config(config);
    a.open_default_tab();
    a.frame(&mut g, false);
    let pane = a.focused_pane().unwrap();
    let (_, rect) = a.layout.panes[0];
    let x = rect.x + 10.0;
    let y = rect.y + 10.0;
    let click = Event::MouseDown { x, y, button: MouseButton::Middle, clicks: 1 };
    let up = Event::MouseUp { x, y, button: MouseButton::Middle };
    a.on_event(click.clone());
    a.on_event(up.clone());
    let zoomed = a.zoom;
    a.panes[&pane].terminal.lock().feed(b"\x1b[?1000h\x1b[?1006h");
    a.take_effects();

    a.on_event(click);
    a.on_event(up);
    a.on_event(Event::Wheel { dx: 0.0, dy: -1.0 });
    assert_eq!(a.zoom, zoomed);
    let effects = a.take_effects();
    assert!(effects.iter().any(|e| matches!(e, Effect::Write { bytes, .. } if bytes.starts_with(b"\x1b[<1;"))));
    assert!(effects.iter().any(|e| matches!(e, Effect::Write { bytes, .. } if bytes.starts_with(b"\x1b[<65;"))));

    a.on_event(Event::Modifiers(Mods { ctrl: true, ..Mods::NONE }));
    a.on_event(Event::Wheel { dx: 0.0, dy: 1.0 });
    a.on_event(Event::Modifiers(Mods::NONE));
    assert!(a.zoom > zoomed);
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::Write { .. })));
}

#[test]
fn modifier_only_shortcut_does_not_fire_after_typing_or_focus_loss() {
    let (mut a, _) = app();
    let mut config = a.config().clone();
    config.hotkeys.insert("new-tab".to_owned(), vec!["Ctrl-Shift".to_owned()]);
    a.set_config(config);
    a.open_default_tab();
    let mods = Mods { ctrl: true, shift: true, ..Mods::NONE };
    a.on_event(Event::Modifiers(mods));
    a.on_event(Event::Key { key: Key::Char('X'), mods, pressed: true });
    a.on_event(Event::Modifiers(Mods::NONE));
    assert_eq!(a.tabs.len(), 1);
    a.on_event(Event::Modifiers(mods));
    a.on_event(Event::Focus(false));
    a.on_event(Event::Modifiers(Mods::NONE));
    assert_eq!(a.tabs.len(), 1);
    a.on_event(Event::Focus(true));
    a.on_event(Event::Modifiers(mods));
    a.on_event(Event::Modifiers(Mods::NONE));
    assert_eq!(a.tabs.len(), 2);
}

#[test]
fn released_modifier_binding_commits_an_mru_switch_immediately() {
    let (mut a, _) = app();
    let mut config = a.config().clone();
    config.hotkeys.insert("next-mru-tab".to_owned(), vec!["Ctrl-Shift".to_owned()]);
    a.set_config(config);
    a.open_default_tab();
    a.open_default_tab();
    assert_eq!(a.active, 1);
    a.on_event(Event::Modifiers(Mods { ctrl: true, shift: true, ..Mods::NONE }));
    a.on_event(Event::Modifiers(Mods::NONE));
    assert_eq!(a.active, 0);
    assert!(a.mru.selection().is_none());
}
