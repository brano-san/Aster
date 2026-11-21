use super::*;

#[test]
fn quick_connect_defaults_to_ssh_and_parses_each_configured_protocol() {
    for (provider, query, value) in [
        (Provider::default(), "root@node:2222", "ssh://root@node:2222"),
        (Provider::Telnet, "router:2323", "telnet://router:2323"),
        (Provider::Telnet, "router", "telnet://router:23"),
        (Provider::Tcp, "[::1]:7000", "tcp://[::1]:7000"),
        (Provider::Serial, "TTY@9600", "serial:TTY:9600"),
    ] {
        let (target, _) = QuickTarget::parse(query, Some(provider), true).unwrap();
        assert_eq!(target.value(), value);
        assert_eq!(QuickTarget::parse(value, None, true).unwrap().0.value(), value);
    }
}

#[test]
fn quick_connect_explicit_types_override_the_default_and_validate_addresses() {
    for (query, expected) in [
        ("TELNET://router", "telnet://router:23"),
        ("tcp://node:1024", "tcp://node:1024"),
        ("serial:COM3:115200,7E2", "serial:COM3:115200,7E2"),
        ("COM3@9600", "serial:COM3:9600"),
        ("/dev/ttyUSB0@115200", "serial:/dev/ttyUSB0:115200"),
        ("ssh://COM3@9600", "ssh://COM3@9600"),
    ] {
        let (target, explicit) = QuickTarget::parse(query, Some(Provider::Tcp), true).unwrap();
        assert!(explicit);
        assert_eq!(target.value(), expected);
    }
    for query in ["tcp://node", "telnet://node:0", "serial:", "serial:COM3@0", "COM3@bad", "other://node", "ssh://"] {
        assert!(QuickTarget::parse(query, Some(Provider::Ssh), true).is_none(), "{query}");
    }
    assert!(QuickTarget::parse("ssh://node", Some(Provider::Serial), false).is_none());
}

#[test]
fn quick_connect_builds_the_profile_used_to_spawn_each_connection() {
    let profile = |query| QuickTarget::parse(query, Some(Provider::Ssh), true).unwrap().0.profile(Some("ssh")).unwrap();
    assert_eq!(profile("root@node:2222").ssh.as_deref(), Some("root@node:2222"));
    assert_eq!(profile("telnet://router").socket.as_deref(), Some("telnet://router:23"));
    assert_eq!(profile("tcp://node:7000").socket.as_deref(), Some("tcp://node:7000"));
    assert_eq!(profile("COM3@9600").serial.as_deref(), Some("COM3:9600"));
}

#[test]
fn a_bare_serial_port_leaves_the_baud_rate_to_profile_defaults() {
    let (target, _) = QuickTarget::parse("COM3", Some(Provider::Serial), false).unwrap();
    assert_eq!(target.label(), "COM3");
    assert_eq!(target.profile(None).unwrap().serial.as_deref(), Some("COM3"));
}
