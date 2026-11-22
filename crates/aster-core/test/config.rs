use super::*;

#[test]
fn an_empty_file_gives_the_defaults() {
    let c = Config::parse("").unwrap();
    assert_eq!(c, Config::default());
    assert_eq!(c.terminal.scrollback_lines, 25_000);
    assert_eq!(c.terminal.right_click, RightClick::Clipboard);
    assert!(c.terminal.copy_on_select);
    assert!(c.terminal.alt_is_meta && c.terminal.bell_attention && c.terminal.open_on_start);
    assert_eq!(c.terminal.link_modifier, LinkModifier::None);
    assert!(!c.terminal.paste_newlines_as_spaces);
}

#[test]
fn defaults_serialize_to_only_the_version_and_round_trip() {
    let config = Config::default();
    assert_eq!(config.to_text(), "version = 1\n");
    assert_eq!(Config::parse(&config.to_text()).unwrap(), config);
}

#[test]
fn cursor_blink_style_preserves_existing_boolean_configs() {
    let old = Config::parse("[terminal]\ncursor-blink = false").unwrap();
    assert!(!old.terminal.cursor_blink);
    assert_eq!(old.terminal.cursor_blink_style, CursorBlinkStyle::Step);
    let smooth = Config::parse("[terminal]\ncursor-blink-style = \"smooth\"").unwrap();
    assert!(smooth.terminal.cursor_blink);
    assert_eq!(smooth.terminal.cursor_blink_style, CursorBlinkStyle::Smooth);
    assert_eq!(Config::parse(&smooth.to_text()).unwrap(), smooth);
}

#[test]
fn only_changed_nested_fields_and_user_tables_are_serialized() {
    let mut config = Config::default();
    config.terminal.search_options.regex = true;
    config.terminal.environment.insert("A".into(), "one".into());
    config.hotkeys.insert("new-tab".into(), Vec::new());
    let text = config.to_text();
    let tree: toml::Value = toml::from_str(&text).unwrap();
    assert_eq!(tree.as_table().unwrap().len(), 3);
    assert_eq!(tree["terminal"].as_table().unwrap().len(), 2);
    assert_eq!(tree["terminal"]["search-options"].as_table().unwrap().len(), 1);
    assert_eq!(Config::parse(&text).unwrap(), config);
    assert_eq!(Config::parse(&Config::defaults_text()).unwrap(), Config::default());
    assert_eq!(Config::parse("[terminal]\nfont-size = 19").unwrap().terminal.font, Config::default().terminal.font);
}

#[test]
fn tray_is_opt_in_and_round_trips() {
    let old = Config::parse("[appearance]\ncycle-tabs = false").unwrap();
    assert!(!old.appearance.tray);
    let enabled = Config::parse("[appearance]\ntray = true").unwrap();
    assert!(Config::parse(&enabled.to_text()).unwrap().appearance.tray);
}

#[test]
fn dock_screen_accepts_named_and_numeric_toml_values() {
    assert_eq!(Config::default().dock.selection(), DockScreen::Current);
    let primary = Config::parse("[dock]\nscreen = \"primary\"").unwrap();
    assert_eq!(primary.dock.selection(), DockScreen::Primary);
    let numbered = Config::parse("[dock]\nscreen = 2").unwrap();
    assert_eq!(numbered.dock.selection(), DockScreen::Number(2));
    assert_eq!(Config::parse(&numbered.to_text()).unwrap(), numbered);
    assert_eq!(Config::parse("[dock]\nscreen = 0").unwrap().dock.selection(), DockScreen::Current);
}

#[test]
fn last_tab_option_migrates_to_terminal_and_the_new_key_wins() {
    let legacy = "[appearance]\nlast-tab-closes-window = false";
    let migrated = Config::parse(legacy).unwrap();
    assert!(!migrated.terminal.close_window_with_last_tab);
    let text = migrated.to_text();
    assert!(text.contains("close-window-with-last-tab = false"));
    assert!(!text.contains("last-tab-closes-window"));
    assert!(Config::parse(&format!("{legacy}\n[terminal]\nclose-window-with-last-tab = true")).unwrap().terminal.close_window_with_last_tab);
}

