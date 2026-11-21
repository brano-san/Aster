use super::*;
use crate::config::Config;

#[test]
fn exit_policy_accounts_for_status_hold_and_connection_kind() {
    let mut profile = Profile::default();
    assert_eq!(profile.exit_decision(Some(0)), ExitDecision::Close);
    assert_eq!(profile.exit_decision(None), ExitDecision::Keep);
    assert_eq!(profile.exit_decision(Some(1)), ExitDecision::Keep);
    profile.hold = true;
    assert_eq!(profile.exit_decision(Some(0)), ExitDecision::Keep);
    profile.on_exit = OnExit::Close;
    assert_eq!(profile.exit_decision(Some(1)), ExitDecision::Close);
    profile.on_exit = OnExit::Keep;
    assert_eq!(profile.exit_decision(Some(0)), ExitDecision::Keep);
    profile.on_exit = OnExit::Reconnect;
    assert_eq!(profile.exit_decision(Some(0)), ExitDecision::Keep);
    for remote in [
        Profile { ssh: Some("host".into()), ..profile.clone() },
        Profile { serial: Some("port".into()), ..profile.clone() },
        Profile { socket: Some("tcp://host:23".into()), ..profile },
    ] {
        assert_eq!(remote.exit_decision(Some(0)), ExitDecision::Reconnect);
        assert_eq!(remote.exit_decision(Some(1)), ExitDecision::Reconnect);
    }
}

#[test]
fn explicit_exit_policies_round_trip_and_omitted_ones_inherit() {
    for (value, policy) in [("auto", OnExit::Auto), ("close", OnExit::Close), ("keep", OnExit::Keep), ("reconnect", OnExit::Reconnect)] {
        let config = Config::parse(&format!("[[profiles]]\non-exit = '{value}'")).unwrap();
        assert_eq!(config.profiles[0].on_exit, policy);
        assert_eq!(Config::parse(&config.to_text()).unwrap().profiles[0].on_exit, policy);
    }
    let profile = Profile::default();
    assert_eq!(profile.on_exit, OnExit::Inherit);
    assert!(!toml::to_string(&profile).unwrap().contains("on-exit"));
}

#[test]
fn stream_options_parse_from_a_profile_table() {
    let text = r#"
[[profiles]]
id = "custom:switch"
name = "Switch"
socket = "telnet://switch.lan:23"

[profiles.stream]
input = "line"
output = "hex"
newlines-in = "implicit-cr"
newlines-out = "crlf"
backspace = "ctrl-?"

[[profiles.stream.login]]
expect = "login:"
send = 'admin\r'

[[profiles.stream.login]]
expect = "[Pp]assword:"
send = 'secret\r'
regex = true
"#;
    let config = Config::parse(text).unwrap();
    let stream = config.profiles[0].stream.clone().unwrap();
    let options = stream.options();
    assert_eq!(options.input, aster_proto::input::InputMode::Readline);
    assert!(options.hex_output);
    assert_eq!(options.newlines_in, aster_proto::newline::Newlines::ImplicitCr);
    assert_eq!(options.newlines_out, aster_proto::newline::Newlines::CrLf);
    assert_eq!(options.backspace, aster_proto::input::Backspace::CtrlQuestion);
    assert_eq!(options.login.len(), 2);
    assert_eq!(options.login[0].send, r"admin\r");
    assert!(options.login[1].regex);
    let again = Config::parse(&config.to_text()).unwrap();
    assert_eq!(again.profiles[0].stream, Some(stream));
}

#[test]
fn profiles_without_a_stream_table_stay_plain() {
    let config = Config::parse("[[profiles]]\nname = \"Shell\"\ncommand = \"bash\"\n").unwrap();
    assert_eq!(config.profiles[0].stream, None);
    assert!(Stream::default().options().is_plain());
    assert!(!config.to_text().contains("stream"));
}

