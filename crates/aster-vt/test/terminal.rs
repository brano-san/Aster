use super::*;
use std::sync::Mutex;

fn term(cols: usize, rows: usize) -> (Terminal, Arc<Mutex<Vec<VtEvent>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink_events = events.clone();
    let sink: Sink = Arc::new(move |e| sink_events.lock().unwrap().push(e));
    (Terminal::new(Size::new(cols, rows), Options::default(), sink), events)
}

#[test]
fn generated_cube_uses_scheme_primaries_and_keeps_the_first_sixteen_colors() {
    let base = Palette::DARK;
    let generated = base.with_extended_colors(false);
    for i in 0..16 {
        assert_eq!(generated.indexed(i), base.indexed(i));
    }
    for (index, ansi) in [(21, 4), (46, 2), (51, 6), (196, 1), (201, 5), (226, 3)] {
        assert_eq!(generated.indexed(index), base.ansi[ansi]);
    }
    assert_ne!(generated.indexed(102), base.indexed(102));
    assert_eq!(base.indexed(16), Rgb { r: 0, g: 0, b: 0 });
}

#[test]
fn generated_ramp_runs_between_scheme_colors_and_reverses_for_light_harmony() {
    let light = Palette::LIGHT;
    let normal = light.with_extended_colors(false);
    let inverted = light.with_extended_colors(true);
    assert!(normal.indexed(232).r > normal.indexed(255).r);
    assert_eq!(normal.indexed(232), inverted.indexed(255));
    assert_eq!(normal.indexed(255), inverted.indexed(232));
    assert_ne!(normal.indexed(196), inverted.indexed(196));
    assert_eq!(Palette::DARK.with_extended_colors(false), Palette::DARK.with_extended_colors(true));
}

#[test]
fn prints_text_and_wraps_lines() {
    let (mut t, _) = term(5, 3);
    t.feed(b"hello world");
    assert_eq!(t.screen_text(), "hello\n worl\nd");
}

#[test]
fn command_marks_use_the_cursor_at_each_osc_and_survive_partial_sequences() {
    let (mut terminal, events) = term(20, 2);
    terminal.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07printf test\r\n\x1b]13");
    terminal.feed(b"3;C\x1b");
    terminal.feed(b"\\first\r\nsecond\r\n\x1b]133;D;7\x07\x1b]133;A\x07$ ");
    assert_eq!(terminal.latest_command_output().as_deref(), Some("first\nsecond\n"));
    assert!(events.lock().unwrap().iter().any(|event| matches!(event, VtEvent::CommandFinished { code: Some(7), .. })));
    assert!(terminal.jump_prompt(true));
    assert_eq!(terminal.screen_text(), "$ printf test\nfirst");
    assert!(terminal.jump_prompt(false));
    assert_eq!(terminal.screen_text(), "second\n$");
}

#[test]
fn command_output_keeps_character_boundaries_after_reflow() {
    let (mut terminal, _) = term(12, 3);
    terminal.feed("prefix \x1b]133;C\x07wide界 characters\x1b]133;D;0\x07 suffix".as_bytes());
    for cols in [12, 7, 20, 5] {
        terminal.resize(Size::new(cols, 3));
        assert_eq!(terminal.latest_command_output().as_deref(), Some("wide界 characters"));
    }
}

#[test]
fn evicted_output_is_unavailable_at_the_scrollback_limit() {
    let mut terminal = Terminal::new(Size::new(20, 2), Options { scrollback: 2, ..Options::default() }, Arc::new(|_| {}));
    terminal.feed(b"\x1b]133;A\x07$ prompt\r\n\x1b]133;C\x07old output\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ ");
    assert_eq!(terminal.latest_command_output().as_deref(), Some("old output\n"));
    terminal.feed(b"\r\n1\r\n2\r\n3\r\n4\r\n");
    assert!(terminal.latest_command_output().is_none());
    assert!(!terminal.jump_prompt(true));
}

