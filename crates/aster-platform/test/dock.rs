use super::*;

const WORK: (i32, i32, i32, i32) = (100, 0, 1920, 1040);

fn spec(edge: Edge, fill: f32, space: f32) -> DockSpec {
    DockSpec { edge, fill, space, on_top: true }
}

#[test]
fn top_and_bottom_docks_span_the_width_and_take_a_share_of_the_height() {
    assert_eq!(dock_rect(WORK, spec(Edge::Top, 0.5, 1.0)), (100, 0, 1920, 520));
    assert_eq!(dock_rect(WORK, spec(Edge::Bottom, 0.25, 1.0)), (100, 780, 1920, 260));
    assert_eq!(dock_rect(WORK, spec(Edge::Top, 0.5, 0.5)), (580, 0, 960, 520), "a partial span is centered");
}

#[test]
fn side_docks_take_a_share_of_the_width() {
    assert_eq!(dock_rect(WORK, spec(Edge::Left, 0.3, 1.0)), (100, 0, 576, 1040));
    assert_eq!(dock_rect(WORK, spec(Edge::Right, 0.3, 1.0)), (1444, 0, 576, 1040));
}

#[test]
fn out_of_range_shares_are_clamped() {
    assert_eq!(dock_rect(WORK, spec(Edge::Top, 3.0, 0.0)), (868, 0, 384, 1040));
}
