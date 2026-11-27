use crate::fixture::{Chunking, Fixture};
use crate::{Options, Size, Terminal};
use std::sync::Arc;

#[test]
fn committed_fixtures_replay_across_chunk_boundaries() {
    for (name, json) in [
        (
            "split OSC 133",
            include_bytes!("fixtures/split-osc-133.json").as_slice(),
        ),
        ("reflow", include_bytes!("fixtures/reflow.json").as_slice()),
        (
            "restored command history",
            include_bytes!("fixtures/restored-commands.json").as_slice(),
        ),
    ] {
        let fixture = Fixture::from_json(json).unwrap_or_else(|err| panic!("{name}: {err}"));
        for chunking in Chunking::ALL {
            fixture
                .verify(chunking)
                .unwrap_or_else(|err| panic!("{name}, {chunking:?}: {err}"));
        }
    }
}

#[test]
fn fixture_capture_is_opt_in_and_replays_screen_after_a_session_change() {
    let mut terminal = Terminal::new(Size::new(6, 3), Options::default(), Arc::new(|_| {}));
    assert!(terminal.capture_fixture().is_err());
    assert!(terminal.enable_fixture_recording());
    terminal.feed_saved_output(b"saved line\r\n");
    terminal.resize(Size::new(10, 4));
    terminal.feed("\x1b[31me\u{301}界\x1b[0m\r\n".as_bytes());
    let mut terminal = terminal.next_session(Options::default(), Arc::new(|_| {}), None);
    terminal.feed(b"live");
    let json = terminal.capture_fixture().unwrap().to_json().unwrap();
    let fixture = Fixture::from_json(&json).unwrap();
    for chunking in Chunking::ALL {
        fixture.verify(chunking).unwrap();
    }
}

#[test]
fn fixture_capture_rejects_missing_prefixes() {
    let mut terminal = Terminal::new(Size::new(10, 3), Options::default(), Arc::new(|_| {}));
    terminal.feed(b"already running");
    assert!(!terminal.enable_fixture_recording());
    assert!(terminal.capture_fixture().is_err());

    let mut terminal = Terminal::new(Size::new(10, 3), Options::default(), Arc::new(|_| {}));
    assert!(terminal.enable_fixture_recording());
    terminal.feed(&vec![b'x'; 1024 * 1024 + 1]);
    assert!(terminal.capture_fixture().is_err());
}

#[test]
fn invalid_fixture_metadata_is_rejected_before_replay() {
    let valid = include_str!("fixtures/reflow.json");
    for (original, invalid) in [
        ("\"version\": 1", "\"version\": 99"),
        ("\"cols\": 4", "\"cols\": 1"),
        ("\"cols\": 8", "\"cols\": 4294967295"),
        ("62636465666768", "6263646566676g"),
        ("\"col\": 7", "\"col\": 8"),
    ] {
        assert!(valid.contains(original));
        assert!(Fixture::from_json(valid.replace(original, invalid).as_bytes()).is_err());
    }
}

#[test]
fn fixture_reports_screen_mismatch() {
    let text = include_str!("fixtures/reflow.json").replace("abcdefgh", "abcdxfgh");
    let fixture = Fixture::from_json(text.as_bytes()).unwrap();
    assert!(fixture
        .verify(Chunking::Bytewise)
        .unwrap_err()
        .to_string()
        .contains("row 1"));
}
