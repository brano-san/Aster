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
    let icon_16 = temp.join("icons/hicolor/16x16/apps/aster.png");
    let scalable = temp.join("icons/hicolor/scalable/apps/aster.svg");
    let other_icon = temp.join("icons/hicolor/16x16/apps/another-app.png");
    fs::create_dir_all(other_icon.parent().unwrap()).unwrap();
    fs::write(&other_icon, "another icon").unwrap();
    assert!(!update_launcher(&temp, &path, &[], None).unwrap());
    assert!(!entry.exists());
    assert!(update_launcher(&temp, &path, &["Old".into()], Some(true)).unwrap());
    assert_eq!(fs::read(&icon).unwrap(), ICON);
    assert!(fs::read(&icon_16).unwrap().starts_with(b"\x89PNG\r\n\x1a\n"));
    assert!(fs::read_to_string(&scalable).unwrap().contains("<svg"));
    assert!(fs::read_to_string(&entry).unwrap().contains("Name=Old"));
    fs::write(&icon, "old Aster icon").unwrap();
    fs::remove_file(&icon_16).unwrap();
    assert!(update_launcher(&temp, &path, &["New".into()], None).unwrap());
    assert_eq!(fs::read(&icon).unwrap(), ICON);
    assert!(icon_16.is_file());
    assert!(fs::read_to_string(&entry).unwrap().contains("Name=New"));
    assert!(!update_launcher(&temp, &path, &[], Some(false)).unwrap());
    assert!(!entry.exists());
    assert!(!icon.exists());
    assert!(!icon_16.exists());
    assert!(!scalable.exists());
    assert_eq!(fs::read_to_string(&other_icon).unwrap(), "another icon");
    fs::write(&entry, "custom launcher").unwrap();
    fs::write(&icon, "custom logo").unwrap();
    assert!(!update_launcher(&temp, &path, &[], None).unwrap());
    assert!(update_launcher(&temp, &path, &[], Some(true)).is_err());
    assert!(!update_launcher(&temp, &path, &[], Some(false)).unwrap());
    assert_eq!(fs::read_to_string(&entry).unwrap(), "custom launcher");
    assert_eq!(fs::read_to_string(&icon).unwrap(), "custom logo");
    fs::remove_dir_all(&temp).unwrap();
}
