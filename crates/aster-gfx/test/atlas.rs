use super::*;

#[test]
fn packs_without_overlap_until_full() {
    let mut a = Atlas::new(64);
    let mut slots = Vec::new();
    while let Some(s) = a.alloc(7, 9) {
        slots.push(s);
    }
    assert_eq!(slots.len(), 8 * 6);
    for (i, p) in slots.iter().enumerate() {
        assert!(p.x + p.w <= 64 && p.y + p.h <= 64);
        for q in &slots[i + 1..] {
            let disjoint = p.x + p.w <= q.x || q.x + q.w <= p.x || p.y + p.h <= q.y || q.y + q.h <= p.y;
            assert!(disjoint, "{p:?} overlaps {q:?}");
        }
    }
    a.clear();
    assert!(a.alloc(63, 63).is_some());
}

#[test]
fn tall_allocations_reclaim_adjacent_shelves_without_crossing_a_frame_pin() {
    let mut atlas = Atlas::new(24);
    let shelves: Vec<_> = (0..4).map(|_| atlas.alloc(23, 5).unwrap()).collect();
    atlas.begin_frame();
    atlas.touch(shelves[1]);
    assert!(atlas.evict(23, 17).is_none());
    let cleared = atlas.evict(23, 11).unwrap();
    assert_eq!(cleared, Slot { x: 0, y: 12, w: 24, h: 12 });
    assert_eq!(atlas.alloc(23, 11).unwrap().y, 12);
    assert!(atlas.evict(23, 11).is_none());
}

#[test]
fn small_glyphs_use_space_left_on_a_tall_pinned_shelf() {
    let mut atlas = Atlas::new(16);
    atlas.alloc(7, 15).unwrap();
    assert_eq!(atlas.alloc(7, 3), Some(Slot { x: 8, y: 0, w: 7, h: 3 }));
}
