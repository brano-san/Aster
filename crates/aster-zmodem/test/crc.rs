use super::*;

#[test]
fn crc16_matches_the_reference_vector() { assert_eq!(crc16(b"123456789"), 0x31c3); }

#[test]
fn crc32_matches_the_reference_vector() { assert_eq!(crc32(b"123456789"), 0xcbf43926); }

#[test]
fn empty_input_has_zero_checksums() { assert_eq!(crc16(b""), 0); assert_eq!(crc32(b""), 0); }

#[test]
fn crc16_streaming_matches_a_single_update() {
    let data: Vec<u8> = (0..=255).cycle().take(10000).collect();
    let mut crc = Crc16::default();
    for chunk in data.chunks(7) { crc.update(chunk); }
    assert_eq!(crc.finish(), crc16(&data));
}

#[test]
fn crc32_streaming_matches_a_single_update() {
    let data: Vec<u8> = (0..=255).cycle().take(10000).collect();
    let mut crc = Crc32::default();
    for chunk in data.chunks(13) { crc.update(chunk); }
    assert_eq!(crc.finish(), crc32(&data));
}
