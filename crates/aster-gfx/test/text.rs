use super::*;

pub struct Mono;

impl TextBackend for Mono {
    fn shape(&mut self, text: &str, _: FontStyle, out: &mut ShapedText) {
        out.glyphs.clear();
        for (i, (b, c)) in text.char_indices().enumerate() {
            out.glyphs.push(ShapedGlyph { font: 0, id: c as u16, x: i as f32 * 10.0, y: 0.0, cluster: b as u32, cluster_x: i as f32 * 10.0 });
        }
        out.width = out.glyphs.len() as f32 * 10.0;
    }
    fn rasterize(&mut self, key: GlyphKey, out: &mut GlyphBitmap) -> bool {
        if key.id == ' ' as u16 {
            return false;
        }
        *out = GlyphBitmap { left: 1, top: 7, width: 6, height: 8, data: vec![255; 48], color: false };
        true
    }
    fn metrics(&mut self, _: FontStyle) -> FontMetrics {
        FontMetrics { ascent: 8.0, descent: 2.0 }
    }
}

fn style() -> FontStyle {
    FontStyle::new(FontFamily::Ui, 12.0, false)
}

#[test]
fn caches_glyphs_and_ellipsizes() {
    let mut g = Gfx::new(Mono, 256);
    g.begin(Color::default());
    assert_eq!(g.text(0.0, 10.0, "ab a", style(), Color::default(), 1000.0), 40.0);
    assert_eq!(g.dl.instances.len(), 3, "space has no quad");
    assert_eq!(g.uploads.rects.len(), 2, "'a' rasterized once");
    g.begin(Color::default());
    let w = g.text(0.0, 10.0, "abcdefgh", style(), Color::default(), 45.0);
    assert_eq!(w, 40.0);
    assert_eq!(g.dl.instances.len(), 4);
}

#[test]
fn caret_math() {
    let mut g = Gfx::new(Mono, 256);
    assert_eq!(g.caret_x("hello", style(), 0), 0.0);
    assert_eq!(g.caret_x("hello", style(), 2), 20.0);
    assert_eq!(g.caret_x("hello", style(), 5), 50.0);
    assert_eq!(g.hit("hello", style(), 23.0), 2);
    assert_eq!(g.hit("hello", style(), 99.0), 5);
    assert_eq!(g.hit("при", style(), 11.0), 2, "byte offsets of multibyte chars");
}

#[test]
fn cell_run_anchors_ligatures_and_combining_marks_to_grid_columns() {
    struct Clusters;
    impl TextBackend for Clusters {
        fn shape(&mut self, text: &str, _: FontStyle, out: &mut ShapedText) {
            assert_eq!(text, "界=>e\u{301}");
            out.glyphs = [(0, 0.0, 0.0), (3, 16.4, 16.4), (5, 34.1, 34.1), (5, 36.1, 34.1)].map(|(cluster, x, cluster_x)| {
                ShapedGlyph { font: 0, id: 1, x, y: 0.0, cluster, cluster_x }
            }).to_vec();
            out.width = 42.4;
        }
        fn rasterize(&mut self, key: GlyphKey, out: &mut GlyphBitmap) -> bool {
            Mono.rasterize(key, out)
        }
        fn metrics(&mut self, style: FontStyle) -> FontMetrics {
            Mono.metrics(style)
        }
    }
    let mut g = Gfx::new(Clusters, 128);
    g.cell_run("界=>e\u{301}", &[(0, 10.0), (3, 30.0), (4, 40.0), (5, 50.0)], 20.0, style(), Color::default());
    assert_eq!(g.dl.instances.iter().map(|i| i.rect[0]).collect::<Vec<_>>(), [11.0, 31.0, 51.0, 53.0]);
}

#[test]
fn glyph_pressure_evicts_the_least_recently_used_shelf() {
    let mut g = Gfx::new(Mono, 21);
    g.begin(Color::default());
    g.text(0.0, 10.0, "abcdef", style(), Color::default(), 1e9);
    let cold = g.dl.instances[3].uv;
    g.uploads.clear();
    g.begin(Color::default());
    g.text(0.0, 10.0, "a", style(), Color::default(), 1e9);
    g.begin(Color::default());
    g.text(0.0, 10.0, "gbca", style(), Color::default(), 1e9);
    assert_eq!(g.dl.instances.len(), 4);
    assert_eq!(g.dl.instances[0].uv, cold);
    assert_eq!(g.uploads.rects.iter().filter(|(s, _)| s.w == 6 && s.h == 8).count(), 1);
}

#[test]
fn all_pinned_glyphs_keep_their_pixels_and_a_deferred_glyph_is_retried() {
    let mut g = Gfx::new(Mono, 16);
    g.begin(Color::default());
    g.text(0.0, 10.0, "abcba", style(), Color::default(), 1e9);
    assert_eq!(g.uploads.rects.len(), 2);
    assert_eq!(g.dl.instances.len(), 4);
    assert_eq!(g.dl.instances[0].uv, g.dl.instances[3].uv);
    assert_eq!(g.dl.instances[1].uv, g.dl.instances[2].uv);
    g.begin(Color::default());
    g.text(0.0, 10.0, "c", style(), Color::default(), 1e9);
    assert_eq!(g.dl.instances.len(), 1);
}

