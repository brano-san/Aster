use super::*;
use crate::app::Effect;
use crate::config::{Bell, Config};
use crate::input::{Event, Mods, MouseButton};
use crate::profile::Profile;
use std::sync::Arc;

fn app() -> App {
    let mut a = App::new(Config::default(), vec![Profile::new("local:a", "A", "a.exe", &[])], true, None, Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    a.chrome = crate::view::chrome(1000.0, 700.0, 1.0, 1);
    a.open_default_tab();
    let pane = a.focused_pane().unwrap();
    a.panes.get_mut(&pane).unwrap().pending = None;
    a.take_effects();
    a
}

fn writes(a: &mut App) -> Vec<Vec<u8>> {
    a.take_effects().into_iter().filter_map(|e| if let Effect::Write { bytes, .. } = e { Some(bytes) } else { None }).collect()
}

fn key(a: &mut App, k: Key) {
    a.on_event(Event::Key { key: k, mods: Mods::NONE, pressed: true });
}

#[test]
fn a_single_line_pastes_at_once() {
    let mut a = app();
    a.paste("echo hi\n".into());
    assert!(a.paste_confirm().is_none());
    assert_eq!(writes(&mut a), [b"echo hi".to_vec()]);
}

#[test]
fn several_lines_wait_for_confirmation() {
    let mut a = app();
    a.paste("rm a\r\nrm b\n".into());
    assert_eq!(a.paste_confirm().map(|c| c.lines().len()), Some(2));
    assert!(writes(&mut a).is_empty());
    key(&mut a, Key::Enter);
    assert_eq!(writes(&mut a), [b"rm a\rrm b".to_vec()]);
    assert!(a.paste_confirm().is_none());
}

#[test]
fn the_text_can_be_joined_into_one_line_or_dropped() {
    let mut a = app();
    a.paste("ls\n  -la\n".into());
    key(&mut a, Key::Right);
    key(&mut a, Key::Enter);
    assert_eq!(writes(&mut a), [b"ls -la".to_vec()]);
    a.paste("a\nb".into());
    key(&mut a, Key::Escape);
    assert!(writes(&mut a).is_empty());
    assert!(a.paste_confirm().is_none());
}

#[test]
fn buttons_respond_to_clicks() {
    let mut a = app();
    a.paste("a\nb".into());
    let geo = a.paste_geometry().unwrap();
    let r = geo.buttons[0];
    a.on_event(Event::MouseDown { x: r.x + 3.0, y: r.y + 3.0, button: MouseButton::Left, clicks: 1 });
    assert_eq!(writes(&mut a), [b"a\rb".to_vec()]);
}

#[test]
fn programs_with_bracketed_paste_get_the_text_without_asking() {
    let mut a = app();
    let pane = a.focused_pane().unwrap();
    a.panes[&pane].terminal.lock().feed(b"\x1b[?2004h");
    a.paste("a\nb".into());
    assert!(a.paste_confirm().is_none());
    assert_eq!(writes(&mut a), [b"\x1b[200~a\rb\x1b[201~".to_vec()]);
}

#[test]
fn the_warning_can_be_turned_off() {
    let mut a = app();
    let mut config = Config::default();
    config.terminal.warn_on_multiline_paste = false;
    a.set_config(config);
    a.paste("a\nb".into());
    assert!(a.paste_confirm().is_none());
}

#[test]
fn trailing_newlines_alone_do_not_make_text_multiline() {
    assert_eq!(line_count("echo hi\r\n\n  "), 1);
    assert_eq!(one_line("a \n\n b\r\n"), "a b");
}

#[test]
fn the_bell_flashes_the_pane_once_per_burst_or_asks_for_a_sound() {
    let mut a = app();
    let pane = a.focused_pane().unwrap();
    a.panes[&pane].terminal.lock().feed(b"\x07\x07\x07");
    a.woken();
    assert!(a.bell_flash(pane));
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::Beep)));
    let mut config = Config::default();
    config.terminal.bell = Bell::Audible;
    a.set_config(config);
    a.last_bell_reset();
    a.panes[&pane].terminal.lock().feed(b"\x07\x07");
    a.woken();
    assert_eq!(a.take_effects().iter().filter(|e| matches!(e, Effect::Beep)).count(), 1);
}
