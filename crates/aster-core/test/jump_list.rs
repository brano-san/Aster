use super::*;

#[test]
fn profiles_are_ranked_by_recency_and_include_only_visible_unique_items() {
    let config = Config {
        profiles: (0..12).map(|n| Profile { id: format!("own:{n}"), name: format!("Profile {n}"), hidden: n == 0, ..Profile::default() }).collect(),
        terminal: crate::config::Terminal { recent_profiles: 2, ..Default::default() },
        hidden_profiles: vec!["hidden".into()],
        ..Default::default()
    };
    let detected = [Profile::new("builtin", "Recent shell", "cmd", &[]), Profile::new("hidden", "Hidden", "cmd", &[])];
    let recent = ["missing", "hidden", "builtin", "builtin", "own:2", "own:3"].map(str::to_owned);
    let list = build(&config, &detected, &recent, &[]);
    let items = &list.categories[0].items;
    assert_eq!(items.len(), 10);
    assert_eq!(items[0].args, ["profile", "builtin"]);
    assert_eq!(items[1].args, ["profile", "own:2"]);
    assert_eq!(items.iter().filter(|item| item.args[1] == "own:2").count(), 1);
    assert!(!items.iter().any(|item| item.args[1] == "own:0"));
    assert_eq!(list.task.args, ["--new-window"]);
}

#[test]
fn folders_keep_windows_paths_in_recent_order_and_drop_duplicates() {
    let mut folders = vec![];
    for dir in [r"C:\Work Notes", "/mnt/c/work", "relative", r"D:\project", "c:/Work Notes/"] {
        remember_folder(&mut folders, dir);
    }
    assert!(!remember_folder(&mut folders, r"C:\Work Notes"));
    let list = build(&Config::default(), &[], &[], &folders);
    let items = &list.categories[1].items;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].args, ["open", "c:/Work Notes/"]);
    assert_eq!(items[1].args, ["open", r"D:\project"]);
}