#[test]
fn images_and_procedural_glyphs_share_the_frame_pin_and_reupload_after_eviction() {
    let mut g = Gfx::new(Mono, 16);
    let image = Rect::new(0.0, 0.0, 6.0, 8.0);
    g.add_image(7, 6, 8, vec![9; 6 * 8 * 4]);
    g.begin(Color::default());
    assert!(g.image(7, image));
    assert!(g.cell_box(0.0, 0.0, '─', 6, 8, Color::default()));
    for _ in 0..2 {
        g.begin(Color::default());
        g.uploads.clear();
        assert!(g.cell_box(0.0, 0.0, '─', 6, 8, Color::default()));
        assert!(g.image(7, image));
        g.text(0.0, 10.0, "x", style(), Color::default(), 1e9);
        assert_eq!(g.dl.instances.len(), 2);
        assert!(g.uploads.rects.is_empty());
    }
    g.begin(Color::default());
    g.text(0.0, 10.0, "xy", style(), Color::default(), 1e9);
    assert_eq!(g.dl.instances.len(), 2);
    assert!(!g.image(7, image));
    g.begin(Color::default());
    g.uploads.clear();
    assert!(g.image(7, image));
    assert!(g.cell_box(0.0, 0.0, '─', 6, 8, Color::default()));
    let bitmaps: Vec<_> = g.uploads.rects.iter().filter(|(s, _)| s.w == 6 && s.h == 8).collect();
    assert_eq!(bitmaps.len(), 2);
    assert_eq!(&g.uploads.data[bitmaps[0].1..bitmaps[0].1 + 6 * 8 * 4], &[9; 6 * 8 * 4]);
}

#[test]
fn a_replaced_background_reserves_atlas_space_before_glyphs_and_uses_full_texels() {
    let mut g = Gfx::new(Mono, 20);
    g.begin(Color::default());
    g.set_background_image(Some((1, [10, 10], &[255; 400])));
    g.prepare_background_image();
    assert!(g.atlas.alloc(19, 8).is_some());
    let shared = g.atlas.alloc(3, 3).unwrap();
    assert_eq!(shared.y, 0);
    g.begin(Color::default());
    g.uploads.clear();
    g.set_background_image(Some((2, [10, 10], &[200; 400])));
    g.prepare_background_image();
    let (slot, offset) = g.uploads.rects.last().unwrap();
    assert_eq!((slot.w, slot.h), (10, 10));
    assert_eq!(&g.uploads.data[*offset..], &[200; 400]);
    g.draw_background_image(Rect::new(0.0, 0.0, 20.0, 20.0), Some([10.0, 10.0]), 128);
    assert_eq!(g.dl.instances[0].uv, [0.0, 0.0, 0.5, 0.5]);
    assert_eq!((g.dl.instances[0].radius, g.dl.instances[0].border), (10.0, 10.0));
}

struct Emoji;

impl TextBackend for Emoji {
    fn shape(&mut self, text: &str, s: FontStyle, out: &mut ShapedText) {
        Mono.shape(text, s, out)
    }
    fn rasterize(&mut self, key: GlyphKey, out: &mut GlyphBitmap) -> bool {
        match key.id {
            0x263A => *out = GlyphBitmap { left: 0, top: 8, width: 2, height: 2, data: [10, 20, 30, 255].repeat(4), color: true },
            0x2639 => *out = GlyphBitmap { left: 0, top: 8, width: 2, height: 2, data: vec![255; 4], color: true },
            _ => *out = GlyphBitmap { left: 0, top: 8, width: 2, height: 2, data: vec![200; 4], color: false },
        }
        true
    }
    fn metrics(&mut self, _: FontStyle) -> FontMetrics {
        FontMetrics { ascent: 8.0, descent: 2.0 }
    }
}

#[test]
fn colour_glyphs_keep_their_colours_and_follow_only_the_text_alpha() {
    let mut g = Gfx::new(Emoji, 256);
    g.begin(Color::default());
    g.text(0.0, 10.0, "a\u{263A}", style(), Color::rgb(0x112233).alpha(0x80), 1000.0);
    let kinds: Vec<u32> = g.dl.instances.iter().map(|i| i.kind).collect();
    assert_eq!(kinds, [crate::render::kind::GLYPH, crate::render::kind::IMAGE]);
    assert_eq!(g.dl.instances[1].color >> 24, 0x80, "the emoji fades with the text but is not tinted");
    let emoji_upload = &g.uploads.data[g.uploads.rects[1].1..g.uploads.rects[1].1 + 16];
    assert_eq!(emoji_upload, [10, 20, 30, 255].repeat(4).as_slice(), "the colour pixels go to the atlas as they are");
    assert_eq!(&g.uploads.data[..4], [200, 200, 200, 200], "a plain glyph is still a tinted mask");
}

#[test]
fn a_colour_glyph_with_short_pixel_data_is_dropped_instead_of_corrupting_the_atlas() {
    let mut g = Gfx::new(Emoji, 256);
    g.begin(Color::default());
    g.text(0.0, 10.0, "\u{2639}b", style(), Color::default(), 1000.0);
    assert_eq!(g.dl.instances.len(), 1, "only the plain glyph is drawn");
    assert_eq!(g.uploads.data.len(), 16);
}
