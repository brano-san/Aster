use super::*;

fn chord(s: &str) -> Chord {
    Chord::parse(s).unwrap_or_else(|| panic!("{s}"))
}

#[test]
fn parses_binding_strings() {
    assert_eq!(chord("Ctrl-Shift-T"), Chord { key: Key::Char('T'), mods: Mods { ctrl: true, shift: true, ..Mods::NONE } });
    assert_eq!(chord("Ctrl--").key, Key::Char('-'));
    assert!(chord("Ctrl--").mods.ctrl);
    assert_eq!(chord("Ctrl-Shift--"), Chord { key: Key::Char('-'), mods: Mods { ctrl: true, shift: true, ..Mods::NONE } });
    assert_eq!(chord("F11"), Chord { key: Key::F(11), mods: Mods::NONE });
    assert_eq!(chord("Alt-Enter").key, Key::Enter);
    assert_eq!(chord("Ctrl-Alt-[").key, Key::Char('['));
    assert_eq!(chord("Shift-Insert").key, Key::Insert);
    assert_eq!(chord("⌘-Shift-D").mods, Mods { meta: true, shift: true, ..Mods::NONE });
    assert_eq!(Chord::parse("Fn-X"), None);
    assert_eq!(Chord::parse("Ctrl-Nope"), None);
}

#[test]
fn chords_format_back_in_canonical_order() {
    for s in ["Ctrl-Shift-T", "Ctrl-Alt-Left", "F11", "Ctrl--", "Ctrl-Meta-Alt-Shift-Space"] {
        assert_eq!(chord(s).to_string(), s);
    }
    assert_eq!(format_binding(&parse_binding("Ctrl-A  Ctrl-B").unwrap()), "Ctrl-A Ctrl-B");
}

#[test]
fn every_default_binding_parses_and_ids_round_trip() {
    for (action, keys) in [true, false].into_iter().flat_map(defaults_for) {
        for k in keys {
            assert!(parse_binding(k).is_some(), "{k}");
        }
        assert_eq!(Action::from_id(&action.id()), Some(action));
    }
    for a in Action::all() {
        assert!(!a.label().is_empty(), "{a:?}");
    }
}

#[test]
fn keymap_matches_single_and_multi_chord_bindings() {
    let mut overrides = BTreeMap::new();
    overrides.insert("split-top".to_owned(), vec!["Ctrl-K Up".to_owned()]);
    overrides.insert("copy".to_owned(), vec![]);
    let mut k = Keymap::new(&overrides);
    assert_eq!(k.press(chord("Ctrl-Shift-T")), Outcome::Action(Action::NewTab));
    assert_eq!(k.press(chord("Ctrl-Shift-C")), Outcome::None, "copy was unbound by the override");
    assert_eq!(k.press(chord("Ctrl-K")), Outcome::Pending);
    assert_eq!(k.press(chord("Up")), Outcome::Action(Action::SplitTop));
    assert_eq!(k.press(chord("Ctrl-K")), Outcome::Pending);
    assert_eq!(k.press(chord("Ctrl-Shift-T")), Outcome::Action(Action::NewTab), "a broken sequence retries the last chord alone");
    assert_eq!(k.press(chord("Q")), Outcome::None);
}

#[test]
fn platform_defaults_and_overrides_are_applied() {
    let mut overrides = BTreeMap::new();
    overrides.insert("paste".to_owned(), vec!["Ctrl-P".to_owned()]);
    for (windows, pane_up, other_pane_up, shift_insert) in [
        (true, "Ctrl-Alt-Up", "Alt-Shift-K", Outcome::Action(Action::Paste)),
        (false, "Alt-Shift-K", "Ctrl-Alt-Up", Outcome::None),
    ] {
        let mut keymap = Keymap::for_platform(&BTreeMap::new(), false, windows);
        assert_eq!(keymap.press(chord(pane_up)), Outcome::Action(Action::PaneNavUp));
        assert_eq!(keymap.press(chord(other_pane_up)), Outcome::None);
        assert_eq!(keymap.press(chord("Ctrl-Shift-C")), Outcome::Action(Action::Copy));
        assert_eq!(keymap.press(chord("Ctrl-Shift-V")), Outcome::Action(Action::Paste));
        assert_eq!(keymap.press(chord("Shift-Insert")), shift_insert);
        let mut customized = Keymap::for_platform(&overrides, false, windows);
        assert_eq!(customized.press(chord("Ctrl-Shift-V")), Outcome::None);
        assert_eq!(customized.press(chord("Ctrl-P")), Outcome::Action(Action::Paste));
    }
}

#[test]
fn dynamic_action_ids_round_trip_and_bind() {
    for id in ["profile.ssh:prod.example", "profile-selector.own", "profile-selector.built-in", "profile-selector.ssh-config", "profile-selector.serial-ports", "group-selector.team.ops"] {
        let action = Action::from_id(id).unwrap();
        assert_eq!(action.id(), id);
        let overrides = BTreeMap::from([(id.to_owned(), vec!["Ctrl-Shift-E".to_owned()])]);
        assert_eq!(Keymap::new(&overrides).press(chord("Ctrl-Shift-E")), Outcome::Action(action));
    }
    for id in ["profile.", "group-selector. ", "profile-selector.unknown", "unknown.prod"] {
        assert_eq!(Action::from_id(id), None);
    }
}

#[test]
fn mouse_wheel_and_modifier_only_chords_parse_and_match() {
    for (text, canonical, key) in [
        ("MouseMiddle", "MouseMiddle", Key::Mouse(MouseButton::Middle)),
        ("MouseBack", "MouseBack", Key::Mouse(MouseButton::Back)),
        ("MouseForward", "MouseForward", Key::Mouse(MouseButton::Forward)),
        ("Ctrl+WheelUp", "Ctrl-WheelUp", Key::Wheel(WheelDirection::Up)),
        ("Shift+WheelDown", "Shift-WheelDown", Key::Wheel(WheelDirection::Down)),
        ("Ctrl+WheelLeft", "Ctrl-WheelLeft", Key::Wheel(WheelDirection::Left)),
        ("Ctrl-Shift", "Ctrl-Shift", Key::ModifiersOnly),
    ] {
        let input = chord(text);
        assert_eq!(input.key, key);
        assert_eq!(input.to_string(), canonical);
        assert_eq!(chord(canonical), input);
        let bindings = BTreeMap::from([("export-buffer".to_owned(), vec![format!("F8 {text}")])]);
        let mut keymap = Keymap::new(&bindings);
        assert!(!keymap.accepts(input));
        assert_eq!(keymap.press(chord("F8")), Outcome::Pending);
        assert!(keymap.accepts(input));
        assert_eq!(keymap.press(input), Outcome::Action(Action::ExportBuffer));
    }
    assert_eq!(Chord::parse("Ctrl++WheelUp"), None);
}
