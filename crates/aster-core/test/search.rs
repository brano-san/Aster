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
            out.glyphs.push(ShapedGlyph { font: 0, id: c as u16, x: i as f32 * 8.0, y: 0.0, cluster: b as u32 });
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
