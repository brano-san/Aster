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
