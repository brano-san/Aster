use super::*;

#[test]
fn accepts_both_setup_filename_forms_case_insensitively() {
    for name in ["Aster-Setup.exe", "ASTER-SETUP-0.10.0.exe", "aster-0.10.0-x86_64-windows-setup.EXE"] {
        assert_eq!(parse(&[], Some(Path::new(name))), Some(Ok(Action::Install { silent: false })));
    }
    assert_eq!(parse(&[], Some(Path::new("aster.exe"))), None);
}

#[test]
fn parses_only_supported_installer_flags() {
    let parse = |args: &[&str]| super::parse(&args.iter().map(ToString::to_string).collect::<Vec<_>>(), None);
    assert_eq!(parse(&["--install", "--silent"]), Some(Ok(Action::Install { silent: true })));
    assert_eq!(parse(&["--uninstall"]), Some(Ok(Action::Uninstall { silent: false })));
    assert_eq!(parse(&["--install", "--unknown"]), Some(Err(())));
    assert_eq!(parse(&["run", "--install"]), None);
}

#[test]
fn derives_the_user_install_paths() {
    let paths = InstallPaths::new(Path::new(r"C:\Some User\Local"));
    assert_eq!(paths.dir, Path::new(r"C:\Some User\Local\Programs\Aster"));
    assert_eq!(paths.exe, Path::new(r"C:\Some User\Local\Programs\Aster\aster.exe"));
}

#[test]
fn accepts_a_custom_directory_but_rejects_roots_and_relative_paths() {
    let paths = InstallPaths::from_dir(PathBuf::from(r"D:\My Apps\Aster")).unwrap();
    assert_eq!(paths.exe, Path::new(r"D:\My Apps\Aster\aster.exe"));
    for path in [r"D:\", r"D:\Apps\..", r"D:Apps", r"Apps\Aster", r"\\server\share\Aster", r"D:\Apps\Aster:test"] {
        assert!(InstallPaths::from_dir(PathBuf::from(path)).is_err(), "{path}");
    }
}

#[test]
fn plans_uninstall_registration_and_rounds_size_up() {
    let paths = InstallPaths::new(Path::new(r"C:\Some User\Local"));
    let values = uninstall_values(&paths, "1.2.3", 1025);
    assert_eq!(values.len(), 11);
    assert_eq!(values[0], ("DisplayName", Value::text("Aster")));
    assert_eq!(values[1], ("Publisher", Value::text("Aster")));
    assert_eq!(values[2], ("DisplayIcon", Value::text(paths.exe.with_extension("exe,0"))));
    assert_eq!(values[3], ("InstallLocation", Value::text(&paths.dir)));
    assert_eq!(values[4], ("URLInfoAbout", Value::text(ABOUT)));
    assert_eq!(values[5], ("UninstallString", Value::text(r#""C:\Some User\Local\Programs\Aster\aster.exe" --uninstall"#)));
    assert_eq!(values[6], ("QuietUninstallString", Value::text(r#""C:\Some User\Local\Programs\Aster\aster.exe" --uninstall --silent"#)));
    assert_eq!(values[7], ("NoModify", Value::Dword(1)));
    assert_eq!(values[8], ("NoRepair", Value::Dword(1)));
    assert_eq!(values[9], ("DisplayVersion", Value::text("1.2.3")));
    assert_eq!(values[10], ("EstimatedSize", Value::Dword(2)));
}

#[test]
fn deferred_removal_preserves_other_files_in_a_custom_directory() {
    let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(".tmp");
    fs::create_dir_all(&scratch).unwrap();
    let dir = scratch.canonicalize().unwrap().join(format!("Aster & %TEMP% !{}", std::process::id()));
    let install = dir.join("Programs").join("Aster");
    let settings = dir.join("Roaming").join("Aster");
    fs::create_dir_all(&install).unwrap();
    fs::create_dir_all(&settings).unwrap();
    fs::write(install.join("aster.exe"), "installed").unwrap();
    fs::write(install.join("notes.txt"), "user data").unwrap();
    fs::write(settings.join("config.toml"), "settings").unwrap();

    assert!(delete_after_exit(&install.join("aster.exe")).unwrap().wait().unwrap().success());
    assert!(!install.join("aster.exe").exists());
    assert!(install.join("notes.txt").exists());
    assert!(settings.join("config.toml").exists());

    fs::remove_file(install.join("notes.txt")).unwrap();
    assert!(delete_after_exit(&install.join("aster.exe")).unwrap().wait().unwrap().success());
    assert!(!install.exists());
    fs::remove_file(settings.join("config.toml")).unwrap();
    fs::remove_dir(settings).unwrap();
    fs::remove_dir(dir.join("Programs")).unwrap();
    fs::remove_dir(dir.join("Roaming")).unwrap();
    fs::remove_dir(dir).unwrap();
}
