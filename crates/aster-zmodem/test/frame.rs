use super::*;

fn stream(wide: bool) -> (Vec<u8>, Vec<Item>) {
    let header = Header::at(ZDATA, 42);
    let mut bytes = bin_header(header, wide);
    let mut expected = vec![Item::Header(header)];
    for end in [ZCRCG, ZCRCQ, ZCRCE] {
        let data: Vec<u8> = (0..=255).collect();
        bytes.extend(subpacket(&data, end, wide));
        expected.push(Item::Subpacket { data, end });
    }
    (bytes, expected)
}

#[test]
fn the_initial_hex_header_matches_the_wire_example() {
    assert_eq!(hex_header(Header::new(ZRQINIT, [0; 4])), b"**\x18B00000000000000\r\x8a\x11");
}

#[test]
fn all_special_bytes_are_escaped() {
    let data = [0x0d, 0x10, 0x11, 0x13, 0x18, 0x8d, 0x90, 0x91, 0x93];
    let mut out = Vec::new();
    escape(&data, &mut out);
    assert_eq!(out, data.into_iter().flat_map(|b| [ZDLE, b ^ 0x40]).collect::<Vec<_>>());
    escape(b"a", &mut out);
    assert_eq!(out.last(), Some(&b'a'));
}

#[test]
fn hex_headers_preserve_flags() {
    let h = Header::new(ZRINIT, [0, 0, 0, CANFDX | CANOVIO | CANFC32]);
    assert_eq!(Decoder::new().feed(&hex_header(h)), vec![Item::Header(h)]);
    assert_eq!(h.flags(), CANFDX | CANOVIO | CANFC32);
}

#[test]
fn binary_headers_support_crc16() {
    let h = Header::at(ZRPOS, 0x1813100d);
    assert_eq!(Decoder::new().feed(&bin_header(h, false)), vec![Item::Header(h)]);
}

#[test]
fn binary_headers_support_crc32() {
    let h = Header::at(ZRPOS, 0x9391908d);
    assert_eq!(Decoder::new().feed(&bin_header(h, true)), vec![Item::Header(h)]);
}

#[test]
fn positions_are_little_endian() {
    let h = Header::at(ZRPOS, 0x01020304);
    assert_eq!(h.data, [4, 3, 2, 1]);
    assert_eq!(h.position(), 0x01020304);
}

#[test]
fn consecutive_subpackets_preserve_every_byte_with_crc16() {
    let (bytes, expected) = stream(false);
    assert_eq!(Decoder::new().feed(&bytes), expected);
}

#[test]
fn consecutive_subpackets_preserve_every_byte_with_crc32() {
    let (bytes, expected) = stream(true);
    assert_eq!(Decoder::new().feed(&bytes), expected);
}

#[test]
fn every_byte_boundary_can_split_the_stream() {
    for wide in [false, true] {
        let (bytes, expected) = stream(wide);
        let mut decoder = Decoder::new();
        let actual: Vec<_> = bytes.chunks(1).flat_map(|c| decoder.feed(c)).collect();
        assert_eq!(actual, expected);
    }
}

#[test]
fn corrupted_data_is_rejected_and_a_new_header_recovers() {
    let h = Header::at(ZDATA, 0);
    let mut bytes = bin_header(h, true);
    let mut packet = subpacket(b"hello", ZCRCE, true);
    packet[2] ^= 1;
    bytes.extend(packet);
    bytes.extend(hex_header(Header::at(ZRPOS, 0)));
    assert_eq!(Decoder::new().feed(&bytes), vec![Item::Header(h), Item::BadCrc, Item::Header(Header::at(ZRPOS, 0))]);
}

#[test]
fn corrupted_hex_headers_are_rejected() {
    for byte in *b"1z" {
        let mut bytes = hex_header(Header::new(ZRQINIT, [0; 4]));
        bytes[5] = byte;
        assert_eq!(Decoder::new().feed(&bytes), vec![Item::BadCrc]);
    }
}