#[test]
fn replay_restores_marks_and_suppresses_command_notifications() {
    let (mut terminal, events) = term(12, 3);
    let mut replay = terminal.blank_for_replay(Size::new(20, 3));
    replay.feed_replay(b"\x1b]133;A\x07$ echo text\r\n\x1b]133;C\x07text\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ ");
    terminal.replace_with_replay(replay);
    assert_eq!(terminal.latest_command_output().as_deref(), Some("text\n"));
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn saved_plain_text_restores_output_ranges_at_a_different_width() {
    let (mut terminal, _) = term(20, 3);
    terminal.feed("old line\r\n\x1b]133;A\x07$ echo\r\n\x1b]133;C\x07one long word with 界\r\n\x1b]133;D;9\x07\x1b]133;A\x07$ ".as_bytes());
    let saved = terminal.saved_tail(3).join("\r\n");
    let (mut restored, events) = term(8, 3);
    restored.feed_saved_output(saved.as_bytes());
    assert_eq!(restored.latest_command_output().as_deref(), Some("one long word with 界\n"));
    assert!(restored.jump_prompt(true));
    assert!(!restored.shell_integrated());
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn alternate_screen_output_and_resize_leave_primary_marks_and_text_intact() {
    let (mut terminal, _) = term(12, 3);
    terminal.feed(b"\x1b]133;A\x07$ command\r\n\x1b]133;C\x07command output\x1b]133;D;0\x07\x1b[?1049h\x1b[Heditor\x1b]133;A\x07");
    terminal.resize(Size::new(8, 3));
    assert_eq!(terminal.screen_text(), "editor");
    assert!(terminal.latest_command_output().is_none());
    terminal.feed(b"\x1b[?1049l");
    assert_eq!(terminal.latest_command_output().as_deref(), Some("command output"));
}

#[test]
fn reducing_history_on_the_alternate_screen_prunes_primary_marks() {
    let (mut terminal, _) = term(20, 2);
    terminal.feed(b"\x1b]133;A\x07$ cmd\r\n\x1b]133;C\x07old output\r\n\x1b]133;D;0\x07\x1b]133;A\x07$ \r\nnew\r\nlatest");
    assert!(terminal.latest_command_output().is_some());
    terminal.feed(b"\x1b[?1049h");
    terminal.set_options(Options { scrollback: 0, ..Options::default() });
    terminal.resize(Size::new(10, 3));
    terminal.feed(b"\x1b[?1049l");
    assert!(terminal.latest_command_output().is_none());
    assert!(!terminal.jump_prompt(true));
}

#[test]
fn marks_inside_a_synchronized_update_use_the_painted_grid() {
    let (mut terminal, events) = term(20, 3);
    terminal.feed(b"\x1b[?2026h\x1b]133;A\x07$ command\r\n\x1b]133;C\x07result\r\n\x1b]133;D;7\x07");
    assert!(events.lock().unwrap().is_empty());
    assert!(terminal.latest_command_output().is_none());
    terminal.feed(b"\x1b[?2026l");
    assert_eq!(terminal.latest_command_output().as_deref(), Some("result\n"));
    assert!(events.lock().unwrap().iter().any(|event| matches!(event, VtEvent::CommandFinished { code: Some(7), .. })));
}

#[test]
fn command_history_keeps_status_labels_and_empty_output() {
    let (mut terminal, _) = term(20, 3);
    terminal.feed(b"\x1b]133;A\x07$ \x1b]133;B\x07echo first\r\n\x1b]133;C\x07first\r\n\x1b]133;D;7\x07\x1b]133;A\x07$ \x1b]133;B\x07true\r\n\x1b]133;C\x07\x1b]133;D;0\x07\x1b]133;A\x07$ ");
    let commands = terminal.command_history();
    assert_eq!(commands.len(), 2);
    assert_eq!((&*commands[0].command, &*commands[0].preview, commands[0].status), ("true", "", Some(0)));
    assert_eq!((&*commands[1].command, &*commands[1].preview, commands[1].status), ("echo first", "first", Some(7)));
    assert_eq!(terminal.command_output(commands[0].id).as_deref(), Some(""));
    assert_eq!(terminal.command_output(commands[1].id).as_deref(), Some("first\n"));
    assert!(terminal.latest_command_output().is_none());
    assert!(terminal.reveal_command(commands[1].id));
    assert!(terminal.screen_text().starts_with("first\n"));
}

#[test]
fn saved_commands_keep_identity_when_the_input_line_is_cut_off() {
    let (mut terminal, _) = term(28, 3);
    terminal.feed(b"\x1b]133;C\x07obsolete\r\n\x1b]133;D\x07\x1b]133;A\x07$ \x1b]133;B\x07echo hello\r\n\x1b]133;C\x07hello\r\n\x1b]133;D;9\x07\x1b]133;A\x07$ \x1b]133;B\x07true\r\n\x1b]133;C\x07\x1b]133;D;0\x07\x1b]133;A\x07$ ");
    let expected = terminal.command_history().into_iter().take(2).collect::<Vec<_>>();
    let saved = terminal.saved_tail(3).join("\r\n");
    let (mut restored, events) = term(7, 3);
    restored.feed_saved_output(saved.as_bytes());
    assert_eq!(restored.command_history(), expected);
    assert_eq!(restored.command_output(expected[1].id).as_deref(), Some("hello\n"));
    assert_eq!(restored.command_output(expected[0].id).as_deref(), Some(""));
    assert!(events.lock().unwrap().is_empty());
}

#[test]
fn historical_command_ids_survive_reflow_alt_screen_and_the_next_shell() {
    let (mut terminal, _) = term(12, 3);
    terminal.feed("\x1b]133;A\x07$ \x1b]133;B\x07first\r\n\x1b]133;C\x07wide界 output\x1b]133;D;0\x07\r\n\x1b]133;A\x07$ \x1b]133;B\x07second\r\n\x1b]133;C\x07another\r\n\x1b]133;D;3\x07".as_bytes());
    let expected = terminal.command_history();
    assert_eq!(expected.len(), 2);
    terminal.feed(b"\x1b[?1049h\x1b]133;C\x07editor\x1b]133;D;0\x07");
    terminal.resize(Size::new(5, 4));
    terminal.feed(b"\x1b[?1049l");
    let mut next = terminal.next_session(Options::default(), Arc::new(|_| {}), None);
    assert_eq!(next.command_history(), expected);
    assert_eq!(next.command_output(expected[1].id).as_deref(), Some("wide界 output"));
    next.feed(b"\x1b]133;C\x07next\x1b]133;D;0\x07");
    assert!(next.command_history()[0].id > expected[0].id);
}

#[test]
fn a_pruned_output_cannot_reveal_newer_rows() {
    let mut terminal = Terminal::new(Size::new(20, 2), Options { scrollback: 2, ..Options::default() }, Arc::new(|_| {}));
    terminal.feed(b"\x1b]133;C\x07old output\r\n\x1b]133;D;0\x07");
    let old = terminal.command_history()[0].id;
    terminal.feed(b"one\r\ntwo\r\nthree\r\n\x1b]133;C\x07new\x1b]133;D;0\x07");
    assert_eq!(terminal.command_history().len(), 1);
    assert!(terminal.command_output(old).is_none());
    assert!(!terminal.reveal_command(old));
}

#[test]
fn the_next_shell_prunes_command_history_to_its_smaller_scrollback() {
    let (mut terminal, _) = term(20, 2);
    terminal.feed(b"\x1b]133;C\x07old output\r\n\x1b]133;D;0\x07one\r\ntwo");
    assert_eq!(terminal.command_history().len(), 1);
    let next = terminal.next_session(Options { scrollback: 0, ..Options::default() }, Arc::new(|_| {}), None);
    assert!(next.command_history().is_empty());
}

#[test]
fn commands_in_large_scrollback_are_still_selectable_after_hundreds_of_runs() {
    let (mut terminal, _) = term(24, 3);
    for i in 0..300 {
        terminal.feed(format!("\x1b]133;C\x07output {i}\r\n\x1b]133;D;0\x07").as_bytes());
    }
    let commands = terminal.command_history();
    assert_eq!(commands.len(), 300);
    let oldest = commands.last().unwrap();
    assert_eq!(terminal.command_output(oldest.id).as_deref(), Some("output 0\n"));
    assert!(terminal.reveal_command(oldest.id));
}

#[test]
fn repeated_marks_at_the_same_cursor_position_have_a_small_retention_limit() {
    let (mut terminal, _) = term(24, 3);
    for _ in 0..300 {
        terminal.feed(b"\x1b]133;C\x07\x1b]133;D;0\x07");
    }
    let commands = terminal.command_history();
    assert!(commands.len() <= 64);
    assert_eq!(terminal.command_output(commands[0].id).as_deref(), Some(""));
}

#[test]
fn replay_replaces_the_grid_without_emitting_historical_side_effects() {
    let (mut terminal, events) = term(40, 5);
    terminal.feed(b"stale saved text");
    let mut replay = terminal.blank_for_replay(Size::new(20, 5));
    replay.feed_replay(b"\x1b]2;Shell\x07\x1b[?1049hnew output\x07\x1b[6n\x1b]52;c;dGV4dA==\x07");
    terminal.replace_with_replay(replay);
    assert!(terminal.mode().contains(TermMode::ALT_SCREEN));
    assert_eq!(terminal.screen_text(), "new output");
    assert_eq!(*events.lock().unwrap(), [VtEvent::Title("Shell".into())]);
    terminal.feed(b"\x07\x1b[6n");
    let events = events.lock().unwrap();
    assert!(events.contains(&VtEvent::Bell));
    assert!(events.iter().any(|event| matches!(event, VtEvent::Write(_))));
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
fn a_revealed_buffer_match_keeps_its_exact_selection_when_width_changes() {
    let (mut terminal, _) = term(26, 3);
    terminal.feed("before\r\nprefix 界 needle suffix\r\nafter\r\nmore\r\nbottom".as_bytes());
    let mut regex = search::compile("界 needle", search::SearchOptions::default()).unwrap();
    let hits = terminal.buffer_matches(&mut regex, -10, 2, 1, None);
    assert!(terminal.reveal_buffer_match(&hits[0], None));
    assert_eq!(terminal.selection_text().as_deref(), Some("界 needle"));
    for width in [8, 31] {
        terminal.resize(Size::new(width, 3));
        assert_eq!(terminal.selection_text().as_deref(), Some("界 needle"));
    }
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
