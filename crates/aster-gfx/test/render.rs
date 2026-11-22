use super::*;

#[test]
fn layout_and_colors() {
    assert_eq!(std::mem::size_of::<Instance>(), 64);
    assert_eq!(Color::rgb(0x112233).0.to_le_bytes(), [0x11, 0x22, 0x33, 0xFF]);
    assert_eq!(Color::rgb(0x112233).alpha(0x80).0.to_le_bytes(), [0x11, 0x22, 0x33, 0x80]);
    assert_eq!(Rect::new(0.4, 0.6, 9.2, 9.3).snap(), Rect::new(0.0, 1.0, 10.0, 9.0));
}

#[test]
fn a_check_mark_is_two_thick_strokes_inside_its_box() {
    for side in [14.0, 18.0, 21.0, 28.0, 36.0] {
        let r = Rect::new(10.5, 20.25, side, side);
        let mut dl = DrawList::default();
        dl.check(r, Color::rgb(0));
        let stroke = check_stroke(r);
        assert!(stroke >= side / 8.0 && stroke >= 1.75, "{side}: stroke {stroke}");
        assert!(dl.instances.len() >= 6);
        let (mut lo, mut hi) = ((f32::MAX, f32::MAX), (f32::MIN, f32::MIN));
        for i in &dl.instances {
            let [x, y, w, h] = i.rect;
            assert_eq!((w, h, i.radius, i.kind), (stroke, stroke, stroke / 2.0, kind::SOLID), "round dots of the stroke width");
            lo = (lo.0.min(x), lo.1.min(y));
            hi = (hi.0.max(x + w), hi.1.max(y + h));
        }
        assert!(lo.0 >= r.x && lo.1 >= r.y && hi.0 <= r.right() && hi.1 <= r.bottom(), "{side}: the mark stays in the box");
        assert!(hi.0 - lo.0 >= side * 0.55 && hi.1 - lo.1 >= side * 0.45, "{side}: the mark fills the box");
        let gaps = dl.instances.windows(2).map(|p| (p[1].rect[0] - p[0].rect[0]).hypot(p[1].rect[1] - p[0].rect[1]));
        assert!(gaps.into_iter().all(|d| d <= stroke * 0.5 + 1e-3), "{side}: the strokes have no holes");
    }
}

#[test]
fn a_panel_refits_with_its_shadow_margin_and_slid_items_keep_their_clip() {
    let panel = Rect::new(10.0, 20.0, 100.0, 80.0);
    let mut dl = DrawList::default();
    dl.shadow(panel, Color::rgba(0x0000_0050), 6.0, 10.0);
    dl.round_rect(panel, Color::rgb(0xFFFFFF), 6.0);
    dl.refit(0, panel, Rect::new(10.0, 20.0, 100.0, 30.0));
    assert_eq!(dl.instances[0].rect, [0.0, 10.0, 120.0, 50.0], "the shadow keeps its blur around the shorter panel");
    assert_eq!(dl.instances[1].rect, [10.0, 20.0, 100.0, 30.0]);
    dl.refit(0, Rect::new(10.0, 20.0, 100.0, 30.0), Rect::new(10.0, 200.0, 100.0, 0.0));
    assert!(dl.instances.iter().all(|i| i.rect[2] >= 0.0 && i.rect[3] >= 0.0), "a size never goes negative");
    let mut dl = DrawList::default();
    dl.set_clip(Some(panel));
    dl.rect(Rect::new(20.0, 30.0, 10.0, 10.0), Color::rgb(0x102030).alpha(0x80));
    dl.nudge(0, 0.0, -8.0);
    dl.fade(0, 0.5);
    let i = dl.instances[0];
    assert_eq!((i.rect[1], i.clip), (22.0, [10.0, 20.0, 110.0, 100.0]), "a slide moves the item, not the panel's clip");
    assert_eq!(i.color >> 24, 0x40);
    assert_eq!(i.color & 0x00FF_FFFF, Color::rgb(0x102030).0 & 0x00FF_FFFF);
    dl.fade(0, 1.0);
    assert_eq!(dl.instances[0].color >> 24, 0x40, "a full fade leaves colours alone");
}

