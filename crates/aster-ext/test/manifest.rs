use super::*;

const MANIFEST: &str = r#"
id = "hello"
name = "Hello"
version = "1.0.0"
author = "Someone"
description = "Adds a greeting command"
command = "bin/hello.exe"
args = ["--stdio"]
[[commands]]
id = "hello.greet"
title = "Greet"
shortcut = "Ctrl+Alt+G"
menu = "terminal"
[[schemes]]
file = "schemes/night.toml"
[profiles]
provider = true
"#;

#[test]
fn parses_process_and_contributions() {
    let manifest = Manifest::parse(MANIFEST).unwrap();
    assert_eq!(manifest.command.as_deref(), Some("bin/hello.exe"));
    assert_eq!(manifest.args, ["--stdio"]);
    assert_eq!(manifest.commands[0].menu, Some(Menu::Terminal));
    assert_eq!(manifest.schemes[0].file, "schemes/night.toml");
    assert!(manifest.profiles.provider);
}

#[test]
fn accepts_a_scheme_only_extension() {
    let manifest = Manifest::parse("id='colors.dark'\nname='Colors'\nversion='1'\n[[schemes]]\nfile='night.toml'").unwrap();
    assert_eq!(manifest.command, None);
    assert_eq!(manifest.schemes.len(), 1);
}

#[test]
fn fixture_contributes_a_color_scheme_without_a_process() {
    let manifest = Manifest::parse(include_str!("fixtures/hello/extension.toml")).unwrap();
    assert_eq!(manifest.id, "hello");
    assert!(manifest.command.is_none());
    assert_eq!(manifest.schemes[0].file, "schemes/night.toml");
}

#[test]
fn refuses_invalid_ids_labels_and_actions() {
    for (original, invalid) in [
        ("id = \"hello\"", "id = \"../hello\""),
        ("id = \"hello\"", "id = \"Hello\""),
        ("name = \"Hello\"", "name = \" \""),
        ("version = \"1.0.0\"", "version = \"\""),
        ("hello.greet", "unrelated.greet"),
        ("hello.greet", "hello."),
        ("menu = \"terminal\"", "menu = \"window\""),
    ] {
        assert!(Manifest::parse(&MANIFEST.replace(original, invalid)).is_err(), "{invalid}");
    }
    let duplicate = format!("{MANIFEST}\n[[commands]]\nid='hello.greet'\ntitle='Other'\n");
    assert!(Manifest::parse(&duplicate).is_err());
}

#[test]
fn refuses_paths_outside_the_extension() {
    for path in ["../night.toml", "schemes/../../night.toml", "/night.toml", "C:/night.toml", "schemes/name:stream", "schemes/NUL.toml"] {
        let invalid = MANIFEST.replace("schemes/night.toml", path);
        assert!(Manifest::parse(&invalid).is_err(), "{path}");
    }
}
