use super::*;

#[test]
fn detection_always_finds_cmd_with_a_directory_reporting_prompt_and_unique_ids() {
    let all = detect();
    let cmd = all.iter().find(|p| p.id == "local:cmd").expect("cmd");
    assert!(cmd.command.to_lowercase().ends_with("cmd.exe"));
    assert_eq!(cmd.env.get("PROMPT").map(String::as_str), Some(r"$e]9;9;$P$e\$P$G"));
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
