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
fn debug_dump_reports_cursor_modes_viewport_and_last_200_history_rows() {
    let (mut t, _) = term(12, 2);
    for i in 0..205 {
        t.feed(format!("row{i:03}\r\n").as_bytes());
    }
    t.feed(b"end\x1b[?1h");
    t.scroll(Scroll::Delta(2));
    let dump = t.debug_dump();
    assert!(dump.starts_with("Grid: 12x2\nCursor (row, column; 1-based): 2, 4\nModes: "));
    assert!(dump.contains("APP_CURSOR"));
    let (head, screen) = dump.split_once("\nScreen:\n").unwrap();
    assert_eq!(screen, "row202\nrow203");
    let (_, history) = head.split_once("\nScrollback:\n").unwrap();
    let rows: Vec<_> = history.lines().collect();
    assert_eq!((rows.len(), rows[0], rows[199]), (200, "row004", "row203"));
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
    let p = Palette::DARK;
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

#[test]
fn working_directory_comes_from_osc_7_9_9_and_1337_even_split_across_chunks() {
    let (mut t, _) = term(20, 3);
    t.feed(b"\x1b]7;file://host/home/me/my%20dir\x07");
    assert_eq!(t.cwd(), Some("/home/me/my dir"));
    t.feed(b"\x1b]7;file://pc/C:/Users/me\x1b\\");
    assert_eq!(t.cwd(), Some(r"C:\Users\me"));
    t.feed(br#"]9;9;"D:\Projects""#.as_slice().iter().fold(vec![0x1b], |mut v, b| {
        v.push(*b);
        v
    }).as_slice());
    t.feed(b"\x1b\\");
    assert_eq!(t.cwd(), Some(r"D:\Projects"));
    t.feed(b"\x1b]1337;CurrentDir=/tmp\x07");
    assert_eq!(t.cwd(), Some("/tmp"));
    t.feed(b"\x1b]0;title\x07");
    assert_eq!(t.cwd(), Some("/tmp"));
    assert_eq!(t.screen_text(), "");
}

#[test]
fn line_text_maps_characters_to_columns_around_wide_chars() {
    let (mut t, _) = term(10, 2);
    t.feed("a日b".as_bytes());
    let (text, cols) = t.line_text(0);
    assert!(text.starts_with("a日b"));
    assert_eq!(&cols[..3], &[0, 1, 3]);
}

#[test]
fn search_walks_matches_from_the_newest_and_scrolls_to_them() {
    let (mut t, _) = term(20, 3);
    t.feed(b"alpha one\r\nbeta\r\nalpha two\r\ngamma\r\ndelta\r\nend");
    t.set_search("alpha", search::SearchOptions::default()).unwrap();
    assert!(t.search_step(true));
    let first = t.visible_matches();
    assert!(first.iter().any(|m| m.current), "{first:?}");
    assert!(t.search_step(true));
    assert!(t.display_offset() > 0, "the older match scrolled into view");
    let screen = t.screen_text();
    assert!(screen.contains("alpha one"), "{screen}");
    assert!(!t.search_step(true) || t.screen_text().contains("alpha"));
}

#[test]
fn search_options_build_the_expected_patterns() {
    use search::{pattern, SearchOptions};
    assert_eq!(pattern("a.b", SearchOptions::default()), r"(?i)a\.b");
    assert_eq!(pattern("a.b", SearchOptions { regex: true, ..Default::default() }), "(?i)a.b");
    assert_eq!(pattern("Hi", SearchOptions { case_sensitive: true, whole_word: true, ..Default::default() }), r"(?-i)(?-u:\b)(?:Hi)(?-u:\b)");
    let (mut t, _) = term(20, 3);
    t.feed(b"Cat cat concat");
    t.set_search("cat", SearchOptions { whole_word: true, ..Default::default() }).unwrap();
    assert_eq!(t.visible_matches().len(), 2);
    t.set_search("cat", SearchOptions { case_sensitive: true, ..Default::default() }).unwrap();
    assert_eq!(t.visible_matches().len(), 2);
    assert!(t.set_search("(", SearchOptions { regex: true, ..Default::default() }).is_err());
}

#[test]
fn progress_reports_set_and_clear_the_state_and_count_as_output() {
    let (mut t, _) = term(20, 3);
    assert_eq!(t.progress(), None);
    let before = t.output_seq();
    t.feed(b"\x1b]9;4;1;42\x07");
    assert_eq!(t.progress(), Some(Progress { state: ProgressState::Normal, percent: 42 }));
    assert!(t.output_seq() > before);
    t.feed(b"\x1b]9;4;3\x1b\\");
    assert_eq!(t.progress().map(|p| p.state), Some(ProgressState::Indeterminate));
    t.feed(b"\x1b]9;4;2;250\x07");
    assert_eq!(t.progress(), Some(Progress { state: ProgressState::Error, percent: 100 }));
    t.feed(b"\x1b]9;4;9;1\x07");
    assert_eq!(t.progress().map(|p| p.state), Some(ProgressState::Error), "unknown states are ignored");
    t.feed(b"\x1b]9;4;0\x07");
    assert_eq!(t.progress(), None);
    t.feed(b"\x1b]9;9;C:/work\x07");
    assert_eq!(t.progress(), None);
}
