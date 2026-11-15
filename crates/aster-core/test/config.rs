use super::*;

#[test]
fn an_empty_file_gives_the_defaults() {
    let c = Config::parse("").unwrap();
    assert_eq!(c, Config::default());
    assert_eq!(c.terminal.scrollback_lines, 25_000);
    assert_eq!(c.terminal.right_click, RightClick::Clipboard);
    assert!(c.terminal.copy_on_select);
}

#[test]
fn partial_sections_keep_the_other_defaults() {
    let c = Config::parse(
        r#"
[terminal]
font-size = 16
cursor = "beam"

[hotkeys]
"split-right" = ["Ctrl-Shift-S", "Ctrl-K Right"]

[[profiles]]
id = "custom:htop"
name = "htop"
command = "wsl.exe"
args = ["htop"]
"#,
    )
    .unwrap();
    assert_eq!(c.terminal.font_size, 16.0);
    assert_eq!(c.terminal.cursor, CursorStyle::Beam);
    assert_eq!(c.terminal.font, "Cascadia Mono");
    assert_eq!(c.hotkeys["split-right"].len(), 2);
    assert_eq!(c.profiles[0].args, ["htop"]);
    assert!(!c.profiles[0].builtin);
}

#[test]
fn round_trips_through_text() {
    let mut c = Config::default();
    c.terminal.profile = "local:cmd".into();
    c.appearance.color_scheme_mode = SchemeMode::Auto;
    assert_eq!(Config::parse(&c.to_text()).unwrap(), c);
}

#[test]
fn bad_values_are_reported() {
    assert!(Config::parse("[terminal]\ncursor = \"triangle\"").is_err());
}

#[test]
fn the_language_defaults_to_automatic_and_round_trips() {
    assert_eq!(Config::default().appearance.language, "auto");
    let config = Config::parse("[appearance]\nlanguage = \"ru\"\n").unwrap();
    assert_eq!(config.appearance.language, "ru");
    assert_eq!(Config::parse(&config.to_text()).unwrap().appearance.language, "ru");
}
