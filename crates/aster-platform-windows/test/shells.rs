use super::*;

#[test]
fn detection_always_finds_cmd_with_a_directory_reporting_prompt_and_unique_ids() {
    let all = detect();
    let cmd = all.iter().find(|p| p.id == "local:cmd").expect("cmd");
    assert!(cmd.command.to_lowercase().ends_with("cmd.exe"));
    let prompt = cmd.env.get("PROMPT").unwrap();
    let inherited = [("P".to_owned(), "process value".to_owned())].into();
    assert_eq!(aster_core::environment::expand(prompt, &inherited, true), r"$e]9;9;$P$e\$P$G");
    let mut ids: Vec<&str> = all.iter().map(|p| p.id.as_str()).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), all.len());
    assert!(all.iter().all(|p| p.builtin && !p.name.is_empty()));
}

#[test]
fn commands_are_encoded_as_base64_utf16() {
    assert_eq!(encode_command("dir"), "ZABpAHIA");
    assert_eq!(encode_command("ab"), "YQBiAA==");
}

#[test]
fn visual_studio_detects_versions_outside_the_previous_fixed_list() {
    let root = std::env::temp_dir().join(format!("aster-vs-profile-{}", std::process::id()));
    let version = root.join("18");
    let edition = version.join("Community");
    let common = edition.join("Common7");
    let tools = common.join("Tools");
    let script = tools.join("VsDevCmd.bat");
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::write(&script, "").unwrap();
    let profiles = visual_studio(Path::new(r"C:\Windows\System32"), std::slice::from_ref(&root));
    std::fs::remove_file(&script).unwrap();
    for path in [&tools, &common, &edition, &version, &root] { std::fs::remove_dir(path).unwrap(); }
    let profile = &profiles[0];
    assert_eq!(profile.id, "local:vs18:community");
    assert_eq!(profile.args, ["/k".to_owned(), path_str(&script)]);
}