#[test]
fn shell_text_and_flow_control_before_headers_are_ignored() {
    let mut bytes = b"hello\r\n\x11\x13***oops".to_vec();
    let h = Header::at(ZACK, 123);
    bytes.extend(hex_header(h));
    assert_eq!(Decoder::new().feed(&bytes), vec![Item::Header(h)]);
}

#[test]
fn five_cancel_bytes_work_across_chunks() {
    let mut decoder = Decoder::new();
    assert!(decoder.feed(&[CAN; 4]).is_empty());
    assert_eq!(decoder.feed(&[CAN]), vec![Item::Cancel]);
    let h = Header::at(ZACK, 1);
    assert_eq!(decoder.feed(&hex_header(h)), vec![Item::Header(h)]);
}

#[test]
fn four_cancel_bytes_do_not_cancel() {
    assert!(Decoder::new().feed(&[CAN, CAN, CAN, CAN, b'a']).is_empty());
}

#[test]
fn flow_control_inside_a_subpacket_is_discarded() {
    let h = Header::at(ZDATA, 0);
    let mut bytes = bin_header(h, false);
    let mut packet = subpacket(b"abc", ZCRCE, false);
    packet.splice(1..1, [XON, XOFF, 0x91, 0x93]);
    bytes.extend(packet);
    assert_eq!(Decoder::new().feed(&bytes), vec![Item::Header(h), Item::Subpacket { data: b"abc".to_vec(), end: ZCRCE }]);
}

#[test]
fn detection_returns_the_earliest_direction() {
    assert_eq!(detect(b"text**\x18B00"), Some((4, Detected::Receive)));
    assert_eq!(detect(b"**\x18B01**\x18B00"), Some((0, Detected::Send)));
    assert_eq!(detect(b"nothing"), None);
}

#[test]
fn hex_data_headers_consume_their_tail_before_payload() {
    let h = Header::at(ZDATA, 0);
    let mut bytes = hex_header(h);
    bytes.extend(subpacket(b"\r\nhello", ZCRCW, false));
    let mut decoder = Decoder::new();
    let actual: Vec<_> = bytes.chunks(1).flat_map(|c| decoder.feed(c)).collect();
    assert_eq!(actual, vec![Item::Header(h), Item::Subpacket { data: b"\r\nhello".to_vec(), end: ZCRCW }]);
}

#[test]
fn oversized_subpackets_are_rejected() {
    let mut bytes = bin_header(Header::at(ZDATA, 0), false);
    bytes.extend(subpacket(&vec![b'a'; 8193], ZCRCE, false));
    assert!(Decoder::new().feed(&bytes).contains(&Item::BadCrc));
}

#[test]
fn invalid_escape_pairs_are_rejected() {
    let mut bytes = bin_header(Header::at(ZDATA, 0), false);
    bytes.extend([ZDLE, b'?']);
    assert_eq!(Decoder::new().feed(&bytes).last(), Some(&Item::BadCrc));
}

#[test]
fn rubout_escapes_are_accepted() {
    let h = Header::at(ZDATA, 0);
    let mut bytes = bin_header(h, false);
    let packet = subpacket(&[0x7f, 0xff], ZCRCE, false);
    bytes.extend([ZDLE, ZRUB0, ZDLE, ZRUB1]);
    bytes.extend(&packet[2..]);
    assert_eq!(Decoder::new().feed(&bytes), vec![Item::Header(h), Item::Subpacket { data: vec![0x7f, 0xff], end: ZCRCE }]);
}

#[test]
fn counted_items_report_how_far_the_input_was_read() {
    let fin = hex_header(Header::new(ZFIN, [0; 4]));
    let mut bytes = b"$ ".to_vec();
    bytes.extend(&fin);
    bytes.extend(b"OO");
    bytes.extend([CAN; 5]);
    bytes.extend(b"tail");
    let header_end = 2 + fin.len() - 2;
    let cancel_end = 2 + fin.len() + 2 + 5;
    assert_eq!(Decoder::new().feed_counted(&bytes), vec![(header_end, Item::Header(Header::new(ZFIN, [0; 4]))), (cancel_end, Item::Cancel)]);
}
