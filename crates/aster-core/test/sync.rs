use super::*;

const LOCAL: &str = r#"version = 1
[appearance]
opacity = 0.8
background-image = 'C:\local.png'
[window]
x = 25
width = 900
tab-index = true
[terminal]
scrollback-lines = 500
[ssh]
agent-pipe = 'machine pipe'
[hotkeys]
copy = ["Ctrl-C"]
[sync]
backend = "folder"
folder = "private folder"
[updates]
check-automatically = false
portable-path = "private path"
"#;

#[test]
fn extract_includes_only_selected_shareable_settings() {
    let text = extract_at(LOCAL, &[Part::Appearance, Part::Shortcuts], "desk", 101).unwrap();
    let table = text.parse::<toml::Table>().unwrap();
    assert_eq!(metadata(&text).unwrap(), Metadata { machine: "desk".into(), saved: 101, schema: 1 });
    assert_eq!(table["appearance"]["opacity"].as_float(), Some(0.8));
    assert!(table["appearance"].get("background-image").is_none());
    assert_eq!(table["window"]["tab-index"].as_bool(), Some(true));
    assert!(table["window"].get("width").is_none());
    assert!(table.contains_key("hotkeys"));
    assert!(!table.contains_key("terminal"));
    assert!(!table.contains_key("updates"));
    assert!(!text.contains("private"));
}

#[test]
fn merge_keeps_local_only_settings_and_resets_omitted_shared_values() {
    let remote = extract("[appearance]\nopacity = 0.6\n[terminal]\n", &[Part::Appearance, Part::Terminal]);
    let merged = merge(LOCAL, &remote, &[Part::Appearance, Part::Terminal, Part::Shortcuts]).unwrap();
    let table = merged.parse::<toml::Table>().unwrap();
    assert!(merged.starts_with("version = 1\n\n"));
    assert_eq!(table["appearance"]["opacity"].as_float(), Some(0.6));
    assert_eq!(table["appearance"]["background-image"].as_str(), Some(r"C:\local.png"));
    assert_eq!(table["window"]["width"].as_integer(), Some(900));
    assert!(table["window"].get("tab-index").is_none());
    assert_eq!(table["ssh"]["agent-pipe"].as_str(), Some("machine pipe"));
    assert!(!table.contains_key("terminal"));
    assert!(table.contains_key("hotkeys"));
    assert_eq!(table["sync"]["folder"].as_str(), Some("private folder"));
    assert_eq!(table["updates"]["check-automatically"].as_bool(), Some(false));
}

#[test]
fn vault_contains_only_the_encrypted_seal() {
    let sealed = format!("aster-vault:1:{}", "A".repeat(80));
    let source = format!("[vault]\nsealed = '{sealed}'\nmaster-password = 'secret'\nunlocked-key = 'key'\nremember-minutes = 17\n");
    let exported = extract(&source, &[Part::Vault]);
    let table = exported.parse::<toml::Table>().unwrap();
    assert_eq!(table["vault"].as_table().unwrap().len(), 1);
    assert_eq!(table["vault"]["sealed"].as_str(), Some(sealed.as_str()));
    let merged = merge("[vault]\nremember-minutes = 11\n", &exported, &[Part::Vault]).unwrap();
    let table = merged.parse::<toml::Table>().unwrap();
    assert_eq!(table["vault"]["remember-minutes"].as_integer(), Some(11));
    assert_eq!(table["vault"]["sealed"].as_str(), Some(sealed.as_str()));
    assert!(extract("[vault]\nsealed = 'plain password'\n", &[Part::Vault]).is_empty());
}

#[test]
fn merge_refuses_future_schemas() {
    assert!(merge(LOCAL, "[sync]\nmachine = 'desk'\nsaved = 100\nschema = 999\n", &[Part::Appearance]).is_err());
}

#[test]
fn direction_compares_each_side_to_the_last_success() {
    assert_eq!(decide(10, 10, 10), Direction::Nothing);
    assert_eq!(decide(12, 10, 10), Direction::Upload);
    assert_eq!(decide(10, 12, 10), Direction::Download);
    assert_eq!(decide(11, 12, 10), Direction::Conflict);
}

#[test]
fn conflict_names_use_the_utc_date() {
    assert_eq!(conflict_name("Desk", 0), "Desk (conflict 1970-01-01)");
    assert_eq!(conflict_name("Desk", 1_709_251_199), "Desk (conflict 2024-02-29)");
}

#[test]
fn rebasing_a_profile_edit_preserves_other_downloaded_profiles() {
    use crate::config::Config;
    let base = Config::parse("[[profiles]]\nid = 'one'\nname = 'First'\n[[profiles]]\nid = 'two'\nname = 'Second'\n").unwrap();
    let mut local = base.clone();
    local.profiles[1].name = "Local edit".into();
    let mut disk = base.clone();
    disk.profiles[0].name = "Remote edit".into();
    disk.profiles.swap(0, 1);
    let merged = rebase_settings(&base, &local, &disk).unwrap();
    let profiles: Vec<_> = merged.profiles.iter().map(|p| (p.id.as_str(), p.name.as_str())).collect();
    assert_eq!(profiles, [("two", "Local edit"), ("one", "Remote edit")]);
}
