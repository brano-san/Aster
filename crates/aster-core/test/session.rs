use super::*;
use crate::app::Effect;
use crate::config::Config;
use crate::input::{Event, Key, Mods};
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

fn app() -> (App, Gfx<Mono>) {
    let profiles = vec![Profile::new("local:a", "Shell A", "a.exe", &[]), Profile::new("local:b", "Shell B", "b.exe", &["-x"])];
    let mut a = App::new(Config::default(), profiles, true, Some("H".into()), Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    (a, Gfx::new(Mono, 1024))
}

fn spawns(e: &[Effect]) -> Vec<(String, Vec<String>, Option<String>)> {
    e.iter().filter_map(|e| if let Effect::Spawn { launch, .. } = e { Some((launch.command.clone(), launch.args.clone(), launch.cwd.clone())) } else { None }).collect()
}

#[test]
fn tabs_splits_profiles_folders_and_buffers_survive_a_restart() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.focused_terminal().unwrap().lock().feed(b"\x1b]7;file://pc/D:/work\x07hello from before\r\nsecond line\r\n");
    a.on_event(Event::Key { key: Key::Char('S'), mods: Mods { ctrl: true, shift: true, ..Mods::NONE }, pressed: true });
    let b = a.profiles()[1].clone();
    a.open_tab(b);
    a.activate(0);
    a.frame(&mut g, false);
    a.take_effects();

    let state = SessionState { windows: vec![a.snapshot()], recent: Vec::new() };
    let text = state.to_text();
    assert!(!text.contains("needle"));
    let parsed = SessionState::parse(&text).unwrap();
    assert_eq!(parsed.to_text(), text);

    let (mut fresh, mut g2) = app();
    assert!(fresh.restore(&parsed.windows[0]));
    assert_eq!(fresh.tab_count(), 2);
    fresh.frame(&mut g2, false);
    assert_eq!(fresh.layout.panes.len(), 2, "the split comes back");
    let launched = spawns(&fresh.take_effects());
    assert_eq!(launched.len(), 2);
    assert!(launched.iter().all(|l| l.2.as_deref() == Some(r"D:\work")), "{launched:?}");
    {
        let mut t = fresh.panes.get(&fresh.layout.panes[0].0).unwrap().terminal.lock();
        t.feed(b"prompt>");
        assert_eq!(t.screen_text(), "hello from before\nsecond line\nprompt>", "the new shell starts below the restored text");
    }
    fresh.activate(1);
    fresh.frame(&mut g2, false);
    let tab_b = spawns(&fresh.take_effects());
    assert_eq!(tab_b[0].0, "b.exe");
    assert_eq!(tab_b[0].1, ["-x"]);
}

#[test]
fn garbage_or_empty_state_restores_nothing() {
    assert!(SessionState::parse("not json").is_none());
    let (mut a, _) = app();
    assert!(!a.restore(&WindowState::default()));
    assert_eq!(a.tab_count(), 0);
}

#[test]
fn a_split_remembers_the_folder_it_inherited_before_the_shell_reports_one() {
    let (mut a, mut g) = app();
    a.open_default_tab();
    a.focused_terminal().unwrap().lock().feed(b"\x1b]7;file://pc/D:/work\x07");
    a.on_event(Event::Key { key: Key::Char('D'), mods: Mods { ctrl: true, shift: true, ..Mods::NONE }, pressed: true });
    a.frame(&mut g, false);
    let state = a.snapshot();
    let NodeState::Split { b, .. } = &state.tabs[0].root else { panic!("split") };
    let NodeState::Pane(p) = b.as_ref() else { panic!("pane") };
    assert_eq!(p.cwd.as_deref(), Some(r"D:\work"));
}
