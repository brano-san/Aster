use super::*;

#[test]
fn full_line() {
    assert_eq!(
        HexDump::new().format(b"Hello, world!\n\x00\xff"),
        "00000000  48 65 6c 6c 6f 2c 20 77  6f 72 6c 64 21 0a 00 ff  |Hello, world!...|\r\n"
    );
}

#[test]
fn partial_line() {
    assert_eq!(
        HexDump::new().format(b"AB"),
        "00000000  41 42                                             |AB|\r\n"
    );
}

#[test]
fn two_full_lines() {
    let result = HexDump::new().format(&[0; 32]);
    let lines: Vec<_> = result.split_terminator("\r\n").collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("00000000  "));
    assert!(lines[1].starts_with("00000010  "));
    assert!(lines
        .iter()
        .all(|line| line.ends_with("|................|")));
}

#[test]
fn offsets_accumulate_without_merging_lines() {
    let mut dump = HexDump::new();
    assert!(dump.format(b"AB").ends_with("|AB|\r\n"));
    assert!(dump.format(b"C").starts_with("00000002  43"));
    assert!(dump.format(b"D").starts_with("00000003  44"));
}

#[test]
fn empty_does_not_advance_offset() {
    let mut dump = HexDump::default();
    assert_eq!(dump.format(b""), "");
    assert!(dump.format(b"a").starts_with("00000000"));
}

#[test]
fn partial_lines_align_at_every_width() {
    for length in 1..=16 {
        let result = HexDump::new().format(&vec![b'a'; length]);
        assert_eq!(result.find('|'), Some(60));
        assert!(result.ends_with(&format!("|{}|\r\n", "a".repeat(length))));
    }
}

#[test]
fn ascii_printable_boundaries() {
    assert!(HexDump::new()
        .format(&[0x1f, 0x20, 0x7e, 0x7f])
        .ends_with("|. ~.|\r\n"));
}
