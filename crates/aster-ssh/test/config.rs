use super::*;
use std::collections::HashMap;

fn run(text: &str, files: &[(&str, &str)]) -> Vec<HostEntry> {
    let map: HashMap<PathBuf, String> = files
        .iter()
        .map(|(path, content)| (PathBuf::from(path), content.to_string()))
        .collect();
    parse(
        text,
        Path::new("/ssh"),
        Path::new("/home/u"),
        &|path| map.get(path).cloned(),
    )
}

fn aliases(entries: &[HostEntry]) -> Vec<&str> {
    entries.iter().map(|e| e.alias.as_str()).collect()
}

#[test]
fn basic_host_fields() {
    let entries = run(
        "Host web\n  HostName example.com\n  User root\n  Port 2222\n  ProxyJump jump\n  ProxyCommand nc %h %p\n",
        &[],
    );
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e.alias, "web");
    assert_eq!(e.host_name.as_deref(), Some("example.com"));
    assert_eq!(e.user.as_deref(), Some("root"));
    assert_eq!(e.port, Some(2222));
    assert_eq!(e.proxy_jump.as_deref(), Some("jump"));
    assert_eq!(e.proxy_command.as_deref(), Some("nc %h %p"));
}

#[test]
fn keys_case_insensitive_and_comments_skipped() {
    let entries = run("# note\n\nhOsT a\n  HOSTNAME h\n  # inner\n  uSeR bob\n", &[]);
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].host_name.as_deref(), Some("h"));
    assert_eq!(entries[0].user.as_deref(), Some("bob"));
}

#[test]
fn equals_separator() {
    let entries = run("Host=a\nHostName=h.example\nUser = carol\nPort= 22\n", &[]);
    assert_eq!(aliases(&entries), vec!["a"]);
    assert_eq!(entries[0].host_name.as_deref(), Some("h.example"));
    assert_eq!(entries[0].user.as_deref(), Some("carol"));
    assert_eq!(entries[0].port, Some(22));
}

#[test]
fn quoted_values_keep_spaces() {
    let entries = run(
        "Host \"my host\" b\n  IdentityFile \"/keys/my key\"\n",
        &[],
    );
    assert_eq!(aliases(&entries), vec!["my host", "b"]);
    assert_eq!(entries[0].identity_files, vec!["/keys/my key".to_string()]);
}

#[test]
fn multiple_patterns_make_entries_in_order_without_duplicates() {
    let entries = run("Host a b c\n  User x\nHost b d\n  User y\n", &[]);
    assert_eq!(aliases(&entries), vec!["a", "b", "c", "d"]);
    assert_eq!(entries[1].user.as_deref(), Some("x"));
    assert_eq!(entries[3].user.as_deref(), Some("y"));
}

#[test]
fn first_value_wins() {
    let entries = run("Host a\n  User one\n  User two\n  Port 1\n  Port 2\n", &[]);
    assert_eq!(entries[0].user.as_deref(), Some("one"));
    assert_eq!(entries[0].port, Some(1));
}

#[test]
fn wildcard_block_after_host_fills_missing_only() {
    let entries = run(
        "Host a\n  User own\nHost b\nHost *\n  User fallback\n  Port 2200\n",
        &[],
    );
    assert_eq!(aliases(&entries), vec!["a", "b"]);
    assert_eq!(entries[0].user.as_deref(), Some("own"));
    assert_eq!(entries[1].user.as_deref(), Some("fallback"));
    assert_eq!(entries[0].port, Some(2200));
}

#[test]
fn wildcard_block_before_host_wins() {
    let entries = run("Host *\n  User first\nHost a\n  User later\n", &[]);
    assert_eq!(entries[0].user.as_deref(), Some("first"));
}

#[test]
fn glob_patterns_question_and_star() {
    let entries = run(
        "Host web1 web22 db\nHost web?\n  User q\nHost *.x\n  Port 9\nHost d*\n  User star\n",
        &[],
    );
    assert_eq!(entries[0].user.as_deref(), Some("q"));
    assert_eq!(entries[1].user, None);
    assert_eq!(entries[2].user.as_deref(), Some("star"));
    assert_eq!(entries[0].port, None);
}

#[test]
fn negation_excludes() {
    let entries = run("Host a b\nHost * !b\n  User u\n", &[]);
    assert_eq!(entries[0].user.as_deref(), Some("u"));
    assert_eq!(entries[1].user, None);
}

#[test]
fn options_before_first_host_apply_to_all() {
    let entries = run("User global\nHost a\nHost b\n  User own\n", &[]);
    assert_eq!(entries[0].user.as_deref(), Some("global"));
    assert_eq!(entries[1].user.as_deref(), Some("global"));
}

