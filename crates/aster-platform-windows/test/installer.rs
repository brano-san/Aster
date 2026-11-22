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
fn deferred_removal_handles_windows_command_metacharacters_in_paths() {
    let scratch = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(".tmp");
    fs::create_dir_all(&scratch).unwrap();
    let dir = scratch.canonicalize().unwrap().join(format!("Aster & %TEMP% !{}", std::process::id()));
    let install = dir.join("Programs").join("Aster");
    let settings = dir.join("Roaming").join("Aster");
    fs::create_dir_all(&install).unwrap();
    fs::create_dir_all(&settings).unwrap();
    fs::write(install.join("aster.exe"), "installed").unwrap();
    fs::write(settings.join("config.toml"), "settings").unwrap();

    assert!(delete_after_exit(&install, None).unwrap().wait().unwrap().success());
    assert!(!install.exists());
    assert!(settings.join("config.toml").exists());
    fs::create_dir_all(&install).unwrap();

    assert!(delete_after_exit(&install, Some(&settings)).unwrap().wait().unwrap().success());
    assert!(!install.exists());
    assert!(!settings.exists());
    fs::remove_dir(dir.join("Programs")).unwrap();
    fs::remove_dir(dir.join("Roaming")).unwrap();
    fs::remove_dir(dir).unwrap();
}
