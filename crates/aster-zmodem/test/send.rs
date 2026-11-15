use super::*;
use crate::receive::{Receiver, RxEvent};
use std::collections::BTreeMap;

fn file(name: &str, bytes: Vec<u8>) -> SendFile { SendFile { name: name.into(), modified: 0o14567, source: Box::new(bytes) } }
fn large() -> Vec<u8> { (0..=255).cycle().take(300000).collect() }

fn transfer(files: Vec<SendFile>, chunk: usize, wide: bool, corrupt: bool, skip: bool) -> (BTreeMap<String, Vec<u8>>, Vec<TxEvent>) {
    let mut tx = Sender::new(files);
    let mut rx = Receiver::new();
    let mut to_tx = Vec::new();
    for event in rx.feed(&hex_header(Header::new(ZRQINIT, [0; 4]))) {
        if let RxEvent::Send(bytes) = event { to_tx.extend(bytes); }
    }
    if !wide { to_tx = hex_header(Header::new(ZRINIT, [0, 0, 0, CANFDX | CANOVIO])); }
    let mut received = BTreeMap::new();
    let mut current = (String::new(), Vec::new());
    let mut events = Vec::new();
    let mut corrupted = false;
    let mut packets = 0;
    let mut starts = 0;
    let mut suppress_position = false;
    for _ in 0..100000 {
        for part in std::mem::take(&mut to_tx).chunks(chunk) { events.extend(tx.feed(part)); }
        let mut to_rx = Vec::new();
        while let Some(mut bytes) = tx.poll() {
            if corrupt && bytes.len() > 1024 && !bytes.starts_with(&[ZPAD, ZDLE]) {
                packets += 1;
                if packets == 20 && !corrupted { bytes[1] ^= 1; corrupted = true; }
            }
            to_rx.extend(bytes);
        }
        for part in to_rx.chunks(chunk) {
            for event in rx.feed(part) {
                match event {
                    RxEvent::Send(bytes) => {
                        if suppress_position { suppress_position = false; } else { to_tx.extend(bytes); }
                    }
                    RxEvent::FileStart { name, .. } => {
                        starts += 1;
                        current = (name, Vec::new());
                        if skip && starts == 2 { to_tx.extend(rx.skip()); suppress_position = true; }
                    }
                    RxEvent::Data(bytes) => current.1.extend(bytes),
                    RxEvent::FileEnd => { received.insert(std::mem::take(&mut current.0), std::mem::take(&mut current.1)); }
                    RxEvent::Aborted => panic!("unexpected abort"),
                    RxEvent::Done => {}
                }
            }
        }
        if tx.finished() && rx.finished() {
            assert!(!corrupt || corrupted);
            return (received, events);
        }
    }
    panic!("transfer did not finish");
}

#[test]
fn a_small_file_is_transferred() {
    let (files, events) = transfer(vec![file("a.txt", b"hello".to_vec())], 4096, true, false, false);
    assert_eq!(files["a.txt"], b"hello");
    assert_eq!(events, vec![TxEvent::Progress { file: 0, sent: 5 }, TxEvent::FileDone(0), TxEvent::Done]);
}

#[test]
fn three_files_include_an_empty_file() {
    let (files, events) = transfer(vec![file("a", vec![1]), file("b", vec![]), file("c", vec![2, 3])], 7, true, false, false);
    assert_eq!(files, BTreeMap::from([("a".into(), vec![1]), ("b".into(), vec![]), ("c".into(), vec![2, 3])]));
    assert_eq!(events.iter().filter_map(|e| if let TxEvent::FileDone(i) = e { Some(*i) } else { None }).collect::<Vec<_>>(), vec![0, 1, 2]);
    assert_eq!(events.last(), Some(&TxEvent::Done));
}

#[test]
fn a_large_file_preserves_every_byte() {
    assert_eq!(transfer(vec![file("large", large())], 1000000, true, false, false).0["large"], large());
}

#[test]
fn a_large_file_can_arrive_one_byte_at_a_time() {
    assert_eq!(transfer(vec![file("large", large())], 1, true, false, false).0["large"], large());
}

#[test]
fn a_large_file_can_arrive_in_seven_byte_chunks() {
    assert_eq!(transfer(vec![file("large", large())], 7, true, false, false).0["large"], large());
}

#[test]
fn a_large_file_can_arrive_in_page_sized_chunks() {
    assert_eq!(transfer(vec![file("large", large())], 4096, true, false, false).0["large"], large());
}

#[test]
fn crc16_is_used_when_crc32_is_not_advertised() {
    assert_eq!(transfer(vec![file("large", large())], 7, false, false, false).0["large"], large());
}

#[test]
fn corruption_rewinds_and_retransmits_the_remaining_file() {
    assert_eq!(transfer(vec![file("large", large())], 7, true, true, false).0["large"], large());
}

#[test]
fn skipping_the_second_file_preserves_the_others() {
    let (files, events) = transfer(vec![file("a", vec![1]), file("b", vec![2]), file("c", vec![3])], 7, true, false, true);
    assert_eq!(files, BTreeMap::from([("a".into(), vec![1]), ("c".into(), vec![3])]));
    assert!(events.contains(&TxEvent::Skipped(1)));
}

