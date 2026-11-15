use super::*;

#[test]
fn detect_shell_from_command_name() {
    for command in [
        r"C:\Program Files\PowerShell\7\pwsh.exe",
        "powershell.exe",
        "/tools/PWSH.EXE",
    ] {
        assert_eq!(shell_kind(command), ShellKind::PowerShell);
    }
    assert_eq!(shell_kind("CMD.EXE"), ShellKind::Cmd);
    assert_eq!(shell_kind("/bin/bash"), ShellKind::Unix);
    assert_eq!(shell_kind("wsl.exe"), ShellKind::Unix);
    assert_eq!(shell_kind("cmd.exe.backup"), ShellKind::Unix);
    assert_eq!(shell_kind(""), ShellKind::Unix);
}

#[test]
fn unix_simple_path() {
    assert_eq!(
        quote_path("/tmp/a_@%+=:,.z-19", ShellKind::Unix),
        "/tmp/a_@%+=:,.z-19"
    );
}

#[test]
fn unix_space() {
    assert_eq!(quote_path("/tmp/a b", ShellKind::Unix), "'/tmp/a b'");
}

#[test]
fn unix_apostrophe() {
    assert_eq!(quote_path("a'b", ShellKind::Unix), r"'a'\''b'");
}

#[test]
fn powershell_simple_path() {
    assert_eq!(
        quote_path(r"C:\tmp\file", ShellKind::PowerShell),
        r"C:\tmp\file"
    );
}

#[test]
fn powershell_space() {
    assert_eq!(quote_path(r"C:\a b", ShellKind::PowerShell), r"'C:\a b'");
}

#[test]
fn powershell_apostrophe() {
    assert_eq!(quote_path("a'b", ShellKind::PowerShell), "'a''b'");
}

#[test]
fn cmd_simple_path() {
    assert_eq!(quote_path(r"C:\tmp\file", ShellKind::Cmd), r"C:\tmp\file");
}

#[test]
fn cmd_space() {
    assert_eq!(quote_path(r"C:\a b", ShellKind::Cmd), r#""C:\a b""#);
}

#[test]
fn cmd_apostrophe() {
    assert_eq!(quote_path("a'b", ShellKind::Cmd), r#""a'b""#);
}

#[test]
fn empty_paths_are_quoted() {
    assert_eq!(quote_path("", ShellKind::Unix), "''");
    assert_eq!(quote_path("", ShellKind::PowerShell), "''");
    assert_eq!(quote_path("", ShellKind::Cmd), r#""""#);
    assert_eq!(quote_paths(&[], ShellKind::Unix), "");
}

#[test]
fn join_paths_with_single_space() {
    let paths = vec!["a".to_owned(), "b c".to_owned()];
    assert_eq!(quote_paths(&paths, ShellKind::Unix), "a 'b c'");
    assert_eq!(quote_paths(&paths, ShellKind::PowerShell), "a 'b c'");
    assert_eq!(quote_paths(&paths, ShellKind::Cmd), r#"a "b c""#);
}

#[test]
fn special_characters_trigger_quoting() {
    for character in r#" '"`$&(){};@#,"#.chars() {
        let path = format!("a{character}b");
        assert!(quote_path(&path, ShellKind::PowerShell).starts_with('\''));
    }
    for character in " &()[]{}^=;!'+,`~".chars() {
        let path = format!("a{character}b");
        assert!(quote_path(&path, ShellKind::Cmd).starts_with('"'));
    }
    assert_eq!(quote_path("путь", ShellKind::Unix), "'путь'");
}
