use super::*;
use std::path::PathBuf;

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

fn parsed(list: &[&str]) -> Parsed {
    parse(&args(list), Some(&PathBuf::from("/home/me")))
}

#[test]
fn no_arguments_open_a_window_and_flags_print_information() {
    assert_eq!(parsed(&[]), Parsed::Request(Command::Window));
    assert_eq!(parsed(&["--version"]), Parsed::Version);
    assert_eq!(parsed(&["-h"]), Parsed::Help);
    assert!(matches!(parsed(&["--bogus"]), Parsed::Error(_)));
}

#[test]
fn folders_are_made_absolute_against_the_callers_folder() {
    let base = PathBuf::from("/home/me");
    assert_eq!(parsed(&["open"]), Parsed::Request(Command::Open { dir: base.join(".").to_string_lossy().into_owned() }));
    assert_eq!(parsed(&["src"]), Parsed::Request(Command::Open { dir: base.join("src").to_string_lossy().into_owned() }));
    let root = if cfg!(windows) { r"C:\work" } else { "/srv" };
    assert_eq!(parsed(&["open", root]), Parsed::Request(Command::Open { dir: root.into() }));
    assert!(matches!(parsed(&["open", "a", "b"]), Parsed::Error(_)));
    assert!(matches!(parsed(&["a", "b"]), Parsed::Error(_)));
}

#[test]
fn run_profile_and_paste_keep_their_words() {
    assert_eq!(parsed(&["run", "--", "make", "-j", "8"]), Parsed::Request(Command::Run { command: args(&["make", "-j", "8"]), dir: None }));
    assert_eq!(parsed(&["run", "tool"]), Parsed::Request(Command::Run { command: args(&["tool"]), dir: None }));
    assert!(matches!(parsed(&["run"]), Parsed::Error(_)));
    assert_eq!(parsed(&["profile", "Git", "Bash"]), Parsed::Request(Command::Profile { name: "Git Bash".into() }));
    assert_eq!(parsed(&["paste", "echo", "hi"]), Parsed::Request(Command::Paste { text: "echo hi".into() }));
}

#[test]
fn exec_forms_preserve_empty_and_option_like_arguments() {
    let command = args(&["tool", "two words", "", "--", "--dir", "literal", "a\"b"]);
    for flag in ["-e", "--"] {
        let mut invocation = vec![flag.to_owned()];
        invocation.extend(command.clone());
        let Parsed::Request(Command::Run { command: actual, .. }) = parse(&invocation, None) else { panic!("expected a run request") };
        assert_eq!(actual, command);
        assert!(matches!(parsed(&[flag]), Parsed::Error(_)));
    }
}

#[test]
fn exec_working_directory_is_resolved_before_forwarding() {
    let cli = ["--dir", "sub folder", "-e", "tool", "--dir", "literal"];
    let expected = PathBuf::from("/home/me").join("sub folder").to_string_lossy().into_owned();
    let Parsed::Request(command) = parsed(&cli) else { panic!("expected a run request") };
    let request = Request { cwd: Some("/home/me".into()), command };
    let request = Request::decode(&request.encode()).unwrap();
    let Command::Run { command, dir } = request.command else { panic!("expected a run request") };
    assert_eq!(dir.as_deref(), Some(expected.as_str()));
    assert_eq!(command, args(&["tool", "--dir", "literal"]));
    assert_eq!(parsed(&["--working-directory=sub folder", "--", "tool"]), Parsed::Request(Command::Run { command: args(&["tool"]), dir: Some(expected.clone()) }));
    assert_eq!(parsed(&["--working-directory", "sub folder", "-e", "tool"]), Parsed::Request(Command::Run { command: args(&["tool"]), dir: Some(expected) }));
    assert!(matches!(parsed(&["--dir"]), Parsed::Error(_)));
    assert!(matches!(parsed(&["--dir=", "-e", "tool"]), Parsed::Error(_)));
    assert!(matches!(parsed(&["--working-directory", "src"]), Parsed::Error(_)));
}

#[test]
fn hidden_is_a_startup_request_and_an_exec_argument_after_the_delimiter() {
    assert_eq!(parsed(&["--hidden"]), Parsed::Request(Command::Hidden));
    assert!(matches!(parsed(&["--hidden", "open"]), Parsed::Error(_)));
    assert_eq!(parsed(&["-e", "tool", "--hidden"]), Parsed::Request(Command::Run { command: args(&["tool", "--hidden"]), dir: None }));
}

#[test]
fn requests_survive_the_trip_between_instances() {
    let request = Request { cwd: Some("/tmp".into()), command: Command::Run { command: args(&["ls", "-la"]), dir: None } };
    assert_eq!(Request::decode(r#"{"cwd":"/tmp","command":{"kind":"run","command":["ls","-la"]}}"#), Some(request.clone()));
    assert_eq!(Request::decode(&request.encode()), Some(request));
    assert_eq!(Request::decode("garbage"), None);
}