#[test]
fn accumulating_options_and_tilde_expansion() {
    let entries = run(
        "Host a\n  IdentityFile ~/.ssh/id1\n  IdentityFile /abs/id2\n  LocalForward 8080 localhost:80\n  LocalForward 9090 localhost:90\n  RemoteForward 2222 localhost:22\n  DynamicForward 1080\nHost *\n  IdentityFile ~/.ssh/id3\n",
        &[],
    );
    let e = &entries[0];
    let home = Path::new("/home/u");
    assert_eq!(
        e.identity_files,
        vec![
            home.join(".ssh/id1").to_string_lossy().into_owned(),
            "/abs/id2".to_string(),
            home.join(".ssh/id3").to_string_lossy().into_owned(),
        ]
    );
    assert_eq!(
        e.local_forwards,
        vec!["8080 localhost:80".to_string(), "9090 localhost:90".to_string()]
    );
    assert_eq!(e.remote_forwards, vec!["2222 localhost:22".to_string()]);
    assert_eq!(e.dynamic_forwards, vec!["1080".to_string()]);
}

#[test]
fn include_relative_and_home() {
    let entries = run(
        "Include conf.d/extra \"~/more\"\nHost a\n",
        &[
            ("/ssh/conf.d/extra", "Host inc\n  User fromrel\n"),
            ("/home/u/more", "Host home\n  User fromhome\n"),
        ],
    );
    assert_eq!(aliases(&entries), vec!["inc", "home", "a"]);
    assert_eq!(entries[0].user.as_deref(), Some("fromrel"));
    assert_eq!(entries[1].user.as_deref(), Some("fromhome"));
}

#[test]
fn include_globs_discover_nested_hosts_and_apply_in_filename_order() {
    let root = std::env::temp_dir().join(format!("aster-ssh-includes-{}", std::process::id()));
    let conf = root.join("conf.d");
    std::fs::create_dir_all(&conf).unwrap();
    std::fs::write(conf.join("20-second.conf"), "Host build\n  Port 2202\n").unwrap();
    std::fs::write(conf.join("10-first.conf"), "Host build\n  Port 2201\nInclude more.conf\n").unwrap();
    std::fs::write(root.join("more.conf"), "Host archive\n  User backup\n").unwrap();
    let text = "Include conf.d/[12]*.conf conf.d/absent?.cfg\n";
    let read = |path: &Path| std::fs::read_to_string(path).ok();
    let entries = parse(text, &root, &root, &read);
    let build = resolve(text, &root, &root, &read, "build");
    for path in [conf.join("20-second.conf"), conf.join("10-first.conf"), root.join("more.conf")] {
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_dir(conf).unwrap();
    std::fs::remove_dir(root).unwrap();
    assert_eq!(aliases(&entries), ["build", "archive"]);
    assert_eq!(build.port, Some(2201));
    assert_eq!(entries[1].user.as_deref(), Some("backup"));
}

#[test]
fn include_inline_inside_block() {
    let entries = run(
        "Host a\n  Include extra\n  Port 5\n",
        &[("/ssh/extra", "User inner\n")],
    );
    assert_eq!(entries[0].user.as_deref(), Some("inner"));
    assert_eq!(entries[0].port, Some(5));
}

#[test]
fn include_cycle_terminates() {
    let entries = run(
        "Include a\nHost top\n",
        &[("/ssh/a", "Include b\nHost one\n"), ("/ssh/b", "Include a\n")],
    );
    assert!(aliases(&entries).contains(&"top"));
    assert!(aliases(&entries).contains(&"one"));
}

#[test]
fn include_wildcard_and_missing_skipped() {
    let entries = run(
        "Include conf.d/*\nInclude missing\nHost a\n",
        &[("/ssh/conf.d/*", "Host star\n")],
    );
    assert_eq!(aliases(&entries), vec!["a"]);
}

#[test]
fn match_block_ignored_until_next_host() {
    let entries = run(
        "Host a\n  User one\nMatch host a\n  User bad\n  Port 1\nHost b\n  Port 2\n",
        &[],
    );
    assert_eq!(aliases(&entries), vec!["a", "b"]);
    assert_eq!(entries[0].user.as_deref(), Some("one"));
    assert_eq!(entries[0].port, None);
    assert_eq!(entries[1].port, Some(2));
}

#[test]
fn non_numeric_port_ignored() {
    let entries = run("Host a\n  Port abc\n  Port 70000\n  Port 22\n", &[]);
    assert_eq!(entries[0].port, Some(22));
}

#[test]
fn resolving_an_unlisted_host_applies_wildcards_and_first_values_win() {
    let text = "Host *.lan\n  User pi\n  Port 2200\nHost *\n  User root\n  IdentityFile ~/.ssh/work\n";
    let entry = resolve(text, Path::new("/ssh"), Path::new("/home/u"), &|_| None, "nas.lan");
    assert_eq!(entry.alias, "nas.lan");
    assert_eq!(entry.user.as_deref(), Some("pi"));
    assert_eq!(entry.port, Some(2200));
    assert_eq!(entry.identity_files.len(), 1);
    let other = resolve(text, Path::new("/ssh"), Path::new("/home/u"), &|_| None, "example.com");
    assert_eq!((other.user.as_deref(), other.port), (Some("root"), None));
}
