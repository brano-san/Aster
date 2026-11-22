use crate::app::{App, Effect};
use crate::config::Config;
use crate::input::{Event, Key, Mods, MouseButton};
use crate::profile::Profile;
use aster_gfx::text::{FontMetrics, FontStyle, Gfx, GlyphBitmap, GlyphKey, ShapedGlyph, ShapedText, TextBackend};
use std::sync::Arc;

struct Mono;

impl TextBackend for Mono {
    fn shape(&mut self, text: &str, _: FontStyle, out: &mut ShapedText) {
        out.glyphs.clear();
        for (i, (b, c)) in text.char_indices().enumerate() {
            out.glyphs.push(ShapedGlyph { font: 0, id: c as u16, x: i as f32 * 8.0, y: 0.0, cluster: b as u32, cluster_x: i as f32 * 8.0 });
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

const CTRL_SHIFT: Mods = Mods { ctrl: true, shift: true, alt: false, meta: false };

fn ready() -> (App, Gfx<Mono>) {
    let mut a = App::new(Config::default(), vec![Profile::new("local:a", "a", "shell.exe", &[])], true, None, Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    a.open_default_tab();
    let mut g = Gfx::new(Mono, 1024);
    a.frame(&mut g, false);
    a.take_effects();
    a.focused_terminal().unwrap().lock().feed(b"error: one\r\nok\r\nerror: two\r\n");
    (a, g)
}

#[test]
fn typing_in_the_search_bar_finds_text_without_reaching_the_shell() {
    let (mut a, mut g) = ready();
    a.on_event(Event::Key { key: Key::Char('F'), mods: CTRL_SHIFT, pressed: true });
    a.on_event(Event::Char('\u{6}'));
    for c in "error".chars() {
        a.on_event(Event::Char(c));
    }
    a.on_event(Event::Key { key: Key::Enter, mods: Mods::NONE, pressed: true });
    a.on_event(Event::Char('\r'));
    assert!(a.take_effects().iter().all(|e| !matches!(e, Effect::Write { .. })), "keys typed into search never reach the pty");
    let bar = a.search.clone().unwrap();
    assert_eq!(bar.query, "error");
    assert!(!bar.missing && !bar.error);
    a.frame(&mut g, false);
    a.on_event(Event::Key { key: Key::Escape, mods: Mods::NONE, pressed: true });
    assert!(a.search.is_none());
}

#[test]
fn toggles_persist_in_the_config_and_bad_regexes_are_flagged() {
    let (mut a, mut g) = ready();
    a.open_search();
    a.frame(&mut g, false);
    let geo = a.search_geometry().unwrap();
    let regex = geo.buttons.iter().find(|(h, _)| *h == crate::search::SearchHit::Regex).unwrap().1;
    a.on_event(Event::MouseDown { x: regex.x + 2.0, y: regex.y + 2.0, button: MouseButton::Left, clicks: 1 });
    assert!(a.config().terminal.search_options.regex);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::SaveConfig(t) if t.contains("regex = true"))));
    a.on_event(Event::Char('('));
    assert!(a.search.as_ref().unwrap().error);
    a.on_event(Event::Key { key: Key::Backspace, mods: Mods::NONE, pressed: true });
    for c in "nothing here".chars() {
        a.on_event(Event::Char(c));
    }
    assert!(a.search.as_ref().unwrap().missing);
}

#[test]
fn dense_matches_draw_marks_along_the_right_edge() {
    let (mut a, mut g) = ready();
    a.open_search();
    for c in "needle".chars() {
        a.on_event(Event::Char(c));
    }
    let mut text = String::new();
    for i in 0..3000 {
        text.push_str(&format!("needle {i}\r\n"));
    }
    a.focused_terminal().unwrap().lock().feed(text.as_bytes());
    a.frame(&mut g, false);
    let pane = a.layout.panes[0].1;
    let edge = g.dl.instances.iter().filter(|i| i.rect[0] >= pane.x + pane.w - 7.0 && i.rect[2] <= 7.0 && i.rect[3] <= 5.0).count();
    assert!(edge > 50, "marks drawn: {edge}, count {}", a.search.as_ref().unwrap().count);
}

#[test]
fn ruler_maps_rows_and_merges_adjacent_marks_in_favor_of_current() {
    use crate::search::{ruler_marks, ruler_row, ruler_track, ruler_viewport};
    use aster_gfx::render::Rect;
    let pane = Rect::new(20.0, 50.0, 180.0, 100.0);
    let track = ruler_track(pane, 1.5);
    assert_eq!(track, Rect::new(191.0, 50.0, 9.0, 100.0));
    assert_eq!(ruler_row(track, 50.0, 200), Some(0));
    assert_eq!(ruler_row(track, 100.0, 200), Some(100));
    assert_eq!(ruler_row(track, 149.99, 200), Some(199));
    assert_eq!(ruler_row(track, 150.0, 200), None);
    assert_eq!(ruler_row(track, 100.0, 0), None);
    let marks = ruler_marks(track, &[0, 80, 81, 115, 199], Some(81), 200);
    assert_eq!(marks.len(), 4);
    assert_eq!(marks.iter().filter(|mark| mark.current).count(), 1);
    assert_eq!(marks[1].rect.y, 90.0);
    assert!(marks.iter().all(|mark| mark.rect.h == 2.0 && mark.rect.y >= track.y && mark.rect.bottom() <= track.bottom()));
    assert_eq!(ruler_viewport(track, 50, 100, 200), Rect::new(191.0, 75.0, 9.0, 50.0));
    let small = ruler_viewport(track, 99_960, 40, 100_000);
    assert_eq!(small.bottom(), track.bottom());
    assert!(small.h >= 2.0);
}

#[test]
fn ruler_click_centers_the_clicked_row_and_selects_the_nearest_match() {
    let (mut app, mut gfx) = ready();
    let term = app.focused_terminal().unwrap().clone();
    term.lock().feed("\r\n".repeat(150).as_bytes());
    app.open_search();
    app.search_paste("error");
    app.frame(&mut gfx, false);
    let track = app.search_ruler_rect().unwrap();
    let before = term.lock().search_overview().unwrap();
    let row = before.total_lines / 2;
    let y = track.y + (row as f32 + 0.5) / before.total_lines as f32 * track.h;
    app.on_event(Event::MouseDown { x: track.x + 1.0, y, button: MouseButton::Left, clicks: 1 });
    let after = term.lock().search_overview().unwrap();
    assert_eq!(after.viewport_start + after.viewport_rows / 2, row);
    assert_eq!(after.current_row, after.rows.last().copied());
    assert!(app.take_effects().iter().all(|e| !matches!(e, Effect::Write { .. })));
    app.close_search();
    assert!(app.search_ruler_rect().is_none());
}