#[test]
fn aborting_the_sender_aborts_the_receiver() {
    let mut tx = Sender::new(vec![]);
    let mut rx = Receiver::new();
    assert_eq!(rx.feed(&tx.abort()), vec![RxEvent::Aborted]);
    assert!(tx.finished());
    assert!(rx.finished());
    assert!(tx.poll().is_none());
}

#[test]
fn acknowledgements_report_increasing_progress() {
    let mut tx = Sender::new(vec![file("a", large())]);
    tx.feed(&hex_header(Header::new(ZRINIT, [0; 4])));
    tx.poll();
    for position in [10, 20, 1000] {
        assert_eq!(tx.feed(&hex_header(Header::at(ZACK, position))), vec![TxEvent::Progress { file: 0, sent: position as u64 }]);
    }
}

#[test]
fn an_empty_batch_completes_the_handshake() {
    assert_eq!(transfer(vec![], 1, true, false, false), (BTreeMap::new(), vec![TxEvent::Done]));
}

#[test]
fn repeated_initialization_repeats_the_file_offer() {
    let mut tx = Sender::new(vec![file("a.txt", b"hello".to_vec())]);
    let init = hex_header(Header::new(ZRINIT, [0, 0, 0, CANFC32]));
    tx.feed(&init);
    let first = tx.poll().unwrap();
    assert!(tx.poll().is_none());
    tx.feed(&init);
    assert_eq!(tx.poll(), Some(first.clone()));
    assert_eq!(Decoder::new().feed(&first), vec![Item::Header(Header::new(ZFILE, [0; 4])), Item::Subpacket { data: b"a.txt\x005 14567 0 0 1 5\0".to_vec(), end: ZCRCW }]);
}

#[test]
fn a_position_request_during_data_starts_a_new_frame() {
    let mut tx = Sender::new(vec![file("a", large())]);
    tx.feed(&hex_header(Header::new(ZRINIT, [0; 4])));
    tx.poll();
    tx.feed(&hex_header(Header::at(ZRPOS, 0)));
    tx.poll();
    tx.poll();
    tx.feed(&hex_header(Header::at(ZRPOS, 100)));
    assert_eq!(tx.poll(), Some(bin_header(Header::at(ZDATA, 100), false)));
}

#[test]
fn vector_sources_bound_reads_to_available_bytes() {
    let mut source = vec![1, 2, 3];
    let mut buf = [0; 5];
    assert_eq!(Source::read_at(&mut source, 1, &mut buf), 2);
    assert_eq!(&buf[..2], &[2, 3]);
    assert_eq!(Source::read_at(&mut source, u64::MAX, &mut buf), 0);
}

#[test]
fn remote_cancel_finishes_the_sender() {
    let mut tx = Sender::new(vec![file("a", vec![])]);
    assert_eq!(tx.feed(&[CAN; 5]), vec![TxEvent::Aborted]);
    assert!(tx.finished());
    assert!(tx.poll().is_none());
}

#[test]
fn skipping_during_a_continuing_data_frame_allows_the_next_file() {
    let mut tx = Sender::new(vec![file("first", vec![1]), file("skip", vec![2; 4000]), file("third", vec![3; 3000])]);
    let mut rx = Receiver::new();
    tx.feed(&hex_header(Header::new(ZRINIT, [0, 0, 0, CANFC32])));
    let mut current = String::new();
    let mut third = Vec::new();
    let mut skipped = false;
    let mut events = Vec::new();
    for _ in 0..1000 {
        let Some(bytes) = tx.poll() else { break; };
        for event in rx.feed(&bytes) {
            match event {
                RxEvent::Send(bytes) => events.extend(tx.feed(&bytes)),
                RxEvent::FileStart { name, .. } => current = name,
                RxEvent::Data(bytes) if current == "skip" && !skipped => {
                    assert_eq!(bytes.len(), 1024);
                    skipped = true;
                    events.extend(tx.feed(&rx.skip()));
                }
                RxEvent::Data(bytes) if current == "third" => third.extend(bytes),
                _ => {}
            }
        }
    }
    assert!(skipped);
    assert!(tx.finished());
    assert!(rx.finished());
    assert_eq!(third, vec![3; 3000]);
    assert!(events.contains(&TxEvent::Skipped(1)));
    assert!(events.contains(&TxEvent::FileDone(2)));
}

#[test]
fn bytes_after_the_final_reply_are_kept_for_the_caller() {
    let mut tx = Sender::new(Vec::new());
    assert!(tx.feed(&hex_header(Header::new(ZRINIT, [0, 0, 0, CANFC32]))).is_empty());
    assert_eq!(tx.poll(), Some(hex_header(Header::new(ZFIN, [0; 4]))));
    assert!(tx.rest().is_empty());
    let mut bytes = hex_header(Header::new(ZFIN, [0; 4]));
    bytes.extend(b"\r\nuser@host:~$ ");
    assert_eq!(tx.feed(&bytes), vec![TxEvent::Done]);
    assert_eq!(tx.rest(), b"\r\x8a\r\nuser@host:~$ ");
    assert_eq!(tx.poll(), Some(b"OO".to_vec()));
}