#[test]
fn pane_options_read_legacy_mouse_focus_and_prefer_the_new_section() {
    let legacy = "[terminal]\nfocus-follows-mouse = true";
    let migrated = Config::parse(legacy).unwrap();
    assert!(migrated.panes.focus_follows_mouse);
    assert_eq!(migrated.panes.resize_step(), 0.1);
    let new = Config::parse(&format!("{legacy}\n[panes]\nfocus-follows-mouse = false\nresize-step = 0.7")).unwrap();
    assert!(!new.panes.focus_follows_mouse);
    assert_eq!(new.panes.resize_step(), 0.7);
    assert_eq!(Config::parse(&new.to_text()).unwrap(), new);
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
    assert_eq!(c.terminal.font, Config::default().terminal.font);
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
fn unversioned_config_migrates_camel_case_paste_settings_and_writes_version_first() {
    let old = "[terminal]\nbracketedPaste = false\ntrimWhitespaceOnPaste = false\npasteNewlinesAsSpaces = true";
    let config = Config::parse(old).unwrap();
    assert!(!config.terminal.bracketed_paste);
    assert!(!config.terminal.trim_whitespace_on_paste);
    assert!(config.terminal.paste_newlines_as_spaces);
    assert!(config.to_text().starts_with("version = 1\n"));
    assert_eq!(Config::parse(&config.to_text()).unwrap(), config);
    let canonical = Config::parse("[terminal]\nbracketedPaste = false\nbracketed-paste = true").unwrap();
    assert!(canonical.terminal.bracketed_paste);
}

#[test]
fn versioned_config_keeps_legacy_window_pane_and_environment_options() {
    let config = Config::parse("version = 1\n[appearance]\nlast-tab-closes-window = false\n[terminal]\nfocus-follows-mouse = true\n[terminal.environment]\nFIRST = 'old'\n").unwrap();
    assert!(!config.terminal.close_window_with_last_tab);
    assert!(config.panes.focus_follows_mouse);
    assert_eq!(config.terminal.environment["FIRST"], "old");

    let config = Config::parse("version = 1\n[terminal.environment]\nFIRST = 'old'\n[terminal.env]\nFIRST = 'new'\n").unwrap();
    assert_eq!(config.terminal.environment["FIRST"], "new");
    assert!(config.to_text().contains("[terminal.env]"));
}

#[test]
fn future_versions_load_read_only_with_supported_settings() {
    let newer = Config::parse("version = 42\nfuture-setting = true\n[terminal]\nfont-size = 19").unwrap();
    assert!(newer.read_only());
    assert_eq!(newer.version, 42);
    assert_eq!(newer.terminal.font_size, 19.0);
    let partial = Config::parse("version = 42\n[terminal]\ncursor = 'future-cursor'\nfont-size = 21\n[vault]\nsealed = 'opaque'").unwrap();
    assert!(partial.read_only());
    assert_eq!(partial.terminal.font_size, 21.0);
    assert_eq!(partial.vault.sealed, "opaque");
    assert!(Config::parse("version = -1").is_err());
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

#[test]
fn old_configs_and_partial_vault_sections_keep_vault_defaults() {
    let old = Config::parse("[ssh]\nremember-passwords = false").unwrap();
    assert!(old.vault.sealed.is_empty());
    assert_eq!(old.vault.remember_minutes, 15);
    let partial = Config::parse("[vault]\nremember-minutes = 0").unwrap();
    assert!(partial.vault.sealed.is_empty());
    assert_eq!(partial.vault.remember_minutes, 0);
}

#[test]
fn sealed_vault_and_unlock_duration_round_trip() {
    let sealed = "aster-vault:1:Ab+/9Q==";
    let mut config = Config::parse(&format!("[vault]\nsealed = \"{sealed}\"")).unwrap();
    assert_eq!(config.vault.remember_minutes, 15);
    assert_eq!(config.vault.sealed, sealed);
    config.vault.remember_minutes = 1440;
    let text = config.to_text();
    assert!(text.contains("remember-minutes = 1440"));
    assert_eq!(Config::parse(&text).unwrap(), config);
}

#[test]
fn serial_defaults_read_integer_and_fractional_stop_bits() {
    for (value, spec) in [("1", ",??1"), ("1.5", ",??1.5"), ("2", ",??2")] {
        let config = Config::parse(&format!("[profile-defaults.serial.serial]\nstop-bits = {value}\n")).unwrap();
        let serial = config.profile_defaults["serial"].serial.as_ref().unwrap();
        assert_eq!(serial.spec(), spec);
        assert_eq!(Config::parse(&config.to_text()).unwrap(), config);
    }
}
