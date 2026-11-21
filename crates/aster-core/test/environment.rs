use super::*;

fn values(pairs: &[(&str, &str)]) -> Variables {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
}

#[test]
fn expands_variables_and_leaves_unknown_tokens_and_escapes_literal() {
    let env = values(&[("Home", "value"), ("OTHER", "!")]);
    assert_eq!(expand("%HOME%/$HOME/${home}-$OTHER/%Missing%/$$HOME/%%HOME%%", &env, true), "value/value/value-!/%Missing%/$HOME/%HOME%");
    assert_eq!(expand("$Home ${HOME} %Home% %%", &env, false), "value ${HOME} %Home% %");
}

#[test]
fn local_layers_resolve_dependencies_and_self_references_without_recursion_cycles() {
    let base = values(&[("Path", "system")]);
    let environment = LocalEnvironment {
        global: values(&[("Path", "%PATH%;global"), ("GLOBAL", "$SECOND"), ("SECOND", "g"), ("CYCLE", "$CYCLE")]),
        profile: values(&[("PATH", "${Path};profile"), ("PROFILE", "$GLOBAL/$TERM")]),
        refresh: true,
        ..LocalEnvironment::default()
    };
    let result = environment.apply(base, &[("TERM".into(), "term".into())], true);
    assert_eq!(result.get("PATH").map(String::as_str), Some("system;global;profile"));
    assert!(!result.contains_key("Path"));
    assert_eq!(result["PROFILE"], "g/term");
    assert_eq!(result["CYCLE"], "$CYCLE");
}

#[test]
fn registry_merge_replaces_stale_values_combines_paths_and_expands_only_expand_sz() {
    let process = values(&[("Path", "stale"), ("SystemRoot", r"C:\Windows"), ("PROCESS_ONLY", "keep"), ("NAME", "old")]);
    let registry_value = |name: &str, value: &str, expandable| RegistryValue { name: name.into(), value: value.into(), expandable };
    let machine = [registry_value("Path", r"%SystemRoot%\bin", true), registry_value("Name", "machine", false), registry_value("TOOL", "tools", false)];
    let user = [registry_value("PATH", r"D:\bin", false), registry_value("name", "%TOOL%/$TOOL", true), registry_value("LITERAL", "%TOOL%", false)];
    let merged = merge_registry(process, &machine, &user);
    assert_eq!(merged["PATH"], r"C:\Windows\bin;D:\bin");
    assert_eq!(merged["name"], "tools/$TOOL");
    assert_eq!(merged["LITERAL"], "%TOOL%");
    assert_eq!(merged["PROCESS_ONLY"], "keep");
    assert!(!merged.contains_key("NAME"));
}

#[test]
fn the_comspec_option_sets_an_aster_marker_for_windows_and_preserves_the_interpreter() {
    let inherited = values(&[("ComSpec", r"C:\Windows\System32\cmd.exe")]);
    let normal = LocalEnvironment::default();
    assert_eq!(normal.apply(inherited.clone(), &[], true), inherited);
    let marked = LocalEnvironment { set_comspec: true, ..normal };
    assert_eq!(marked.apply(inherited.clone(), &[], false), inherited);
    let mut windows = inherited.clone();
    windows.insert("ASTER".into(), "1".into());
    assert_eq!(marked.apply(inherited, &[], true), windows);
}

#[test]
fn a_literal_cmd_prompt_is_not_expanded_as_environment_references() {
    let process = values(&[("E", "escape"), ("P", "directory"), ("G", "greater")]);
    assert_eq!(expand(r"$$e]9;9;$$P$$e\$$P$$G", &process, true), r"$e]9;9;$P$e\$P$G");
}
