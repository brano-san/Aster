use super::*;

const CTRL: Mods = Mods { ctrl: true, shift: false, alt: false, meta: false };
const ALT: Mods = Mods { ctrl: false, shift: false, alt: true, meta: false };

#[test]
fn ctrl_letters_become_control_characters_and_alt_text_gets_an_escape() {
    assert_eq!(chars_for(Key::Char('C'), CTRL, Some("c")), [Event::Char('\u{3}')]);
    assert_eq!(chars_for(Key::Char('['), CTRL, None), [Event::Char('\u{1b}')]);
    assert!(chars_for(Key::Char('1'), CTRL, Some("1")).is_empty());
    assert_eq!(chars_for(Key::Char('X'), ALT, Some("x")), [Event::AltChar('x')]);
    assert_eq!(chars_for(Key::Char('A'), Mods::NONE, Some("ф")), [Event::Char('ф')]);
    let altgr = Mods { ctrl: true, alt: true, ..Mods::NONE };
    assert_eq!(chars_for(Key::Char('Q'), altgr, Some("@")), [Event::Char('@')]);
}

#[test]
fn keys_map_by_us_position_so_hotkeys_work_on_any_layout() {
    let k = map_key(PhysicalKey::Code(KeyCode::KeyT), &WinitKey::Character("е".into()));
    assert_eq!(k, Key::Char('T'));
    assert_eq!(map_key(PhysicalKey::Code(KeyCode::Equal), &WinitKey::Character("=".into())), Key::Char('='));
    assert_eq!(map_key(PhysicalKey::Code(KeyCode::F11), &WinitKey::Named(NamedKey::F11)), Key::F(11));
    assert_eq!(map_key(PhysicalKey::Code(KeyCode::ShiftLeft), &WinitKey::Named(NamedKey::Shift)), Key::Other);
    assert_eq!(map_key(PhysicalKey::Code(KeyCode::ControlRight), &WinitKey::Named(NamedKey::Control)), Key::Control);
}

#[test]
fn resize_edges_and_corners() {
    let size = (800.0, 600.0);
    assert_eq!(resize_edge(2.0, 300.0, size, 5.0), Some(ResizeDirection::West));
    assert_eq!(resize_edge(2.0, 2.0, size, 5.0), Some(ResizeDirection::NorthWest));
    assert_eq!(resize_edge(798.0, 598.0, size, 5.0), Some(ResizeDirection::SouthEast));
    assert_eq!(resize_edge(400.0, 300.0, size, 5.0), None);
}
