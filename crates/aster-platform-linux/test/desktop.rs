use super::*;

#[test]
fn desktop_exec_escapes_each_argument() {
    assert_eq!(exec("/opt/Aster $test/aster", &["profile", "a\"b", "a\\b", "a`b", "%f;\n"]), r#""/opt/Aster \\$test/aster" "profile" "a\\"b" "a\\\\b" "a\\`b" "%%f;\n""#);
}

#[test]
fn desktop_actions_use_unique_ids_and_exact_profile_names() {
    let text = launcher("/opt/Aster/aster", &["Work; dev".into(), "Name\n[Entry]".into()]);
    assert!(text.contains("StartupWMClass=aster\n"));
    assert!(text.contains("X-TerminalArgExec=-e\nX-TerminalArgDir=--working-directory\n"));
    assert!(text.contains("Actions=new-window;profile-0;profile-1;\n"));
    assert!(text.contains("[Desktop Action new-window]\nName=New window\n"));
    assert!(text.contains("[Desktop Action profile-0]\nName=Work; dev\nExec=\"/opt/Aster/aster\" \"profile\" \"Work; dev\"\n"));
    assert!(text.contains(r"Name=Name\n[Entry]"));
    assert!(launcher("/opt/Aster/aster", &[]).contains("Actions=new-window;\n"));
}

#[test]
fn launcher_install_refresh_and_remove_use_only_the_supplied_data_directory() {
    let temp = std::env::temp_dir().join(format!("aster-launcher-test-{}", std::process::id()));
    fs::create_dir(&temp).unwrap();
    let path = temp.join("aster");
    let entry = temp.join(LAUNCHER);
    let icon = temp.join(ICON_PATH);
    assert!(!update_launcher(&temp, &path, &[], None).unwrap());
    assert!(!entry.exists());
    assert!(update_launcher(&temp, &path, &["Old".into()], Some(true)).unwrap());
    assert_eq!(fs::read(&icon).unwrap(), ICON);
    assert!(fs::read_to_string(&entry).unwrap().contains("Name=Old"));
    assert!(update_launcher(&temp, &path, &["New".into()], None).unwrap());
    assert!(fs::read_to_string(&entry).unwrap().contains("Name=New"));
    assert!(!update_launcher(&temp, &path, &[], Some(false)).unwrap());
    assert!(!entry.exists());
    assert!(!icon.exists());
    fs::write(&entry, "custom launcher").unwrap();
    assert!(!update_launcher(&temp, &path, &[], None).unwrap());
    assert!(update_launcher(&temp, &path, &[], Some(true)).is_err());
    assert!(!update_launcher(&temp, &path, &[], Some(false)).unwrap());
    assert_eq!(fs::read_to_string(&entry).unwrap(), "custom launcher");
    fs::remove_file(&entry).unwrap();
}
