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
