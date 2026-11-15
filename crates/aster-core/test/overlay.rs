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

#[test]
fn quick_connect_offers_an_ssh_target_typed_into_the_selector() {
    let mut o = Overlay::new(OverlayKind::Profiles, items()).with_quick_connect();
    o.insert("root@db1:2222");
    assert_eq!(o.selected_value().as_deref(), Some("quick:root@db1:2222"));
    o.backspace(true);
    o.insert("split");
    assert_ne!(o.selected_value().as_deref(), Some("quick:split"), "a plain word keeps profiles first");
    assert!(o.shown.iter().any(|(i, _)| o.items[*i].value == "quick:split"));
    o.backspace(true);
    o.insert("two words");
    assert!(o.items.iter().all(|p| !p.value.starts_with(QUICK)));
}
