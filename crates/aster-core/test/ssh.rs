use super::*;

fn t(user: Option<&str>, host: &str, port: Option<u16>) -> Option<Target> {
    Some(Target { user: user.map(str::to_owned), host: host.to_owned(), port })
}

#[test]
fn targets_parse_users_ports_and_ipv6() {
    assert_eq!(parse_target("example.com"), t(None, "example.com", None));
    assert_eq!(parse_target("root@10.0.0.2"), t(Some("root"), "10.0.0.2", None));
    assert_eq!(parse_target("me@host:2222"), t(Some("me"), "host", Some(2222)));
    assert_eq!(parse_target("[::1]:22"), t(None, "::1", Some(22)));
    assert_eq!(parse_target("me@[fe80::1]"), t(Some("me"), "fe80::1", None));
    assert_eq!(parse_target("fe80::1"), t(None, "fe80::1", None));
    assert_eq!(parse_target("a.b@c@d"), t(Some("a.b@c"), "d", None));
    for bad in ["", "two words", "@host", "host:port", "host:99999", "[::1", "ho/st"] {
        assert_eq!(parse_target(bad), None, "{bad}");
    }
}

#[test]
fn targets_become_openssh_arguments_and_profiles() {
    let target = parse_target("me@[::1]:2222").unwrap();
    assert_eq!(target.args(), ["-p", "2222", "me@::1"]);
    assert_eq!(target.display(), "me@[::1]:2222");
    let p = target.profile("ssh");
    assert_eq!((p.command.as_str(), p.name.as_str(), p.id.as_str()), ("ssh", "me@[::1]:2222", "ssh:me@[::1]:2222"));
    let h = host_profile("prod", "/usr/bin/ssh");
    assert_eq!((h.name.as_str(), h.args.as_slice()), ("SSH: prod", &["prod".to_owned()][..]));
}