#[test]
fn grow_range_scales_only_selected_rects_and_radii_and_keeps_their_clip() {
    let rect = Rect::new(10.0, 20.0, 20.0, 40.0);
    let color = Color::rgba(0x1234_5641);
    let mut dl = DrawList::default();
    dl.set_clip(Some(rect));
    for _ in 0..3 {
        dl.outline(rect, color, 8.0, 2.0);
    }
    dl.grow_range(1..2, 0.5, (20.0, 40.0), 0.5);
    assert_eq!(dl.instances[1].rect, [12.5, 25.0, 15.0, 30.0]);
    assert_eq!(dl.instances[1].radius, 6.0);
    assert_eq!(dl.instances[1].border, 2.0);
    assert_eq!(dl.instances[1].clip, [10.0, 20.0, 30.0, 60.0]);
    assert_eq!(dl.instances[1].color.to_le_bytes(), [0x12, 0x34, 0x56, 0x21]);
    for i in [0, 2] {
        assert_eq!(dl.instances[i].rect, [10.0, 20.0, 20.0, 40.0]);
        assert_eq!(dl.instances[i].color, color.0);
    }
}

#[test]
fn pop_and_grow_use_their_start_scale_and_keep_the_final_frame_exact() {
    let rect = Rect::new(10.0, 20.0, 20.0, 40.0);
    let color = Color::rgb(0x123456);
    let mut dl = DrawList::default();
    dl.round_rect(rect, color, 8.0);
    dl.round_rect(rect, color, 8.0);
    dl.pop_in(1, 0.0, (20.0, 40.0));
    assert!((dl.instances[1].rect[2] - 19.2).abs() < 0.00001);
    assert_eq!(dl.instances[1].color, color.alpha(0).0);
    dl.round_rect(rect, color, 8.0);
    dl.grow(2, 0.0, (20.0, 40.0), 0.5);
    assert_eq!(dl.instances[2].rect, [15.0, 30.0, 10.0, 20.0]);
    assert_eq!(dl.instances[0].rect, [10.0, 20.0, 20.0, 40.0]);

    dl.pop_in(0, 1.0, (1.0e9, 1.0e9));
    dl.grow(0, 1.5, (1.0e9, 1.0e9), 0.5);
    dl.grow_range(0..1, 1.0, (1.0e9, 1.0e9), 0.5);
    dl.fade(0, 1.5);
    assert_eq!(dl.instances[0].rect, [10.0, 20.0, 20.0, 40.0]);
    assert_eq!(dl.instances[0].radius, 8.0);
    assert_eq!(dl.instances[0].color, color.0);
}

#[test]
fn shift_moves_clips_with_rects_but_leaves_the_unclipped_sentinel() {
    let rect = Rect::new(10.0, 20.0, 20.0, 40.0);
    let color = Color::rgb(0x123456);
    let mut dl = DrawList::default();
    dl.set_clip(Some(rect));
    dl.rect(rect, color);
    dl.rect(rect, color);
    dl.set_clip(None);
    dl.rect(rect, color);
    dl.shift_range(1..2, 4.0, -6.0);
    dl.shift(2, 500.0, 300.0);
    assert_eq!(dl.instances[0].rect, [10.0, 20.0, 20.0, 40.0]);
    assert_eq!(dl.instances[1].rect, [14.0, 14.0, 20.0, 40.0]);
    assert_eq!(dl.instances[1].clip, [14.0, 14.0, 34.0, 54.0]);
    assert_eq!(dl.instances[2].rect, [510.0, 320.0, 20.0, 40.0]);
    assert_eq!(dl.instances[2].clip, NO_CLIP);
    dl.shift(1, 0.0, 0.0);
    dl.shift_range(1..3, 0.0, 0.0);
    assert_eq!(dl.instances[1].rect, [14.0, 14.0, 20.0, 40.0]);
    assert_eq!(dl.instances[1].clip, [14.0, 14.0, 34.0, 54.0]);
    assert_eq!(dl.instances[2].clip, NO_CLIP);
}

#[test]
fn refit_applies_all_edge_deltas_and_idle_translation_keeps_rects_exact() {
    let was = Rect::new(10.0, 20.0, 100.0, 80.0);
    let to = Rect::new(14.0, 26.0, 104.0, 69.0);
    let mut dl = DrawList::default();
    dl.rect(was, Color::rgb(0));
    dl.rect(Rect::new(20.0, 40.0, 50.0, 30.0), Color::rgb(0));
    dl.refit(1, was, to);
    assert_eq!(dl.instances[0].rect, [10.0, 20.0, 100.0, 80.0]);
    assert_eq!(dl.instances[1].rect, [24.0, 46.0, 54.0, 19.0]);
    dl.refit(1, to, to);
    dl.nudge(1, 0.0, 0.0);
    assert_eq!(dl.instances[1].rect, [24.0, 46.0, 54.0, 19.0]);
}
