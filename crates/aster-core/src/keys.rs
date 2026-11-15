use crate::input::{Key, Mods, MouseButton};
use aster_vt::TermMode;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Encoded {
    pub bytes: Vec<u8>,
    pub swallows_char: bool,
}

fn modifier(m: Mods) -> u8 {
    1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.ctrl)
}

fn cursor(letter: u8, m: Mods, mode: TermMode) -> Vec<u8> {
    let n = modifier(m);
    if n > 1 {
        format!("\x1b[1;{n}{}", letter as char).into_bytes()
    } else if mode.contains(TermMode::APP_CURSOR) {
        vec![0x1b, b'O', letter]
    } else {
        vec![0x1b, b'[', letter]
    }
}

fn tilde(code: u8, m: Mods) -> Vec<u8> {
    let n = modifier(m);
    if n > 1 {
        format!("\x1b[{code};{n}~").into_bytes()
    } else {
        format!("\x1b[{code}~").into_bytes()
    }
}

fn function(n: u8, m: Mods) -> Option<Vec<u8>> {
    let ss3 = |letter: u8| {
        let k = modifier(m);
        if k > 1 {
            format!("\x1b[1;{k}{}", letter as char).into_bytes()
        } else {
            vec![0x1b, b'O', letter]
        }
    };
    Some(match n {
        1 => ss3(b'P'),
        2 => ss3(b'Q'),
        3 => ss3(b'R'),
        4 => ss3(b'S'),
        5 => tilde(15, m),
        6 => tilde(17, m),
        7 => tilde(18, m),
        8 => tilde(19, m),
        9 => tilde(20, m),
        10 => tilde(21, m),
        11 => tilde(23, m),
        12 => tilde(24, m),
        _ => return None,
    })
}

fn alt(prefix: bool, mut bytes: Vec<u8>) -> Vec<u8> {
    if prefix {
        bytes.insert(0, 0x1b);
    }
    bytes
}

pub fn encode_key(key: Key, m: Mods, mode: TermMode) -> Option<Encoded> {
    let produced = |bytes: Vec<u8>| Some(Encoded { bytes, swallows_char: true });
    let silent = |bytes: Vec<u8>| Some(Encoded { bytes, swallows_char: false });
    match key {
        Key::Up => silent(cursor(b'A', m, mode)),
        Key::Down => silent(cursor(b'B', m, mode)),
        Key::Right => silent(cursor(b'C', m, mode)),
        Key::Left => silent(cursor(b'D', m, mode)),
        Key::Home => silent(cursor(b'H', m, mode)),
        Key::End => silent(cursor(b'F', m, mode)),
        Key::Insert => silent(tilde(2, m)),
        Key::Delete => silent(tilde(3, m)),
        Key::PageUp => silent(tilde(5, m)),
        Key::PageDown => silent(tilde(6, m)),
        Key::F(n) => function(n, m).and_then(silent),
        Key::Enter => {
            let base = if mode.contains(TermMode::LINE_FEED_NEW_LINE) { b"\r\n".to_vec() } else { b"\r".to_vec() };
            produced(alt(m.alt, base))
        }
        Key::Backspace => produced(alt(m.alt, vec![if m.ctrl { 0x08 } else { 0x7f }])),
        Key::Tab if m.shift => produced(b"\x1b[Z".to_vec()),
        Key::Tab => produced(alt(m.alt, vec![b'\t'])),
        Key::Escape => produced(alt(m.alt, vec![0x1b])),
        Key::Space if m.ctrl => produced(alt(m.alt, vec![0])),
        Key::Char('2') if m.ctrl && m.shift => produced(vec![0]),
        Key::Char('/') if m.ctrl && !m.alt => produced(vec![0x1f]),
        _ => None,
    }
}

pub fn encode_alt_char(c: char) -> Vec<u8> {
    let mut out = vec![0x1b];
    let mut buf = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press(MouseButton),
    Release(MouseButton),
    Motion(Option<MouseButton>),
    WheelUp,
    WheelDown,
}

pub fn encode_mouse(action: MouseAction, col: usize, line: usize, m: Mods, mode: TermMode) -> Option<Vec<u8>> {
    let motion_ok = match action {
        MouseAction::Motion(Some(_)) => mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION),
        MouseAction::Motion(None) => mode.contains(TermMode::MOUSE_MOTION),
        _ => mode.intersects(TermMode::MOUSE_MODE),
    };
    if !motion_ok {
        return None;
    }
    let button = |b: MouseButton| match b {
        MouseButton::Left => Some(0u8),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        _ => None,
    };
    let sgr = mode.contains(TermMode::SGR_MOUSE);
    let (mut code, pressed) = match action {
        MouseAction::Press(b) => (button(b)?, true),
        MouseAction::Release(b) => (if sgr { button(b)? } else { 3 }, false),
        MouseAction::Motion(b) => (b.and_then(button).unwrap_or(3) + 32, true),
        MouseAction::WheelUp => (64, true),
        MouseAction::WheelDown => (65, true),
    };
    code += 4 * u8::from(m.shift) + 8 * u8::from(m.alt) + 16 * u8::from(m.ctrl);
    let (x, y) = (col + 1, line + 1);
    if sgr {
        return Some(format!("\x1b[<{code};{x};{y}{}", if pressed { 'M' } else { 'm' }).into_bytes());
    }
    if x > 223 || y > 223 {
        return None;
    }
    Some(vec![0x1b, b'[', b'M', 32 + code, 32 + x as u8, 32 + y as u8])
}

pub fn paste_bytes(text: &str, mode: TermMode, trim: bool) -> Vec<u8> {
    let mut text = text.replace("\r\n", "\r").replace('\n', "\r");
    if trim {
        let trimmed = text.trim_end_matches([' ', '\t', '\r']);
        text = if trimmed.contains('\r') { trimmed.to_owned() } else { trimmed.trim_start_matches([' ', '\t']).to_owned() };
    }
    if mode.contains(TermMode::BRACKETED_PASTE) {
        let clean = text.replace("\x1b[201~", "");
        return format!("\x1b[200~{clean}\x1b[201~").into_bytes();
    }
    text.into_bytes()
}

#[cfg(test)]
#[path = "../test/keys.rs"]
mod tests;
