use super::*;

fn connect(host: &str, port: u16) -> Step { Step::Connect { host: host.into(), port } }
fn five(request: &[u8]) -> Vec<Step> {
    let mut h = Handshake::new();
    assert_eq!(h.feed(&[5, 1, 0]), vec![Step::Reply(vec![5, 0])]);
    h.feed(request)
}

#[test]
fn five_accepts_ipv4() { assert_eq!(five(&[5,1,0,1,1,2,3,4,0,80]), vec![connect("1.2.3.4",80)]); }
#[test]
fn five_accepts_domain() { assert_eq!(five(b"\x05\x01\x00\x03\x03abc\x01\xbb"), vec![connect("abc",443)]); }
#[test]
fn five_accepts_ipv6() {
    let mut b = vec![5,1,0,4]; b.extend_from_slice(&std::net::Ipv6Addr::LOCALHOST.octets()); b.extend_from_slice(&80u16.to_be_bytes());
    assert_eq!(five(&b), vec![connect("::1",80)]);
}
#[test]
fn missing_anonymous_method_fails() { assert_eq!(Handshake::new().feed(&[5,2,1,2]),vec![Step::Fail(vec![5,255])]); }
#[test]
fn bind_command_fails() { assert_eq!(five(&[5,2,0,1]), vec![Step::Fail(Handshake::reply(7))]); }
#[test]
fn unknown_address_fails() { assert_eq!(five(&[5,1,0,9]),vec![Step::Fail(Handshake::reply(8))]); }
#[test]
fn greeting_and_request_share_one_chunk() {
    assert_eq!(Handshake::new().feed(&[5,1,0,5,1,0,1,1,2,3,4,0,80]),vec![Step::Reply(vec![5,0]),connect("1.2.3.4",80)]);
}
#[test]
fn individual_bytes_preserve_state() {
    let mut h=Handshake::new(); let mut steps=Vec::new();
    for b in [5,1,0,5,1,0,1,1,2,3,4,0,80] { steps.extend(h.feed(&[b]).into_iter().filter(|s| *s != Step::More)); }
    assert_eq!(steps,vec![Step::Reply(vec![5,0]),connect("1.2.3.4",80)]);
}
#[test]
fn rest_is_taken_once() {
    let mut h=Handshake::new(); h.feed(&[5,1,0,5,1,0,1,1,2,3,4,0,80,42,43]);
    assert_eq!(h.rest(),[42,43]); assert!(h.rest().is_empty());
}
#[test]
fn four_accepts_ipv4() { assert_eq!(Handshake::new().feed(&[4,1,0,80,1,2,3,4,0]),vec![connect("1.2.3.4",80)]); }
#[test]
fn four_accepts_domain() { assert_eq!(Handshake::new().feed(b"\x04\x01\x00\x50\x00\x00\x00\x01user\x00host\x00"),vec![connect("host",80)]); }
#[test]
fn four_rejects_bind() { assert_eq!(Handshake::new().feed(&[4,2]),vec![Step::Fail(vec![0,91,0,0,0,0,0,0])]); }
#[test]
fn unknown_version_fails() { assert_eq!(Handshake::new().feed(&[7]),vec![Step::Fail(vec![])]); }
#[test]
fn replies_match_the_version() {
    let mut h=Handshake::default(); assert_eq!(h.success(),Handshake::reply(0)); assert_eq!(h.failure(),Handshake::reply(1));
    h.feed(&[4]); assert_eq!(h.success(),[0,90,0,0,0,0,0,0]); assert_eq!(h.failure(),[0,91,0,0,0,0,0,0]);
}
#[test]
fn unterminated_request_is_bounded() {
    let mut b=vec![4,1,0,80,1,2,3,4]; b.extend(vec![1;1017]);
    assert!(matches!(Handshake::new().feed(&b).as_slice(),[Step::Fail(_)]));
    let mut b=vec![5,1]; b.extend(vec![1;1025]); assert!(matches!(Handshake::new().feed(&b).as_slice(),[Step::Fail(_)]));
}
#[test]
fn invalid_utf8_fails() { assert_eq!(five(&[5,1,0,3,1,255,0,80]),vec![Step::Fail(Handshake::reply(1))]); }
