use super::*;

fn address(protocol: Protocol, host: &str, port: u16) -> Address {
    Address { protocol, host: host.to_owned(), port }
}

#[test]
fn bare_host_is_telnet_on_port_23() {
    assert_eq!(parse("router.lan"), Ok(address(Protocol::Telnet, "router.lan", 23)));
    assert_eq!(parse(" 10.0.0.1:2323 "), Ok(address(Protocol::Telnet, "10.0.0.1", 2323)));
}

#[test]
fn schemes_choose_the_protocol() {
    assert_eq!(parse("telnet://host"), Ok(address(Protocol::Telnet, "host", 23)));
    assert_eq!(parse("TELNET://host:24/"), Ok(address(Protocol::Telnet, "host", 24)));
    assert_eq!(parse("tcp://host:7000"), Ok(address(Protocol::Raw, "host", 7000)));
    assert_eq!(parse("raw://host:7000"), Ok(address(Protocol::Raw, "host", 7000)));
}

#[test]
fn ipv6_hosts_use_brackets() {
    assert_eq!(parse("telnet://[::1]:23"), Ok(address(Protocol::Telnet, "::1", 23)));
    assert_eq!(parse("[fe80::1%eth0]"), Ok(address(Protocol::Telnet, "fe80::1%eth0", 23)));
    assert_eq!(parse("fe80::1"), Ok(address(Protocol::Telnet, "fe80::1", 23)));
}

#[test]
fn raw_needs_a_port() {
    assert!(parse("tcp://host").is_err());
}

#[test]
fn invalid_addresses_fail() {
    for text in ["", "telnet://", "host:port", "host:0", "host:70000", "user@host", "a b", "[::1]x", "host/path"] {
        assert!(parse(text).is_err(), "{text}");
    }
}

#[test]
fn urls_need_a_scheme() {
    assert_eq!(parse_url("telnet://host"), Some(address(Protocol::Telnet, "host", 23)));
    assert_eq!(parse_url("tcp://host:9"), Some(address(Protocol::Raw, "host", 9)));
    assert_eq!(parse_url("host"), None);
    assert_eq!(parse_url("telnet://"), None);
    assert_eq!(parse_url("tcp://host"), None);
}

#[test]
fn display_round_trips() {
    for text in ["telnet://host:23", "tcp://10.0.0.1:7000", "telnet://[::1]:2323"] {
        assert_eq!(parse(text).unwrap().display(), text);
    }
    assert_eq!(address(Protocol::Telnet, "::1", 23).authority(), "[::1]:23");
}
