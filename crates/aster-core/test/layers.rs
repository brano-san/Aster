use super::*;
use crate::motion::POP;
use aster_gfx::render::Color;

#[test]
fn popup_opens_and_its_last_frame_fades_after_close() {
    let now = Instant::now();
    let rect = Rect::new(20.0, 30.0, 80.0, 50.0);
    let mut layers = Layers::default();
    let mut dl = DrawList::default();
    layers.begin(1, now + POP / 2, true);
    let from = layers.start(&mut dl, Layer::Overlay, Some(now), POP);
    dl.rect(rect, Color::rgb(0xFFFFFF));
    layers.pop(&mut dl, Layer::Overlay, from, rect, 0, POP);
    assert!(layers.moving);
    let drawn = dl.instances[0];
    assert!(drawn.rect[2] < rect.w);

    dl.instances.clear();
    layers.begin(1, now + POP, true);
    layers.start(&mut dl, Layer::Overlay, None, POP);
    assert!(layers.moving);
    assert_eq!(dl.instances[0].rect, drawn.rect);
    assert_eq!(dl.instances[0].color, drawn.color);

    dl.instances.clear();
    layers.begin(1, now + POP + POP / 2, true);
    layers.start(&mut dl, Layer::Overlay, None, POP);
    assert!(layers.moving);
    assert!(dl.instances[0].color < drawn.color);

    dl.instances.clear();
    layers.begin(1, now + POP * 2, true);
    layers.start(&mut dl, Layer::Overlay, None, POP);
    assert!(!layers.moving);
    assert!(dl.instances.is_empty());
    assert!(!layers.slots.contains_key(&Layer::Overlay));
}

#[test]
fn popup_is_at_final_geometry_without_animations_or_after_opening() {
    let now = Instant::now();
    let rect = Rect::new(20.0, 30.0, 80.0, 50.0);
    for (clock, enabled) in [(now, false), (now + POP, true)] {
        let mut layers = Layers::default();
        let mut dl = DrawList::default();
        layers.begin(1, clock, enabled);
        let from = layers.start(&mut dl, Layer::Overlay, Some(now), POP);
        dl.rect(rect, Color::rgb(0xFFFFFF));
        layers.pop(&mut dl, Layer::Overlay, from, rect, 0, POP);
        assert!(!layers.moving);
        assert_eq!(dl.instances[0].rect, [20.0, 30.0, 80.0, 50.0]);
        assert_eq!(dl.instances[0].color, Color::rgb(0xFFFFFF).0);

        dl.instances.clear();
        layers.begin(1, clock, false);
        layers.start(&mut dl, Layer::Overlay, None, POP);
        assert!(dl.instances.is_empty());
        assert!(!layers.moving);
    }
}
