use super::*;
use crate::app::Effect;
use crate::config::{Bell, Config};
use crate::input::{Event, Mods, MouseButton};
use crate::profile::Profile;
use std::sync::Arc;

fn app() -> App {
    let mut a = App::new(Config::default(), vec![Profile::new("local:a", "A", "a.exe", &[])], true, None, Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    a.open_default_tab();
    a.chrome = a.tab_chrome();
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
fn paste_replaces_each_line_break_before_warning_trimming_and_bracketing() {
    let mut a = app();
    a.set_config(Config::parse("[terminal]\npaste-newlines-as-spaces = true\ntrim-whitespace-on-paste = false").unwrap());
    a.on_event(Event::Pasted(" a\r\n\n b\rc\n".into()));
    assert!(a.paste_confirm().is_none());
    assert_eq!(writes(&mut a), [b" a   b c ".to_vec()]);
    a.focused_terminal().unwrap().lock().feed(b"\x1b[?2004h");
    a.set_config(Config::parse("[terminal]\npaste-newlines-as-spaces = true").unwrap());
    a.on_event(Event::Pasted(" a\r\n\n b\rc\n".into()));
    assert_eq!(writes(&mut a), [b"\x1b[200~a   b c\x1b[201~".to_vec()]);
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
fn mixed_line_endings_are_shown_and_joined_in_the_confirmation() {
    let mut a = app();
    a.paste("first\rsecond\r\nthird\nfourth".into());
    assert_eq!(a.paste_confirm().unwrap().lines(), ["first", "second", "third", "fourth"]);
    key(&mut a, Key::Right);
    key(&mut a, Key::Enter);
    assert_eq!(writes(&mut a), [b"first second third fourth".to_vec()]);
}

#[test]
fn paste_normalizes_crlf_even_when_trimming_is_disabled() {
    let mut a = app();
    a.set_config(Config::parse("[terminal]\ntrim-whitespace-on-paste = false\nwarn-on-multiline-paste = false").unwrap());
    a.paste(" a\r\nb\r\n".into());
    assert_eq!(writes(&mut a), [b" a\rb\r".to_vec()]);
}

#[test]
fn disabling_bracketed_paste_restores_the_multiline_warning() {
    let mut a = app();
    a.set_config(Config::parse("[terminal]\nbracketed-paste = false").unwrap());
    a.focused_terminal().unwrap().lock().feed(b"\x1b[?2004h");
    a.paste("a\rb".into());
    assert!(a.paste_confirm().is_some());
    key(&mut a, Key::Enter);
    assert_eq!(writes(&mut a), [b"a\rb".to_vec()]);
}

#[test]
fn audible_bell_with_animations_off_requests_attention_if_sound_is_unavailable() {
    let mut a = app();
    a.set_config(Config::parse("[appearance]\nanimations = false\n[terminal]\nbell = 'audible'").unwrap());
    let pane = a.focused_pane().unwrap();
    for focused in [true, false] {
        a.on_event(Event::Focus(focused));
        a.last_bell_reset();
        a.terminal(pane).unwrap().lock().feed(b"\x07");
        a.woken();
        a.flash_bell();
        let effects = a.take_effects();
        assert!(effects.iter().any(|e| matches!(e, Effect::Beep)));
        assert_eq!(effects.iter().filter(|e| matches!(e, Effect::Attention)).count(), 1);
        assert_eq!(a.bell_alpha(pane, a.clock()), 0.0);
    }
}

#[test]
fn background_bell_attention_can_be_disabled_independently_of_sound() {
    let mut a = app();
    a.on_event(Event::Focus(false));
    for enabled in [false, true] {
        a.set_config(Config::parse(&format!("[terminal]\nbell = 'audible'\nbell-attention = {enabled}")).unwrap());
        a.last_bell_reset();
        a.focused_terminal().unwrap().lock().feed(b"\x07");
        a.woken();
        let effects = a.take_effects();
        assert!(effects.iter().any(|e| matches!(e, Effect::Beep)));
        assert_eq!(effects.iter().any(|e| matches!(e, Effect::Attention)), enabled);
    }
}

#[test]
fn the_bell_flashes_the_pane_once_per_burst_or_asks_for_a_sound() {
    let mut a = app();
    let pane = a.focused_pane().unwrap();
    a.panes[&pane].terminal.lock().feed(b"\x07\x07\x07");
    a.woken();
    assert!(a.bell_alpha(pane, a.clock()) > 0.0);
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::Beep)));
    let mut config = Config::default();
    config.terminal.bell = Bell::Audible;
    a.set_config(config);
    a.last_bell_reset();
    a.panes[&pane].terminal.lock().feed(b"\x07\x07");
    a.woken();
    assert_eq!(a.take_effects().iter().filter(|e| matches!(e, Effect::Beep)).count(), 1);
}
