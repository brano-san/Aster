use super::*;

fn chord(s: &str) -> Chord {
    Chord::parse(s).unwrap_or_else(|| panic!("{s}"))
}

#[test]
fn parses_tabby_binding_strings() {
    assert_eq!(chord("Ctrl-Shift-T"), Chord { key: Key::Char('T'), mods: Mods { ctrl: true, shift: true, ..Mods::NONE } });
    assert_eq!(chord("Ctrl--").key, Key::Char('-'));
    assert!(chord("Ctrl--").mods.ctrl);
    assert_eq!(chord("Ctrl-Shift--"), Chord { key: Key::Char('-'), mods: Mods { ctrl: true, shift: true, ..Mods::NONE } });
    assert_eq!(chord("F11"), Chord { key: Key::F(11), mods: Mods::NONE });
    assert_eq!(chord("Alt-Enter").key, Key::Enter);
    assert_eq!(chord("Ctrl-Alt-[").key, Key::Char('['));
    assert_eq!(chord("Shift-Insert").key, Key::Insert);
    assert_eq!(chord("⌘-Shift-D").mods, Mods { meta: true, shift: true, ..Mods::NONE });
    assert_eq!(Chord::parse("Hyper-X"), None);
    assert_eq!(Chord::parse("Ctrl-Nope"), None);
}

#[test]
fn chords_format_back_in_tabby_order() {
    for s in ["Ctrl-Shift-T", "Ctrl-Alt-Left", "F11", "Ctrl--", "Ctrl-Meta-Alt-Shift-Space"] {
        assert_eq!(chord(s).to_string(), s);
    }
    assert_eq!(format_binding(&parse_binding("Ctrl-A  Ctrl-B").unwrap()), "Ctrl-A Ctrl-B");
}

#[test]
fn every_default_binding_parses_and_ids_round_trip() {
    for (action, keys) in defaults() {
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
