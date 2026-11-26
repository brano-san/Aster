use super::*;

#[test]
fn profile_file_imports_connections_and_skips_invalid_entries() {
    let yaml = r#"
- name: Build box
  group: Work
  options:
    host: build.example.test
    user: deploy
    port: 2200
    privateKeys: [~/.ssh/build]
    agentForward: false
    keepaliveInterval: 15000
    forwardedPorts:
      - type: Local
        host: 127.0.0.1
        port: 8080
        targetAddress: localhost
        targetPort: 80
      - type: Dynamic
        host: ::1
        port: 1080
- name: Bad port
  options: {host: bad.example.test, port: invalid}
- name: No host
  options: {}
- name: Build box
  options: {host: duplicate.example.test}
- name: IPv6
  options: {host: '2001:db8::42'}
"#;
    let profiles = parse_file_profiles(yaml, "ssh", &HashMap::new()).unwrap();
    assert_eq!(
        profiles
            .iter()
            .map(|profile| profile.name.as_str())
            .collect::<Vec<_>>(),
        ["Build box", "IPv6"]
    );
    let build = &profiles[0];
    assert!(build.builtin);
    assert_eq!(build.group.as_deref(), Some("Work"));
    assert_eq!(build.ssh.as_deref(), Some("deploy@build.example.test:2200"));
    assert_eq!(
        build.args.last().map(String::as_str),
        Some("deploy@build.example.test")
    );
    let options = build.ssh_options.as_ref().unwrap();
    assert_eq!(
        options.identity_files.as_deref(),
        Some(["~/.ssh/build".to_owned()].as_slice())
    );
    assert_eq!(options.forward_agent, Some(false));
    assert_eq!(options.keepalive_ms, Some(15000));
    let forwards = options.forwards.as_ref().unwrap();
    assert_eq!(forwards[0].destination.as_deref(), Some("localhost:80"));
    assert_eq!(forwards[1].listen, "[::1]:1080");
    assert_eq!(profiles[1].ssh.as_deref(), Some("[2001:db8::42]"));
}
