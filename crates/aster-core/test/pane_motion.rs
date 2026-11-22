use super::*;
use crate::panes::{Node, Side};

const BOUNDS: Rect = Rect::new(0.0, 36.0, 201.0, 100.0);

#[test]
fn split_grows_the_new_pane_as_the_separator_moves() {
    let now = Instant::now();
    let mut motion = PaneMotion::default();
    let mut node = Node::Leaf(1);
    assert!(!motion.settle(Some(1), BOUNDS, &node.layout(BOUNDS, 1.0), None, true, now));
    node.split(1, Side::Right, 2);
    let layout = node.layout(BOUNDS, 1.0);
    assert!(motion.settle(Some(1), BOUNDS, &layout, None, true, now));
    assert_eq!(motion.pane(1, now), BOUNDS);
    assert_eq!(motion.pane(2, now), Rect::new(201.0, 36.0, 0.0, 100.0));
    let middle = now + RESIZE / 2;
    assert_eq!(motion.pane(1, middle), Rect::new(0.0, 36.0, 113.0, 100.0));
    assert_eq!(motion.pane(2, middle), Rect::new(114.0, 36.0, 87.0, 100.0));
    assert_eq!(motion.splitter(&[], middle), Rect::new(113.0, 36.0, 1.0, 100.0));
    assert!(!motion.settle(Some(1), BOUNDS, &layout, None, true, now + RESIZE));
    assert_eq!(motion.pane(2, now + RESIZE), layout.panes[1].1);
}

#[test]
fn closing_a_pane_expands_its_neighbor_from_the_visible_size() {
    let now = Instant::now();
    let mut motion = PaneMotion::default();
    let mut node = Node::Leaf(1);
    motion.settle(Some(1), BOUNDS, &node.layout(BOUNDS, 1.0), None, true, now);
    node.split(1, Side::Right, 2);
    motion.settle(Some(1), BOUNDS, &node.layout(BOUNDS, 1.0), None, true, now);
    let retarget = now + RESIZE / 2;
    node.remove(2);
    let layout = node.layout(BOUNDS, 1.0);
    assert!(motion.settle(Some(1), BOUNDS, &layout, None, true, retarget));
    assert_eq!(motion.pane(1, retarget), Rect::new(0.0, 36.0, 113.0, 100.0));
    assert_eq!(motion.pane(1, retarget + RESIZE / 2), Rect::new(0.0, 36.0, 190.0, 100.0));
    assert!(!motion.settle(Some(1), BOUNDS, &layout, None, true, retarget + RESIZE));
    assert_eq!(motion.pane(1, retarget + RESIZE), BOUNDS);
}

#[test]
fn zoom_and_unzoom_interpolate_between_split_and_full_rectangles() {
    let now = Instant::now();
    let mut motion = PaneMotion::default();
    let mut node = Node::Leaf(1);
    node.split(1, Side::Right, 2);
    let split = node.layout(BOUNDS, 1.0);
    motion.settle(Some(1), BOUNDS, &split, None, true, now);
    let zoom = Layout { panes: vec![(2, BOUNDS)], splitters: Vec::new() };
    assert!(motion.settle(Some(1), BOUNDS, &zoom, Some(2), true, now));
    assert_eq!(motion.pane(2, now), split.panes[1].1);
    assert_eq!(motion.pane(2, now + RESIZE / 2), Rect::new(13.0, 36.0, 188.0, 100.0));
    let unzoom = now + RESIZE;
    assert!(!motion.settle(Some(1), BOUNDS, &zoom, Some(2), true, unzoom));
    assert_eq!(motion.pane(2, unzoom), BOUNDS);
    assert!(motion.settle(Some(1), BOUNDS, &split, None, true, unzoom));
    assert_eq!(motion.pane(1, unzoom), Rect::new(0.0, 36.0, 0.0, 100.0));
    assert_eq!(motion.pane(2, unzoom + RESIZE / 2), Rect::new(88.0, 36.0, 113.0, 100.0));
    assert!(!motion.settle(Some(1), BOUNDS, &split, None, true, unzoom + RESIZE));
    assert_eq!(motion.pane(2, unzoom + RESIZE), split.panes[1].1);
}

#[test]
fn resizing_or_disabling_animation_snaps_panes_without_motion() {
    let now = Instant::now();
    let mut motion = PaneMotion::default();
    let mut node = Node::Leaf(1);
    motion.settle(Some(1), BOUNDS, &node.layout(BOUNDS, 1.0), None, true, now);
    node.split(1, Side::Bottom, 2);
    let split = node.layout(BOUNDS, 1.0);
    assert!(motion.settle(Some(1), BOUNDS, &split, None, true, now));
    node.set_ratio(&[], 0.6);
    let dragged = node.layout(BOUNDS, 1.0);
    assert!(!motion.settle(Some(1), BOUNDS, &dragged, None, false, now));
    assert_eq!(motion.pane(1, now), dragged.panes[0].1);
    let bounds = Rect::new(0.0, 36.0, 301.0, 100.0);
    let resized = node.layout(bounds, 1.0);
    assert!(!motion.settle(Some(1), bounds, &resized, None, true, now));
    assert_eq!(motion.pane(2, now), resized.panes[1].1);
}

#[test]
fn rearrange_labels_and_targets_fade_in_without_delaying_input() {
    let now = Instant::now();
    let mut motion = RearrangeMotion::default();
    assert_eq!(motion.settle(true, Some(BOUNDS), true, now), (0.0, 0.0, true));
    assert_eq!(motion.settle(true, Some(BOUNDS), true, now + POP / 2), (0.875, 0.875, true));
    assert_eq!(motion.settle(true, Some(BOUNDS), true, now + POP), (1.0, 1.0, false));
    assert_eq!(motion.settle(false, None, true, now + POP), (0.0, 0.0, false));
    assert_eq!(motion.settle(true, Some(BOUNDS), false, now + POP), (1.0, 1.0, false));
}
