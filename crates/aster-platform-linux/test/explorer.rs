use super::*;

#[test]
fn folder_menu_generators_escape_the_executable_and_label() {
    let path = "/opt/Aster ' & %F/aster";
    assert!(script(path).contains(r#"exec '/opt/Aster '\'' & %F/aster' open "$dir""#));
    assert!(nemo(path, "Open\nNow").contains("Name=Open\\nNow\n"));
    assert!(nemo(path, "Open").contains(r"Exec='/opt/Aster '\\'' & %%F/aster' open %F"));
    assert!(service(path, "Open").contains("MimeType=inode/directory;\n"));
    assert!(service(path, "Open").contains(r#"Exec="/opt/Aster ' & %%F/aster" "open" %f"#));
    assert!(thunar::action(path, "<Open & Go>", false).contains("<name>&lt;Open &amp; Go&gt;</name>"));
    assert!(thunar::action(path, "Open", false).contains(r"<command>exec &apos;/opt/Aster &apos;\&apos;&apos; &amp; %%F/aster&apos; open %f</command>"));
}

#[test]
fn file_menu_install_and_remove_preserve_other_entries() {
    let base = std::env::temp_dir().join(format!("aster-file-menu-test-{}", std::process::id()));
    let data = base.join("data");
    let config = base.join("config");
    let executable = base.join("Aster files/aster");
    let other = data.join("nemo/actions/another.nemo_action");
    let uca = config.join("Thunar/uca.xml");
    let initial = "<?xml version=\"1.0\"?>\r\n<actions>\r\n <action><name>Mine</name><unique-id>user</unique-id></action>\r\n</actions>\r\n";
    desktop::write(&other, b"untouched").unwrap();
    desktop::write(&uca, initial.as_bytes()).unwrap();
    for _ in 0..2 {
        set(&data, &config, &executable, "Open in Aster", true).unwrap();
    }
    for entry in [SCRIPT, NEMO, SERVICE] {
        assert!(data.join(entry).is_file());
    }
    assert_eq!(fs::read_to_string(&uca).unwrap().matches("com.brano-san.Aster.open").count(), 1);
    for _ in 0..2 {
        set(&data, &config, &executable, "Open in Aster", false).unwrap();
    }
    for entry in [SCRIPT, NEMO, SERVICE] {
        assert!(!data.join(entry).exists());
    }
    assert_eq!(fs::read(&other).unwrap(), b"untouched");
    assert_eq!(fs::read_to_string(&uca).unwrap(), initial);
    fs::remove_dir_all(base).unwrap();
}
