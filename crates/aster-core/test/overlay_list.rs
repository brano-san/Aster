use super::*;
use crate::overlay::{OverlayKind, Pick};

#[test]
fn grouped_picker_keeps_a_fixed_viewport_and_hits_at_the_target() {
    let picks = (0..30).map(|i| Pick {
        label: i.to_string(),
        group: (i / 10).to_string(),
        ..Pick::default()
    }).collect();
    let overlay = Overlay::new(OverlayKind::Profiles, picks);
    let layout = OverlayList::new(Rect::new(0.0, 20.0, 800.0, 600.0), 1.0, &overlay, None);
    let start = layout.target(&overlay);
    let end = layout.reveal(29, start);
    assert_eq!(start, 0.0);
    assert_eq!(end, layout.max);
    let first = layout.geometry(start);
    let last = layout.geometry(end);
    assert_eq!(first.frame, last.frame);
    assert_eq!(last.items.last().unwrap().0, 29);
    let (_, row) = last.items.last().unwrap();
    let mouse = (row.x + 1.0, row.y + 1.0);
    assert_eq!(layout.hit_row(end, mouse), Some(29));
    assert_ne!(layout.hit_row(start, mouse), Some(29));
    assert_eq!(layout.hit_row(end, (row.x, layout.input.y)), None);
}

#[test]
fn shrinking_the_picker_keeps_the_keyboard_selection_in_view() {
    let picks = (0..20).map(|i| Pick { label: i.to_string(), ..Pick::default() }).collect();
    let mut overlay = Overlay::new(OverlayKind::Commands, picks);
    overlay.selected = 8;
    let large = OverlayList::new(Rect::new(0.0, 20.0, 800.0, 600.0), 1.0, &overlay, None);
    let position = large.position(0.0);
    overlay.scroll_offset = Some(position);
    let compact = OverlayList::new(Rect::new(0.0, 20.0, 800.0, 210.0), 1.0, &overlay, None);
    let scroll = compact.target(&overlay);
    assert!(scroll > 0.0);
    let visible = compact.geometry(scroll);
    let (_, selected) = visible.items.iter().find(|(i, _)| *i == overlay.selected).unwrap();
    assert!(selected.y >= visible.list.y && selected.bottom() <= visible.list.bottom());
}
