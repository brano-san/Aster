use super::*;

fn grant(fake: u8, local: Option<u8>) -> X11 {
    X11 { display: Display::parse("localhost:3.2").unwrap(), cookie: [fake; 16], local_cookie: local.map(|b| [b; 16]) }
}

#[test]
fn accepts_only_local_displays_and_valid_screen_numbers() {
    let display = Display::parse("localhost:3.2").unwrap();
    assert_eq!((display.number, display.screen), (3, 2));
    assert!(!display.unix);
    assert_eq!(Display::parse(":0").unwrap().unix, cfg!(target_os = "linux"));
    for invalid in ["other:1", "localhost:60000", "localhost:-1", ":0.-2", ":0.1.2", ":0.", ":", ""] {
        assert!(Display::parse(invalid).is_err());
    }
}

fn record(family: u16, addr: &[u8], display: &[u8], cookie: u8) -> Vec<u8> {
    let mut out = family.to_be_bytes().to_vec();
    for field in [addr, display, PROTOCOL, &[cookie; 16]] {
        out.extend_from_slice(&(field.len() as u16).to_be_bytes());
        out.extend_from_slice(field);
    }
    out
}

#[test]
fn selects_authority_for_the_local_host_and_display() {
    let display = Display::parse("localhost:3.2").unwrap();
    let mut records = record(65535, b"", b"3", 1);
    records.extend(record(256, b"other", b"3", 2));
    records.extend(record(0, &[127, 0, 0, 1], b"4", 3));
    records.extend(record(256, b"mine", b"3", 4));
    assert_eq!(authority(&records, &display, b"mine").unwrap(), Some([4; 16]));
    records.extend(record(0, &[127, 0, 0, 1], b"3", 5));
    assert_eq!(authority(&records, &display, b"mine").unwrap(), Some([5; 16]));
    assert!(authority(&records[..records.len()-1], &display, b"mine").is_err());
}

#[tokio::test]
async fn replaces_setup_cookie_in_both_byte_orders_without_reading_application_data() {
    let grant = grant(42, Some(3));
    for order in *b"lB" {
        let mut sent = setup(order, Some(&grant.cookie));
        sent.extend_from_slice(b"next");
        let mut incoming = sent.as_slice();
        let (received_order, cookie) = read_setup(&mut incoming).await.unwrap();
        assert_eq!(cookie, grant.cookie);
        assert_eq!(incoming, b"next");
        let local = setup(received_order, grant.local_cookie.as_ref());
        assert_eq!(&local[32..48], &[3; 16]);
        assert_eq!(local.len(), 48);
        let anonymous = setup(order, None);
        assert_eq!(anonymous.len(), 12);
        assert_eq!(&anonymous[6..10], &[0; 4]);
    }
}

#[tokio::test]
async fn rejects_invalid_or_oversized_setup_before_opening_local_display() {
    let registry = Grants::default();
    let holder = Arc::new(grant(42, None));
    registry.register(&holder);
    let wrong = setup(b'l', Some(&[99; 16]));
    assert_eq!(registry.relay(&mut std::io::Cursor::new(wrong)).await.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    for (offset, byte) in [(0, b'X'), (2, 4), (6, 255), (8, 255), (12, b'N')] {
        let mut invalid = setup(b'l', Some(&holder.cookie));
        invalid[offset] = byte;
        assert!(read_setup(&mut invalid.as_slice()).await.is_err());
    }
}

#[test]
fn closing_one_session_revokes_only_its_own_x11_cookie() {
    let registry = Grants::default();
    let one = Arc::new(grant(1, None));
    let two = Arc::new(grant(2, None));
    registry.register(&one);
    registry.register(&two);
    assert!(registry.lookup(&one.cookie).is_some());
    drop(one);
    assert!(registry.lookup(&[1; 16]).is_none());
    assert!(registry.active());
    assert!(registry.lookup(&two.cookie).is_some());
    drop(two);
    assert!(!registry.active());
}