#[test]
fn group_paths_follow_parents_and_tolerate_unknown_or_cyclic_links() {
    let mut config = Config::parse(r#"
[[groups]]
id = "ops"
name = "Operations"
[[groups]]
id = "edge"
name = "Edge"
parent = "ops"
[[groups]]
name = "Lab"
parent = "Edge"
"#).unwrap();
    assert_eq!(group_path("Lab", &config), "Operations › Edge › Lab");
    assert_eq!(group_path("Legacy", &config), "Legacy");
    config.groups[0].parent = Some("missing".into());
    assert_eq!(group_path("Edge", &config), "Operations › Edge");
    config.groups[0].parent = Some("edge".into());
    assert_eq!(group_path("Edge", &config), "Operations › Edge");
}

#[test]
fn effective_profile_layers_kind_ancestors_and_own_fields_without_mutation() {
    let config = Config::parse(r##"
[profile-defaults.local]
cwd = "unused"
[profile-defaults.ssh]
env = { base = "1", override = "base" }
cwd = "/home"
color = "#777777"
on-exit = "reconnect"
stream = { input = "line", output = "hex" }
ssh-options = { port = 22, x11 = true, algorithms = { cipher = ["cipher"], kex = ["base"] } }
[[groups]]
id = "ops"
name = "Operations"
icon = "server"
color = "#aaaaaa"
[groups.defaults.ssh]
env = { outer = "1", override = "outer" }
cwd = "/outer"
scheme = "Outer"
ssh-options = { port = 2222 }
[[groups]]
name = "Production"
parent = "ops"
[groups.defaults.ssh]
env = { inner = "1", override = "inner" }
cwd = "/inner"
color = "#bbbbbb"
[[profiles]]
id = "web"
name = "Web"
group = "Production"
ssh = "host"
env = { override = "own" }
scheme = "Own"
on-exit = "auto"
stream = { input = "normal" }
ssh-options = { x11 = false, algorithms = { kex = [] } }
"##).unwrap();
    let raw = &config.profiles[0];
    let before = config.to_text();
    let p = effective(raw, &config);
    assert_eq!(p.env, [("base", "1"), ("outer", "1"), ("inner", "1"), ("override", "own")].map(|(k, v)| (k.to_owned(), v.to_owned())).into());
    assert_eq!(p.cwd.as_deref(), Some("/inner"));
    assert_eq!(p.scheme.as_deref(), Some("Own"));
    assert_eq!(p.color.as_deref(), Some("#bbbbbb"));
    assert_eq!(p.icon.as_deref(), Some("server"));
    assert_eq!(p.on_exit, OnExit::Auto);
    assert_eq!(p.stream.as_ref().unwrap().input_mode(), InputMode::Normal);
    assert!(p.stream.unwrap().options().hex_output);
    let ssh = p.ssh_options.unwrap();
    assert_eq!(ssh.port, Some(2222));
    assert_eq!(ssh.x11, Some(false));
    let algorithms = ssh.algorithms.unwrap();
    assert_eq!(algorithms.kex, Some(vec![]));
    assert_eq!(algorithms.cipher, Some(vec!["cipher".into()]));
    let inherited = Profile { env: BTreeMap::new(), on_exit: OnExit::Inherit, ..raw.clone() };
    let resolved = effective(&inherited, &config);
    assert_eq!(resolved.on_exit, OnExit::Reconnect);
    assert_eq!(resolved.env["override"], "inner");
    assert_eq!(before, config.to_text());
    assert_eq!(config, Config::parse(&before).unwrap());
}

#[test]
fn effective_profiles_choose_their_kind_and_inherit_group_appearance() {
    let config = Config::parse(r#"
[profile-defaults.local]
scheme = "Local"
[profile-defaults.serial]
scheme = "Serial"
[profile-defaults.socket]
scheme = "Socket"
[[groups]]
name = "Machines"
color = "blue"
icon = "console"
"#).unwrap();
    let local = Profile { group: Some("Machines".into()), ..Profile::default() };
    let serial = Profile { serial: Some("COM1".into()), icon: Some("private".into()), ..local.clone() };
    let socket = Profile { socket: Some("tcp://localhost:9000".into()), color: Some("red".into()), ..local.clone() };
    for (p, scheme, icon, color) in [(local, "Local", "console", "blue"), (serial, "Serial", "private", "blue"), (socket, "Socket", "console", "red")] {
        let effective = effective(&p, &config);
        assert_eq!(effective.scheme.as_deref(), Some(scheme));
        assert_eq!(effective.icon.as_deref(), Some(icon));
        assert_eq!(effective.color.as_deref(), Some(color));
    }
}

#[test]
fn ssh_options_round_trip_without_filling_missing_fields() {
    let text = "[[profiles]]\nssh = 'web'\n[profiles.ssh-options]\nidentity-files = ['~/first', '~/second']\nkeepalive-ms = 800\nremember-cwd = true\n[profiles.ssh-options.algorithms]\nhost-key = []\n";
    let config = Config::parse(text).unwrap();
    let profile = &config.profiles[0];
    let options = profile.ssh_options.as_ref().unwrap();
    assert_eq!(options.identity_files, Some(vec!["~/first".into(), "~/second".into()]));
    assert_eq!(options.keepalive_ms, Some(800));
    assert_eq!(options.remember_cwd, Some(true));
    assert_eq!(options.port, None);
    let algorithms = options.algorithms.as_ref().unwrap();
    assert_eq!(algorithms.host_key, Some(Vec::new()));
    assert_eq!(algorithms.kex, None);
    assert_eq!(&Config::parse(&config.to_text()).unwrap().profiles[0], profile);
    assert!(!toml::to_string(&Profile::default()).unwrap().contains("ssh-options"));
}
