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
    assert_eq!(parsed(&["run", "--", "make", "-j", "8"]), Parsed::Request(Command::Run { command: args(&["make", "-j", "8"]) }));
    assert_eq!(parsed(&["run", "htop"]), Parsed::Request(Command::Run { command: args(&["htop"]) }));
    assert!(matches!(parsed(&["run"]), Parsed::Error(_)));
    assert_eq!(parsed(&["profile", "Git", "Bash"]), Parsed::Request(Command::Profile { name: "Git Bash".into() }));
    assert_eq!(parsed(&["paste", "echo", "hi"]), Parsed::Request(Command::Paste { text: "echo hi".into() }));
}

#[test]
fn requests_survive_the_trip_between_instances() {
    let request = Request { cwd: Some("/tmp".into()), command: Command::Run { command: args(&["ls", "-la"]) } };
    assert_eq!(Request::decode(&request.encode()), Some(request));
    assert_eq!(Request::decode("garbage"), None);
}
