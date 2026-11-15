use super::*;
use crate::config::Config;
use aster_gfx::text::{FontMetrics, FontStyle, GlyphBitmap, GlyphKey, Gfx, ShapedGlyph, ShapedText, TextBackend};

struct Mono;

impl TextBackend for Mono {
    fn shape(&mut self, text: &str, _: FontStyle, out: &mut ShapedText) {
        out.glyphs.clear();
        for (i, (b, c)) in text.char_indices().enumerate() {
            out.glyphs.push(ShapedGlyph { font: 0, id: c as u16, x: i as f32 * 8.0, y: 0.0, cluster: b as u32 });
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

fn writes(effects: &[Effect]) -> Vec<Vec<u8>> {
    effects.iter().filter_map(|e| if let Effect::Write { bytes, .. } = e { Some(bytes.clone()) } else { None }).collect()
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
fn wsl_profiles_receive_a_unix_directory_through_their_cd_argument() {
    let wsl = Profile::new("local:wsl:u", "WSL", "wsl.exe", &["-d", "Ubuntu", "--cd", "~"]);
    let (args, cwd) = inherit_cwd(&wsl, Some("/srv/app".into()), Some(r"C:\home".into()));
    assert_eq!(args, ["-d", "Ubuntu", "--cd", "/srv/app"]);
    assert_eq!(cwd.as_deref(), Some(r"C:\home"));
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
