use super::*;

fn items() -> Vec<Pick> {
    ["New tab", "Split to the right", "Split to the bottom", "Close tab"].iter().map(|l| Pick { label: (*l).into(), hint: String::new(), value: l.to_lowercase() }).collect()
}

#[test]
fn filtering_ranks_by_fuzzy_score_and_resets_the_selection() {
    let mut o = Overlay::new(OverlayKind::Commands, items());
    assert_eq!(o.shown.len(), 4);
    o.step(2);
    o.insert("spl b");
    assert_eq!(o.selected, 0);
    assert_eq!(o.selected_value().as_deref(), Some("split to the bottom"));
    o.backspace(true);
    assert_eq!(o.query, "spl ");
    o.insert("zzz");
    assert_eq!(o.selected_value(), None);
}

#[test]
fn stepping_wraps_by_one_and_clamps_by_pages() {
    let mut o = Overlay::new(OverlayKind::Commands, items());
    o.step(-1);
    assert_eq!(o.selected, 3);
    o.step(1);
    assert_eq!(o.selected, 0);
    o.step(8);
    assert_eq!(o.selected, 3);
}
