use super::*;

fn file(name: &[u8]) -> Vec<u8> {
    let mut bytes = bin_header(Header::new(ZFILE, [0; 4]), true);
    bytes.extend(subpacket(name, ZCRCW, true));
    bytes
}

fn data(position: u32, data: &[u8], end: u8) -> Vec<u8> {
    let mut bytes = bin_header(Header::at(ZDATA, position), true);
    bytes.extend(subpacket(data, end, true));
    bytes
}

fn active() -> Receiver {
    let mut rx = Receiver::new();
    rx.feed(&file(b"a\x005\0"));
    rx
}

#[test]
fn initialization_advertises_crc32_each_time() {
    let mut rx = Receiver::default();
    for _ in 0..2 {
        assert_eq!(rx.feed(&hex_header(Header::new(ZRQINIT, [0; 4]))), vec![RxEvent::Send(ready())]);
    }
}

#[test]
fn file_information_starts_a_file_and_requests_position_zero() {
    assert_eq!(Receiver::new().feed(&file(b"dir/report.txt\x00123 14567 100644\0")), vec![RxEvent::FileStart { name: "report.txt".into(), size: Some(123) }, reply(ZRPOS, 0)]);
}

#[test]
fn data_frames_are_ordered_and_acknowledged() {
    let mut rx = active();
    assert_eq!(rx.feed(&data(0, b"ab", ZCRCE)), vec![RxEvent::Data(b"ab".to_vec())]);
    assert_eq!(rx.feed(&data(2, b"cde", ZCRCW)), vec![RxEvent::Data(b"cde".to_vec()), reply(ZACK, 5)]);
}

#[test]
fn a_wrong_position_requests_the_current_position_and_discards_data() {
    let mut rx = active();
    rx.feed(&data(0, b"ab", ZCRCE));
    assert_eq!(rx.feed(&data(0, b"wrong", ZCRCW)), vec![reply(ZRPOS, 2)]);
}

#[test]
fn a_corrupted_subpacket_requests_the_last_good_position() {
    let mut rx = active();
    let mut bytes = data(0, b"abc", ZCRCE);
    let offset = bin_header(Header::at(ZDATA, 0), true).len();
    bytes[offset] ^= 1;
    assert_eq!(rx.feed(&bytes), vec![reply(ZRPOS, 0)]);
    assert_eq!(rx.feed(&data(0, b"abc", ZCRCE)), vec![RxEvent::Data(b"abc".to_vec())]);
}

#[test]
fn matching_eof_finishes_the_file() {
    let mut rx = active();
    rx.feed(&data(0, b"abc", ZCRCE));
    assert_eq!(rx.feed(&hex_header(Header::at(ZEOF, 3))), vec![RxEvent::FileEnd, RxEvent::Send(ready())]);
}

#[test]
fn mismatching_eof_is_ignored() {
    assert!(active().feed(&hex_header(Header::at(ZEOF, 9))).is_empty());
}

#[test]
fn finish_replies_once_and_ignores_trailing_bytes() {
    let mut rx = Receiver::new();
    assert_eq!(rx.feed(&hex_header(Header::at(ZFIN, 0))), vec![reply(ZFIN, 0), RxEvent::Done]);
    assert!(rx.finished());
    assert!(rx.feed(b"OO").is_empty());
}

#[test]
fn cancel_finishes_the_receiver() {
    let mut rx = active();
    assert_eq!(rx.feed(&[CAN; 5]), vec![RxEvent::Aborted]);
    assert!(rx.finished());
}

#[test]
fn skipping_discards_remaining_data_until_another_file() {
    let mut rx = active();
    assert_eq!(rx.skip(), hex_header(Header::at(ZSKIP, 0)));
    assert!(rx.feed(&data(0, b"abc", ZCRCW)).is_empty());
    assert!(rx.feed(&hex_header(Header::at(ZEOF, 3))).is_empty());
    assert!(rx.skip().is_empty());
    assert!(matches!(rx.feed(&file(b"next\x003\0"))[0], RxEvent::FileStart { .. }));
}

#[test]
fn remote_paths_are_reduced_to_a_filename() {
    for path in [b"../../etc/passwd\0".as_slice(), b"..\\..\\etc\\passwd\0"] {
        assert_eq!(Receiver::new().feed(&file(path))[0], RxEvent::FileStart { name: "passwd".into(), size: None });
    }
}

#[test]
fn unsafe_or_empty_names_use_the_default() {
    for name in [b"..\0".as_slice(), b".\0", b"\0"] {
        assert_eq!(Receiver::new().feed(&file(name))[0], RxEvent::FileStart { name: "file".into(), size: None });
    }
}

#[test]
fn sender_initialization_is_acknowledged() {
    let mut bytes = hex_header(Header::at(ZSINIT, 0));
    bytes.extend(subpacket(b"hello", ZCRCW, false));
    assert_eq!(Receiver::new().feed(&bytes), vec![reply(ZACK, 0)]);
}

#[test]
fn abort_returns_the_cancel_sequence() {
    let mut rx = Receiver::new();
    assert_eq!(rx.abort(), [vec![CAN; 8], vec![8; 8]].concat());
    assert!(rx.finished());
    assert!(rx.feed(&file(b"a\0")).is_empty());
}
