use super::*;
use crate::config::Config;

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
