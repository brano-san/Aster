use super::*;
use std::time::Instant;

fn term(cols: usize, rows: usize, scrollback: usize) -> Terminal {
    let sink: Sink = Arc::new(|_| {});
    Terminal::new(Size::new(cols, rows), Options { scrollback, ..Options::default() }, sink)
}

fn current_line_text(t: &mut Terminal) -> Option<String> {
    let m = t.visible_matches().into_iter().find(|m| m.current)?;
    let (text, _) = t.line_text(m.line);
    Some(text.trim_end().to_owned())
}

fn lines(t: &mut Terminal, from: usize, to: usize, every: usize) {
    let mut out = String::new();
    for i in from..to {
        if i % every == 0 {
            out.push_str(&format!("needle {i}\r\n"));
        } else {
            out.push_str(&format!("line {i}\r\n"));
        }
    }
    t.feed(out.as_bytes());
}

#[test]
fn the_current_match_stays_on_its_text_while_output_streams() {
    let mut t = term(40, 10, 10_000);
    lines(&mut t, 0, 200, 10);
    t.set_search("needle", search::SearchOptions::default()).unwrap();
    assert!(t.search_step(true));
    assert!(t.search_step(true));
    let picked = current_line_text(&mut t).unwrap();
    assert_eq!(picked, "needle 180");
    for batch in 0..20 {
        lines(&mut t, 200 + batch * 5, 205 + batch * 5, 10);
        assert_eq!(current_line_text(&mut t).as_deref(), Some("needle 180"), "after batch {batch}");
    }
    let status = t.search_status().unwrap();
    assert_eq!(status.count, 30);
    assert!(t.search_step(true));
    assert_eq!(current_line_text(&mut t).as_deref(), Some("needle 170"));
}

#[test]
fn a_full_scrollback_keeps_tracking_by_line_content() {
    let mut t = term(40, 10, 100);
    lines(&mut t, 0, 300, 7);
    t.set_search("needle", search::SearchOptions::default()).unwrap();
    assert!(t.search_step(true));
    assert!(t.search_step(true));
    let picked = current_line_text(&mut t).unwrap();
    for batch in 0..10 {
        lines(&mut t, 300 + batch * 3, 303 + batch * 3, 7);
        assert_eq!(current_line_text(&mut t), Some(picked.clone()), "after batch {batch}");
    }
    let count = t.search_status().unwrap().count;
    let expected = (0..330).filter(|i| i % 7 == 0).count();
    assert!(count <= expected && count >= 13, "only matches still in the buffer are counted: {count}");
}

#[test]
fn new_matches_are_counted_and_marked_as_they_arrive() {
    let mut t = term(40, 10, 10_000);
    t.set_search("needle", search::SearchOptions::default()).unwrap();
    assert_eq!(t.search_status().unwrap().count, 0);
    lines(&mut t, 0, 100, 10);
    assert_eq!(t.search_status().unwrap().count, 10);
    lines(&mut t, 100, 150, 10);
    assert_eq!(t.search_status().unwrap().count, 15);
    let marks = t.search_marks();
    assert_eq!(marks.len(), 15);
    assert!(marks.windows(2).all(|w| w[0].0 < w[1].0));
    assert!(marks.iter().all(|(f, _)| (0.0..=1.0).contains(f)));
}

#[test]
fn rewritten_screen_lines_drop_their_old_matches() {
    let mut t = term(40, 5, 1000);
    t.feed(b"needle here");
    t.set_search("needle", search::SearchOptions::default()).unwrap();
    assert_eq!(t.search_status().unwrap().count, 1);
    t.feed(b"\r\x1b[2Kgone now");
    assert_eq!(t.search_status().unwrap().count, 0);
}

#[test]
fn indexing_a_full_buffer_and_following_a_stream_are_fast() {
    let mut t = term(120, 40, 25_000);
    lines(&mut t, 0, 25_000, 50);
    let started = Instant::now();
    t.set_search("needle", search::SearchOptions::default()).unwrap();
    let status = t.search_status().unwrap();
    let first = started.elapsed();
    assert_eq!(status.count, 500);
    let started = Instant::now();
    for batch in 0..60 {
        lines(&mut t, 25_000 + batch * 2, 25_002 + batch * 2, 50);
        let _ = t.visible_matches();
        let _ = t.search_status();
    }
    let streaming = started.elapsed() / 60;
    assert!(first.as_millis() < 2_000, "full index took {first:?}");
    assert!(streaming.as_millis() < 20, "a streamed frame took {streaming:?}");
}

#[test]
fn marks_stay_inside_the_track_while_a_full_scrollback_streams() {
    let mut t = term(40, 10, 200);
    lines(&mut t, 0, 400, 1);
    t.set_search("needle", search::SearchOptions::default()).unwrap();
    for batch in 0..30 {
        lines(&mut t, 400 + batch * 13, 413 + batch * 13, 1);
        let marks = t.search_marks();
        let out: Vec<f32> = marks.iter().map(|m| m.0).filter(|f| !(0.0..=1.0).contains(f)).collect();
        assert!(out.is_empty(), "batch {batch}: {} of {} marks outside: {:?}", out.len(), marks.len(), &out[..out.len().min(3)]);
        assert_eq!(marks.len(), 209, "batch {batch}");
    }
}

#[test]
fn marks_follow_dense_output_that_arrives_after_the_query() {
    let mut t = term(40, 20, 25_000);
    t.set_search("needle", search::SearchOptions::default()).unwrap();
    let mut from = 0;
    for size in [1, 7, 50, 300, 1000, 1642] {
        lines(&mut t, from, from + size, 1);
        from += size;
        let _ = t.visible_matches();
    }
    let marks = t.search_marks();
    let status = t.search_status().unwrap();
    assert_eq!(marks.len(), status.count);
    let bad: Vec<f32> = marks.iter().map(|m| m.0).filter(|f| !(0.0..=1.0).contains(f)).take(5).collect();
    assert!(bad.is_empty(), "{bad:?} of {}", marks.len());
}
