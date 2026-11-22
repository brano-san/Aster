use super::*;

fn resolve_with(spec: &str, config: &str) -> Destination {
    let cx = Context { config, ssh_dir: Path::new("/home/u/.ssh"), home: Path::new("/home/u"), read: &|_| None, default_user: "", default_term: "" };
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
    assert_eq!((d.host.as_str(), d.port, d.user.as_str()), ("example.com", 22, ""));
    assert!(!d.explicit_identities);
    assert_eq!((d.keepalive_ms, d.keepalive_count, d.ready_timeout_ms), (5000, 10, 20000));
    assert_eq!(d.term, "xterm-256color");
    assert!(!d.skip_banner && !d.remember_cwd && !d.x11);
}

#[test]
fn profile_options_override_destination_and_config() {
    let config = "Host web\n User config\n Port 2222\n ServerAliveInterval 3\n ServerAliveCountMax 7\n ConnectTimeout 12\n ForwardX11 yes\n SetEnv TERM=screen\n KexAlgorithms kex1,kex2\n MACs mac1,mac2\n Ciphers cipher\n Compression no\n IdentityFile ~/.ssh/config-key\n";
    let home = Path::new("/home/u");
    let ssh_dir = home.join(".ssh");
    let cx = Context { config, ssh_dir: &ssh_dir, home, read: &|_| None, default_user: "", default_term: "ansi" };
    let inherited = resolve("web", &cx);
    assert_eq!((inherited.keepalive_ms, inherited.keepalive_count, inherited.ready_timeout_ms), (3000, 7, 12000));
    assert_eq!(inherited.identity_files, [ssh_dir.join("config-key")]);
    assert!(inherited.x11);
    assert_eq!(inherited.term, "screen");
    assert_eq!(resolve("other", &cx).term, "ansi");
    let profile = SshOptions {
        user: Some("profile".into()), port: Some(24), keepalive_ms: Some(250), x11: Some(false),
        identity_files: Some(vec!["~/.ssh/one".into(), "~/.ssh/two".into()]), skip_banner: Some(true),
        term: Some("vt100".into()), remember_cwd: Some(true),
        algorithms: Some(AlgorithmOptions { kex: Some(Vec::new()), mac: Some(vec!["profile-mac".into()]), ..Default::default() }),
        ..Default::default()
    };
    let dest = resolve_options("explicit@web:23", &cx, &profile);
    assert_eq!((dest.user.as_str(), dest.port, dest.term.as_str()), ("profile", 24, "vt100"));
    assert_eq!((dest.keepalive_ms, dest.keepalive_count, dest.ready_timeout_ms), (250, 7, 12000));
    assert!(dest.remember_cwd && dest.skip_banner && !dest.x11);
    assert_eq!(dest.identity_files, [ssh_dir.join("one"), ssh_dir.join("two")]);
    assert!(dest.algorithms.kex.is_empty());
    assert_eq!(dest.algorithms.cipher, ["cipher"]);
    assert_eq!(dest.algorithms.mac, ["profile-mac"]);
    assert_eq!(dest.algorithms.compression, ["none"]);
}

#[test]
fn modified_algorithm_lists_fail_closed_unless_overridden() {
    for prefix in ["-", "+", "^"] {
        let config = format!("Ciphers {prefix}absent,aes128-ctr\n");
        let home = Path::new("/home/u");
        let ssh_dir = home.join(".ssh");
        let cx = Context { config: &config, ssh_dir: &ssh_dir, home, read: &|_| None, default_user: "", default_term: "" };
        let inherited = resolve("web", &cx);
        let (result, warnings) = crate::algorithms::preferred(&inherited.algorithms);
        assert!(result.is_err());
        assert!(warnings.iter().any(|w| w.contains(&format!("{prefix}absent,aes128-ctr")) && w.contains("explicit list")));
        let profile = SshOptions { algorithms: Some(AlgorithmOptions { cipher: Some(vec!["aes128-ctr".into()]), ..Default::default() }), ..Default::default() };
        let (result, warnings) = crate::algorithms::preferred(&resolve_options("web", &cx, &profile).algorithms);
        assert!(result.is_ok() && warnings.is_empty());
    }
}

#[test]
fn discovery_includes_only_existing_keys_in_preferred_order() {
    let home = crate::test_server::temp_ssh_dir();
    let ssh_dir = home.join(".ssh");
    std::fs::create_dir(&ssh_dir).unwrap();
    let cx = Context { config: "", ssh_dir: &ssh_dir, home: &home, read: &|_| None, default_user: "", default_term: "" };
    assert!(resolve("box", &cx).identity_files.is_empty());
    for name in ["id_rsa", "id_ed25519"] {
        std::fs::write(ssh_dir.join(name), []).unwrap();
    }
    assert_eq!(resolve("box", &cx).identity_files, [ssh_dir.join("id_ed25519"), ssh_dir.join("id_rsa")]);
    std::fs::write(ssh_dir.join("id_ecdsa"), []).unwrap();
    assert_eq!(resolve("box", &cx).identity_files, ["id_ed25519", "id_ecdsa", "id_rsa"].map(|name| ssh_dir.join(name)));
    std::fs::write(ssh_dir.join("id_work"), []).unwrap();
    std::fs::write(ssh_dir.join("id_work.pub"), []).unwrap();
    std::fs::create_dir(ssh_dir.join("id_folder")).unwrap();
    assert_eq!(resolve("box", &cx).identity_files, ["id_ed25519", "id_ecdsa", "id_rsa", "id_work"].map(|name| ssh_dir.join(name)));
    std::fs::remove_dir_all(home).unwrap();
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
fn explicit_profile_proxy_overrides_config_and_uses_resolved_tokens() {
    let config = "Host db\n HostName 10.1.2.3\n User ann\n Port 2022\n ProxyJump bastion\n";
    let dir = Path::new("/home/u/.ssh");
    let cx = Context { config, ssh_dir: dir, home: Path::new("/home/u"), read: &|_| None, default_user: "", default_term: "" };
    let mut options = SshOptions { proxy: Some(SshProxy::Command { command: "tunnel %h %p %r %%".into() }), ..Default::default() };
    let command = resolve_options("db", &cx, &options);
    assert!(command.proxy_jump.is_empty());
    assert_eq!(command.proxy_command_line().as_deref(), Some("tunnel 10.1.2.3 2022 ann %"));
    options.proxy = Some(SshProxy::Direct);
    let direct = resolve_options("db", &cx, &options);
    assert!(direct.proxy_jump.is_empty() && direct.proxy_command_line().is_none());
}

#[test]
fn explicitly_blank_user_requests_a_login_instead_of_inheriting_one() {
    let cx = Context { config: "User ann", ssh_dir: Path::new("/home/.ssh"), home: Path::new("/home"), read: &|_| None, default_user: "fallback", default_term: "" };
    let options = SshOptions { user: Some(String::new()), ..Default::default() };
    assert!(resolve_options("legacy@host", &cx, &options).user.is_empty());
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

#[test]
fn explicit_profile_agent_forwarding_overrides_the_ssh_configuration() {
    let cx = Context { config: "ForwardAgent yes", ssh_dir: Path::new("/home/.ssh"), home: Path::new("/home"), read: &|_| None, default_user: "", default_term: "" };
    let options = SshOptions { forward_agent: Some(false), ..Default::default() };
    assert_eq!(resolve_options("host", &cx, &options).forward_agent, Some(false));
}
