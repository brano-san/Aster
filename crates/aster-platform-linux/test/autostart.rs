use super::*;

#[test]
fn autostart_uses_an_independent_desktop_entry() {
    let text = entry("/opt/Aster $test/aster", &["--hidden"]);
    assert!(text.starts_with("[Desktop Entry]\nType=Application\nName=Aster\n"));
    assert!(text.contains(r#"Exec="/opt/Aster \\$test/aster" "--hidden""#));
    assert!(text.contains("Terminal=false\n"));
    assert!(!text.contains("Actions="));
    assert!(!entry("/opt/aster", &[]).contains("--hidden"));
}

#[test]
fn autostart_updates_the_binary_path_and_removes_its_own_file() {
    let base = std::env::temp_dir().join(format!("aster-autostart-test-{}", std::process::id()));
    let executable = base.join("Old/aster");
    let path = base.join(ENTRY);
    set(&base, Some((&executable, &["--hidden"]))).unwrap();
    assert!(fs::read_to_string(&path).unwrap().contains("--hidden"));
    let executable = base.join("New/aster");
    set(&base, Some((&executable, &[]))).unwrap();
    let current = fs::read_to_string(&path).unwrap();
    assert!(current.contains("New"));
    assert!(!current.contains("--hidden"));
    set(&base, None).unwrap();
    assert!(!path.exists());
    fs::write(&path, "custom entry").unwrap();
    set(&base, None).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "custom entry");
    fs::remove_dir_all(base).unwrap();
}
