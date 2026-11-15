use super::*;

#[test]
fn keep() {
    assert_eq!(
        NewlineConverter::new(Newlines::Keep).convert(b"a\r\nb\rc\nd"),
        b"a\r\nb\rc\nd"
    );
}

#[test]
fn strip() {
    assert_eq!(
        NewlineConverter::new(Newlines::Strip).convert(b"a\r\nb\rc\nd"),
        b"abcd"
    );
}

#[test]
fn cr() {
    assert_eq!(
        NewlineConverter::new(Newlines::Cr).convert(b"a\r\nb\rc\nd"),
        b"a\rb\rc\rd"
    );
}

#[test]
fn lf() {
    assert_eq!(
        NewlineConverter::new(Newlines::Lf).convert(b"a\r\nb\rc\nd"),
        b"a\nb\nc\nd"
    );
}

#[test]
fn crlf() {
    assert_eq!(
        NewlineConverter::new(Newlines::CrLf).convert(b"a\r\nb\rc\nd"),
        b"a\r\nb\r\nc\r\nd"
    );
}

#[test]
fn implicit_cr() {
    assert_eq!(
        NewlineConverter::new(Newlines::ImplicitCr).convert(b"a\r\nb\rc\nd"),
        b"a\r\nb\rc\r\nd"
    );
}

#[test]
fn implicit_lf() {
    assert_eq!(
        NewlineConverter::new(Newlines::ImplicitLf).convert(b"a\r\nb\rc\nd"),
        b"a\r\nb\r\nc\nd"
    );
}

#[test]
fn split_pairs() {
    for mode in [Newlines::CrLf, Newlines::ImplicitCr, Newlines::ImplicitLf] {
        let mut converter = NewlineConverter::new(mode);
        let mut result = converter.convert(b"a\r");
        assert!(converter.convert(b"").is_empty());
        result.extend(converter.convert(b"\nb"));
        assert_eq!(result, b"a\r\nb");
    }
}

#[test]
fn delayed_implicit_lf() {
    let mut converter = NewlineConverter::new(Newlines::ImplicitLf);
    assert_eq!(converter.convert(b"a\r"), b"a\r");
    assert_eq!(converter.convert(b"b"), b"\nb");
}

#[test]
fn empty() {
    assert!(NewlineConverter::new(Newlines::CrLf)
        .convert(b"")
        .is_empty());
}

#[test]
fn arbitrary_bytes() {
    let bytes: Vec<u8> = (0..=255).collect();
    assert_eq!(NewlineConverter::new(Newlines::Keep).convert(&bytes), bytes);
}

#[test]
fn repeated_crs_are_individual_newlines() {
    assert_eq!(
        NewlineConverter::new(Newlines::Lf).convert(b"\r\r\n\n"),
        b"\n\n\n"
    );
}
