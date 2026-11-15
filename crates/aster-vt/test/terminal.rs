use super::*;
use std::sync::Mutex;

fn term(cols: usize, rows: usize) -> (Terminal, Arc<Mutex<Vec<VtEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink_events = events.clone();
    let sink: Sink = Arc::new(move |e| sink_events.lock().unwrap().push(e));
    (Terminal::new(Size::new(cols, rows), Options::default(), sink), events)
}

#[test]
fn prints_text_and_wraps_lines() {
    let (mut t, _) = term(5, 3);
    t.feed(b"hello world");
    assert_eq!(t.screen_text(), "hello\n worl\nd");
}

#[test]
fn carriage_return_and_newline_move_the_cursor() {
    let (mut t, _) = term(10, 3);
    t.feed(b"ab\r\ncd");
    assert_eq!(t.screen_text(), "ab\ncd");
    let c = t.cursor();
    assert_eq!((c.line, c.col), (1, 2));
}

#[test]
fn osc_title_reaches_the_sink() {
    let (mut t, events) = term(10, 3);
    t.feed(b"\x1b]0;my title\x07");
    assert_eq!(events.lock().unwrap().as_slice(), [VtEvent::Title("my title".into())]);
}

#[test]
fn device_attributes_request_is_answered_through_the_sink() {
    let (mut t, events) = term(10, 3);
    t.feed(b"\x1b[c");
    let got = events.lock().unwrap().clone();
    assert!(matches!(got.as_slice(), [VtEvent::Write(bytes)] if bytes.starts_with(b"\x1b[?")), "{got:?}");
}

#[test]
fn bell_and_osc52_are_forwarded() {
    let (mut t, events) = term(10, 3);
    t.feed(b"\x07\x1b]52;c;aGk=\x07");
    assert_eq!(events.lock().unwrap().as_slice(), [VtEvent::Bell, VtEvent::Clipboard("hi".into())]);
}

#[test]
fn scrollback_keeps_lines_that_left_the_screen() {
    let (mut t, _) = term(10, 2);
    t.feed(b"1\r\n2\r\n3\r\n4");
    assert_eq!(t.screen_text(), "3\n4");
    assert_eq!(t.history(), 2);
    t.scroll(Scroll::Delta(2));
    assert_eq!(t.screen_text(), "1\n2");
    assert_eq!(t.cursor().shape, CursorShape::Hidden, "cursor is below the viewport");
}

#[test]
fn selection_text_follows_the_selected_cells() {
    let (mut t, _) = term(10, 2);
    t.feed(b"hello\r\nworld");
    t.start_selection(SelectionType::Simple, t.viewport_point(0, 1), Side::Left);
    t.update_selection(t.viewport_point(1, 2), Side::Right);
    assert_eq!(t.selection_text().as_deref(), Some("ello\nwor"));
    t.clear_selection();
    assert!(!t.has_selection());
}

#[test]
fn resize_reflows_wrapped_lines() {
    let (mut t, _) = term(4, 4);
    t.feed(b"abcdefgh");
    assert_eq!(t.screen_text(), "abcd\nefgh");
    t.resize(Size::new(8, 4));
    assert_eq!(t.screen_text(), "abcdefgh");
}

#[test]
fn modes_reflect_escape_sequences() {
    let (mut t, _) = term(10, 3);
    assert!(!t.mode().contains(TermMode::BRACKETED_PASTE));
    t.feed(b"\x1b[?2004h\x1b[?1049h");
    assert!(t.mode().contains(TermMode::BRACKETED_PASTE));
    assert!(t.mode().contains(TermMode::ALT_SCREEN));
}

#[test]
fn palette_resolves_named_indexed_and_true_colors() {
    let p = Palette::ASTER_DARK;
    let (mut t, _) = term(10, 2);
    t.feed(b"\x1b[31ma\x1b[1;32mb\x1b[38;5;196mc\x1b[38;2;1;2;3md\x1b[7me");
    let mut cells = Vec::new();
    t.visible(|v| {
        if v.line == 0 && v.col < 5 {
            cells.push(p.cell(v.cell, t.colors()));
        }
    });
    assert_eq!(cells[0].0, p.ansi[1]);
    assert_eq!(cells[1].0, p.ansi[10], "bold green is drawn bright");
    assert_eq!(cells[2].0, Rgb { r: 255, g: 0, b: 0 });
    assert_eq!(cells[3].0, Rgb { r: 1, g: 2, b: 3 });
    assert_eq!(cells[4], (p.background, Rgb { r: 1, g: 2, b: 3 }), "inverse swaps colors");
}

#[test]
fn palette_cube_and_gray_ramp() {
    let p = Palette::default();
    assert_eq!(p.indexed(16), Rgb { r: 0, g: 0, b: 0 });
    assert_eq!(p.indexed(231), Rgb { r: 255, g: 255, b: 255 });
    assert_eq!(p.indexed(232), Rgb { r: 8, g: 8, b: 8 });
    assert_eq!(p.indexed(255), Rgb { r: 238, g: 238, b: 238 });
}

#[test]
fn every_community_scheme_parses() {
    let all = schemes::builtin();
    assert_eq!(all.len(), 2 + schemes::COMMUNITY.len());
    assert!(schemes::COMMUNITY.len() > 180);
    let night = all.iter().find(|s| s.name == "3024 Night").unwrap();
    assert_eq!(night.palette.background, Rgb { r: 0x09, g: 0x03, b: 0x00 });
}

#[test]
fn xresources_defines_and_short_hex_are_resolved() {
    let p = schemes::parse_xresources("#define bg #123
*.foreground: #ffffff
*.background: bg
*.color0: #000000
*.color1: #ff0000
*.color2: #00ff00
*.color3: #ffff00
*.color4: #0000ff
*.color5: #ff00ff
*.color6: #00ffff
*.color7: #eeeeee
").unwrap();
    assert_eq!(p.background, Rgb { r: 0x11, g: 0x22, b: 0x33 });
    assert_eq!(p.ansi[9], Rgb { r: 255, g: 0, b: 0 }, "missing bright colors repeat the normal ones");
    assert_eq!(p.cursor, p.foreground);
    assert!(schemes::parse_xresources("*.foreground: #fff").is_none());
}
