use super::*;

#[test]
fn placement_round_trips_and_rejects_garbage() {
    let p = Placement { x: -8, y: 20, width: 1200, height: 700, maximized: true };
    assert_eq!(parse_placement(&format_placement(&p)), Some(p));
    assert_eq!(parse_placement("1 2 3"), None);
    assert_eq!(parse_placement(""), None);
}

#[test]
fn hidden_start_requires_both_the_request_and_a_configured_activation_mode() {
    let mut config = Config::default();
    assert!(!starts_hidden(&config, &cli::Command::Hidden));
    config.appearance.tray = true;
    assert!(starts_hidden(&config, &cli::Command::Hidden));
    assert!(!starts_hidden(&config, &cli::Command::Window));
    config.appearance.tray = false;
    config.appearance.dock = aster_core::config::Dock::Top;
    assert!(starts_hidden(&config, &cli::Command::Hidden));
}

#[test]
fn portable_mode_requires_a_data_directory_beside_the_executable() {
    let root = std::env::temp_dir().join(format!("aster-portable-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let exe = root.join("aster");
    let data = root.join("data");
    let fallback = || Some(root.join("system"));
    assert_eq!(choose_config_dir(Some(&exe), fallback), fallback());
    std::fs::write(&data, []).unwrap();
    assert_eq!(choose_config_dir(Some(&exe), fallback), fallback());
    std::fs::remove_file(&data).unwrap();
    std::fs::create_dir(&data).unwrap();
    assert_eq!(choose_config_dir(Some(&exe), || None), Some(data.clone()));
    assert_eq!(choose_config_dir(None, fallback), fallback());
    std::fs::remove_dir(data).unwrap();
    std::fs::remove_dir(root).unwrap();
}
