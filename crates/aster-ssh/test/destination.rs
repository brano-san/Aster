use super::*;

fn resolve_with(spec: &str, config: &str) -> Destination {
    let cx = Context { config, ssh_dir: Path::new("/home/u/.ssh"), home: Path::new("/home/u"), read: &|_| None, default_user: "me" };
    resolve(spec, &cx)
}

#[test]
fn destinations_split_into_user_host_and_port() {
    assert_eq!(split("root@example.com:2222"), (Some("root".into()), "example.com".into(), Some(2222)));
    assert_eq!(split("[::1]:22"), (None, "::1".into(), Some(22)));
    assert_eq!(split("fe80::1"), (None, "fe80::1".into(), None));
    assert_eq!(split("a@b@host"), (Some("a@b".into()), "host".into(), None));
}

#[test]
fn an_alias_takes_its_host_name_user_port_and_keys_from_the_config() {
    let d = resolve_with("web", "Host web\n  HostName 10.0.0.5\n  User deploy\n  Port 2200\n  IdentityFile ~/.ssh/deploy\n");
    assert_eq!((d.host.as_str(), d.port, d.user.as_str()), ("10.0.0.5", 2200, "deploy"));
    assert!(d.explicit_identities);
    assert_eq!(d.identity_files.len(), 1);
}

#[test]
fn values_typed_in_the_destination_win_over_the_config() {
    let d = resolve_with("ops@web:23", "Host web\n  User deploy\n  Port 2200\n");
    assert_eq!((d.user.as_str(), d.port), ("ops", 23));
}

#[test]
fn defaults_fill_in_the_rest() {
    let d = resolve_with("example.com", "");
    assert_eq!((d.host.as_str(), d.port, d.user.as_str()), ("example.com", 22, "me"));
    assert!(!d.explicit_identities);
    assert_eq!(d.identity_files.first().unwrap(), &Path::new("/home/u/.ssh").join("id_ed25519"));
}

#[test]
fn jump_hosts_are_listed_in_order_and_none_disables_them() {
    let d = resolve_with("db", "Host db\n  ProxyJump bastion, edge:2222\n");
    assert_eq!(d.proxy_jump, ["bastion", "edge:2222"]);
    let d = resolve_with("db", "Host db\n  ProxyJump none\n  ProxyCommand none\n");
    assert!(d.proxy_jump.is_empty() && d.proxy_command.is_none());
}

#[test]
fn proxy_commands_expand_their_tokens() {
    let d = resolve_with("box", "Host box\n  HostName 1.2.3.4\n  User ann\n  ProxyCommand nc -X 5 %h %p %r %n 100%% %q\n");
    assert_eq!(d.proxy_command_line().as_deref(), Some("nc -X 5 1.2.3.4 22 ann box 100% %q"));
}

#[test]
fn agent_forwarding_and_the_agent_socket_come_from_the_config() {
    let d = resolve_with("box", "Host box\n  ForwardAgent yes\n  IdentityAgent ~/.ssh/agent.sock\n");
    assert_eq!(d.forward_agent, Some(true));
    assert_eq!(d.agent.as_deref(), Some(Path::new("/home/u").join(".ssh/agent.sock").to_string_lossy().as_ref()));
    let d = resolve_with("other", "Host *\n  ForwardAgent no\n  IdentityAgent SSH_AUTH_SOCK\n");
    assert_eq!((d.forward_agent, d.agent), (Some(false), None));
    assert_eq!(resolve_with("plain", "").forward_agent, None);
}
