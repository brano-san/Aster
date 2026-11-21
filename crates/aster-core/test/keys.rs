use super::*;

const CTRL: Mods = Mods { ctrl: true, shift: false, alt: false, meta: false };
const SHIFT: Mods = Mods { ctrl: false, shift: true, alt: false, meta: false };
const ALT: Mods = Mods { ctrl: false, shift: false, alt: true, meta: false };

fn bytes(key: Key, m: Mods, mode: TermMode) -> Vec<u8> {
    encode_key(key, m, mode).map(|e| e.bytes).unwrap_or_default()
}

#[test]
fn cursor_keys_follow_application_mode_and_modifiers() {
    assert_eq!(bytes(Key::Up, Mods::NONE, TermMode::empty()), b"\x1b[A");
    assert_eq!(bytes(Key::Up, Mods::NONE, TermMode::APP_CURSOR), b"\x1bOA");
    assert_eq!(bytes(Key::Left, CTRL, TermMode::APP_CURSOR), b"\x1b[1;5D");
    assert_eq!(bytes(Key::Right, SHIFT, TermMode::empty()), b"\x1b[1;2C");
    assert_eq!(bytes(Key::Home, Mods::NONE, TermMode::empty()), b"\x1b[H");
    assert_eq!(bytes(Key::Delete, Mods::NONE, TermMode::empty()), b"\x1b[3~");
    assert_eq!(bytes(Key::PageUp, CTRL, TermMode::empty()), b"\x1b[5;5~");
}

#[test]
fn function_keys() {
    assert_eq!(bytes(Key::F(1), Mods::NONE, TermMode::empty()), b"\x1bOP");
    assert_eq!(bytes(Key::F(4), SHIFT, TermMode::empty()), b"\x1b[1;2S");
    assert_eq!(bytes(Key::F(5), Mods::NONE, TermMode::empty()), b"\x1b[15~");
    assert_eq!(bytes(Key::F(12), CTRL, TermMode::empty()), b"\x1b[24;5~");
    assert!(encode_key(Key::F(13), Mods::NONE, TermMode::empty()).is_none());
}

#[test]
fn keys_that_also_produce_a_char_swallow_it() {
    let e = encode_key(Key::Backspace, Mods::NONE, TermMode::empty()).unwrap();
    assert_eq!((e.bytes.as_slice(), e.swallows_char), (&b"\x7f"[..], true));
    assert_eq!(bytes(Key::Backspace, CTRL, TermMode::empty()), b"\x08");
    assert_eq!(bytes(Key::Backspace, ALT, TermMode::empty()), b"\x1b\x7f");
    assert_eq!(bytes(Key::Enter, Mods::NONE, TermMode::empty()), b"\r");
    assert_eq!(bytes(Key::Enter, Mods::NONE, TermMode::LINE_FEED_NEW_LINE), b"\r\n");
    assert_eq!(bytes(Key::Tab, SHIFT, TermMode::empty()), b"\x1b[Z");
    assert_eq!(bytes(Key::Space, CTRL, TermMode::empty()), b"\0");
    assert_eq!(bytes(Key::Char('/'), CTRL, TermMode::empty()), b"\x1f");
    assert!(encode_key(Key::Char('a'), Mods::NONE, TermMode::empty()).is_none());
    assert!(!encode_key(Key::Up, Mods::NONE, TermMode::empty()).unwrap().swallows_char);
}

#[test]
fn alt_chars_get_an_escape_prefix() {
    assert_eq!(encode_alt_char('x'), b"\x1bx");
    assert_eq!(encode_alt_char('ж'), "\x1bж".as_bytes());
}

#[test]
fn mouse_reports_in_sgr_and_legacy_forms() {
    let sgr = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
    assert_eq!(encode_mouse(MouseAction::Press(MouseButton::Left), 0, 0, Mods::NONE, sgr).unwrap(), b"\x1b[<0;1;1M");
    assert_eq!(encode_mouse(MouseAction::Release(MouseButton::Right), 4, 2, CTRL, sgr).unwrap(), b"\x1b[<18;5;3m");
    assert_eq!(encode_mouse(MouseAction::WheelDown, 0, 0, Mods::NONE, sgr).unwrap(), b"\x1b[<65;1;1M");
    let legacy = TermMode::MOUSE_REPORT_CLICK;
    assert_eq!(encode_mouse(MouseAction::Press(MouseButton::Middle), 1, 1, Mods::NONE, legacy).unwrap(), [0x1b, b'[', b'M', 33, 34, 34]);
    assert_eq!(encode_mouse(MouseAction::Release(MouseButton::Middle), 1, 1, Mods::NONE, legacy).unwrap(), [0x1b, b'[', b'M', 35, 34, 34]);
    assert_eq!(encode_mouse(MouseAction::Press(MouseButton::Left), 0, 0, Mods::NONE, TermMode::empty()), None);
    assert_eq!(encode_mouse(MouseAction::Motion(None), 0, 0, Mods::NONE, legacy), None, "plain clicks mode does not report motion");
    assert_eq!(encode_mouse(MouseAction::Motion(Some(MouseButton::Left)), 0, 0, Mods::NONE, TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE).unwrap(), b"\x1b[<32;1;1M");
}

#[test]
fn paste_normalizes_newlines_trims_and_brackets() {
    assert_eq!(paste_bytes("a\r\nb\nc", TermMode::empty(), false), b"a\rb\rc");
    assert_eq!(paste_bytes("  ls -la \n", TermMode::empty(), true), b"ls -la");
    assert_eq!(paste_bytes("a\nb\n", TermMode::empty(), true), b"a\rb", "multi-line text keeps its inner lines");
    assert_eq!(paste_bytes("x\x1b[201~y", TermMode::BRACKETED_PASTE, false), b"\x1b[200~xy\x1b[201~");
}

#[test]
fn extended_mouse_buttons_and_horizontal_wheel_are_reported_in_sgr_mode() {
    let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
    assert_eq!(encode_mouse(MouseAction::Press(MouseButton::Back), 0, 0, Mods::NONE, mode).unwrap(), b"\x1b[<128;1;1M");
    assert_eq!(encode_mouse(MouseAction::Release(MouseButton::Forward), 0, 0, Mods::NONE, mode).unwrap(), b"\x1b[<129;1;1m");
    assert_eq!(encode_mouse(MouseAction::WheelLeft, 0, 0, Mods::NONE, mode).unwrap(), b"\x1b[<66;1;1M");
}
