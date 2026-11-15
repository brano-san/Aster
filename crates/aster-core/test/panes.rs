use super::*;

fn area() -> Rect {
    Rect::new(0.0, 0.0, 201.0, 101.0)
}

#[test]
fn splitting_places_the_new_pane_on_the_requested_side() {
    let mut n = Node::Leaf(1);
    assert!(n.split(1, Side::Right, 2));
    assert!(n.split(2, Side::Top, 3));
    assert_eq!(n.panes(), [1, 3, 2]);
    let l = n.layout(area(), 1.0);
    let rect = |id| l.panes.iter().find(|(p, _)| *p == id).unwrap().1;
    assert_eq!(rect(1), Rect::new(0.0, 0.0, 100.0, 101.0));
    assert_eq!(rect(3), Rect::new(101.0, 0.0, 100.0, 50.0));
    assert_eq!(rect(2), Rect::new(101.0, 51.0, 100.0, 50.0));
    assert_eq!(l.splitters.len(), 2);
    assert_eq!(l.splitters[1].path, [true]);
}

#[test]
fn removing_a_pane_collapses_its_split() {
    let mut n = Node::Leaf(1);
    n.split(1, Side::Bottom, 2);
    n.split(2, Side::Left, 3);
    assert!(n.remove(3));
    assert_eq!(n, Node::Split { axis: Axis::Vertical, ratio: 0.5, a: Box::new(Node::Leaf(1)), b: Box::new(Node::Leaf(2)) });
    assert!(n.remove(1));
    assert_eq!(n, Node::Leaf(2));
    assert!(!n.remove(2), "the last pane is never removed by the tree");
}

#[test]
fn ratios_change_by_path_and_by_growing_a_pane() {
    let mut n = Node::Leaf(1);
    n.split(1, Side::Right, 2);
    n.set_ratio(&[], 0.25);
    assert_eq!(n.layout(area(), 1.0).panes[0].1.w, 50.0);
    assert!(n.grow(2, Axis::Horizontal, 0.25));
    assert_eq!(n.layout(area(), 1.0).panes[0].1.w, 10.0);
    assert!(!n.grow(2, Axis::Vertical, 0.1));
    n.set_ratio(&[], 2.0);
    assert!(matches!(n, Node::Split { ratio, .. } if ratio == 0.95));
}

#[test]
fn neighbors_are_found_geometrically() {
    let mut n = Node::Leaf(1);
    n.split(1, Side::Right, 2);
    n.split(2, Side::Bottom, 3);
    let l = n.layout(area(), 1.0);
    assert_eq!(neighbor(&l, 1, Side::Right), Some(2));
    assert_eq!(neighbor(&l, 3, Side::Left), Some(1));
    assert_eq!(neighbor(&l, 3, Side::Top), Some(2));
    assert_eq!(neighbor(&l, 2, Side::Bottom), Some(3));
    assert_eq!(neighbor(&l, 1, Side::Left), None);
}
