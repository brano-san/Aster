use super::*;

fn items() -> Vec<Pick> {
    ["New tab", "Split to the right", "Split to the bottom", "Close tab"].iter().map(|l| Pick { label: (*l).into(), value: l.to_lowercase(), ..Pick::default() }).collect()
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
fn serial_speed_opens_at_115200_and_navigates_the_presets() {
    let mut o = Overlay::serial_speed(4);
    assert_eq!(o.kind, OverlayKind::SerialSpeed(4));
    assert_eq!(o.selected_value().as_deref(), Some("115200"));
    let speeds: Vec<_> = o.shown.iter().map(|(i, _)| o.items[*i].value.as_str()).collect();
    assert_eq!(speeds, ["110", "300", "1200", "2400", "4800", "9600", "19200", "38400", "57600", "115200", "230400", "460800", "921600", "1500000"]);
    o.step(1);
    assert_eq!(o.selected_value().as_deref(), Some("230400"));
    o.step(10);
    assert_eq!(o.selected_value().as_deref(), Some("1500000"));
    assert!(o.selected < o.scroll + VISIBLE);
}

#[test]
fn serial_speed_accepts_exact_custom_values_and_rejects_invalid_input() {
    let mut o = Overlay::serial_speed(1);
    for (text, value) in [("50", "50"), ("31250", "31250"), ("009600", "9600"), ("4000000", "4000000")] {
        o.set_query(text);
        assert_eq!(o.selected_value().as_deref(), Some(value));
        assert_eq!(o.shown.len(), 1);
    }
    for text in ["0", "49", "4000001", "4294967296", "+9600", "9k6"] {
        o.set_query(text);
        assert_eq!(o.selected_value(), None);
    }
    o.backspace(true);
    assert_eq!(o.selected_value().as_deref(), Some("115200"));
}

#[test]
fn quick_connect_offers_an_ssh_target_typed_into_the_selector() {
    let mut o = Overlay::new(OverlayKind::Profiles, items()).with_quick_connect(Some(Provider::Ssh), true);
    o.insert("root@db1:2222");
    assert_eq!(o.selected_value().as_deref(), Some("quick:ssh://root@db1:2222"));
    o.backspace(true);
    o.insert("split");
    assert_ne!(o.selected_value().as_deref(), Some("quick:ssh://split"), "a plain word keeps profiles first");
    assert!(o.shown.iter().any(|(i, _)| o.items[*i].value == "quick:ssh://split"));
    o.backspace(true);
    o.insert("two words");
    assert!(o.items.iter().all(|p| !p.value.starts_with(QUICK)));
}

#[test]
fn translated_commands_are_still_found_by_their_english_names() {
    crate::i18n::set_language("ru");
    let actions = [crate::keymap::Action::NewTab, crate::keymap::Action::NewAdminTab];
    let picks = actions.iter().map(|a| Pick { label: a.label(), value: a.id(), ..Pick::default() }).collect();
    let mut o = Overlay::new(OverlayKind::Commands, picks);
    o.insert("administrator");
    assert_eq!(o.selected_value().as_deref(), Some("new-admin-tab"));
    assert!(o.shown[0].1.is_empty(), "the translated label is not highlighted for an English match");
    o.backspace(true);
    o.insert("администратора");
    assert_eq!(o.selected_value().as_deref(), Some("new-admin-tab"));
    crate::i18n::set_language("en");
}

#[test]
fn quick_connect_offers_telnet_and_tcp_addresses_without_ssh() {
    let mut o = Overlay::new(OverlayKind::Profiles, items());
    o.insert("telnet://router");
    assert_eq!(o.selected_value().as_deref(), Some("quick:telnet://router:23"));
    assert_eq!(o.items.last().map(|p| p.hint.as_str()), Some("Telnet"));
    o.backspace(true);
    o.insert("tcp://10.0.0.5:7000");
    assert_eq!(o.selected_value().as_deref(), Some("quick:tcp://10.0.0.5:7000"));
    o.backspace(true);
    o.insert("router");
    assert!(o.items.iter().all(|p| !p.value.starts_with(QUICK)), "a bare host needs a scheme or SSH");
}
