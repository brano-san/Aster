use super::*;

#[test]
fn a_character_split_between_chunks_waits_for_its_tail() {
    let (mut pending, mut out) = (b"a\xd0".to_vec(), String::new());
    decode(&mut pending, &mut out);
    assert_eq!((out.as_str(), pending.as_slice()), ("a", &b"\xd0"[..]));
    pending.extend_from_slice(b"\xbf!");
    decode(&mut pending, &mut out);
    assert_eq!((out.as_str(), pending.len()), ("aп!", 0));
}

#[test]
fn invalid_bytes_become_replacement_characters() {
    let (mut pending, mut out) = (b"x\xffy".to_vec(), String::new());
    decode(&mut pending, &mut out);
    assert_eq!((out.as_str(), pending.len()), ("x\u{fffd}y", 0));
}
