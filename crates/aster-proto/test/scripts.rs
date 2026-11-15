use super::*;

fn script(expect: &str, send: &str) -> Script {
    Script {
        expect: expect.to_owned(),
        send: send.to_owned(),
        ..Script::default()
    }
}

#[test]
fn unescape_control_sequences() {
    assert_eq!(
        unescape(r"\a\b\e\f\n\r\t\v\\"),
        [7, 8, 27, 12, 10, 13, 9, 11, 92]
    );
}

#[test]
fn unescape_hex_and_unicode() {
    assert_eq!(unescape(r"\x00\xFf\x414"), [0, 255, 65, 52]);
    assert_eq!(unescape(r"\u041f\u0440ивет"), "Привет".as_bytes());
    assert_eq!(unescape(r"\u00411"), b"A1");
}

#[test]
fn unknown_incomplete_and_invalid_escapes_are_literal() {
    for text in [
        r"\q", r"\x4", r"\xGG", r"\u123", r"\uD800", r"\", r"\xя", r"\uZZZZ",
    ] {
        assert_eq!(unescape(text), text.as_bytes(), "{text}");
    }
}

#[test]
fn unescape_plain_utf8() {
    assert_eq!(unescape("🙂 текст"), "🙂 текст".as_bytes());
}

#[test]
fn login_password_and_done() {
    let mut runner = ScriptRunner::new(vec![
        script("login:", r"root\r"),
        script("Password:", r"secret\r"),
    ])
    .unwrap();
    assert!(!runner.done());
    assert!(runner.start().is_empty());
    assert_eq!(runner.feed(b"login:"), b"root\r");
    assert!(!runner.done());
    assert_eq!(runner.feed(b"Password:"), b"secret\r");
    assert!(runner.done());
    assert!(runner.feed(b"Password:").is_empty());
    assert!(runner.start().is_empty());
}

#[test]
fn substring_split_across_feeds() {
    let mut runner = ScriptRunner::new(vec![script("Password:", "ok")]).unwrap();
    assert!(runner.feed(b"Pass").is_empty());
    assert!(runner.feed(b"wo").is_empty());
    assert_eq!(runner.feed(b"rd:"), b"ok");
}

#[test]
fn regex_prompt() {
    let mut entry = script(r"\$\s*$", "ready");
    entry.regex = true;
    let mut runner = ScriptRunner::new(vec![entry]).unwrap();
    assert!(runner.feed(b"user").is_empty());
    assert_eq!(runner.feed(b"$ "), b"ready");
}

#[test]
fn color_sequences_inside_prompt() {
    let mut runner = ScriptRunner::new(vec![script("Password:", "ok")]).unwrap();
    assert_eq!(runner.feed(b"Pass\x1b[31mword\x1b[0m:"), b"ok");
}

#[test]
fn split_csi_osc_and_short_escape_sequences() {
    let mut runner = ScriptRunner::new(vec![script("Password:", "ok")]).unwrap();
    for chunk in [
        b"Pa\x1b".as_slice(),
        b"[3",
        b"1mss\x1b]title",
        b"\x1b",
        b"\\wo\x1b]ignored",
        b"\x07r\x1b",
        b"Md:",
    ] {
        let result = runner.feed(chunk);
        if chunk == b"Md:" {
            assert_eq!(result, b"ok");
        } else {
            assert!(result.is_empty());
        }
    }
}

#[test]
fn optional_script_can_be_skipped() {
    let mut optional = script("login:", "user");
    optional.optional = true;
    let mut runner = ScriptRunner::new(vec![optional, script("Password:", "secret")]).unwrap();
    assert_eq!(runner.feed(b"Password:"), b"secret");
    assert!(runner.done());
}

#[test]
fn required_script_blocks_later_matches() {
    let mut runner = ScriptRunner::new(vec![
        script("login:", "user"),
        script("Password:", "secret"),
    ])
    .unwrap();
    assert!(runner.feed(b"Password:").is_empty());
    assert!(!runner.done());
}

#[test]
fn first_matching_script_wins_over_later_option() {
    let mut optional = script("ready", "first");
    optional.optional = true;
    let mut runner = ScriptRunner::new(vec![optional, script("ready", "second")]).unwrap();
    assert_eq!(runner.feed(b"ready"), b"first");
    assert!(!runner.done());
    assert_eq!(runner.feed(b"ready"), b"second");
}

#[test]
fn empty_expectations_at_start_and_after_match() {
    let mut runner = ScriptRunner::new(vec![
        script("", "a"),
        script("", "b"),
        script("ready", "c"),
        script("", "d"),
        script("", "e"),
    ])
    .unwrap();
    assert_eq!(runner.start(), b"ab");
    assert!(runner.start().is_empty());
    assert_eq!(runner.feed(b"ready"), b"cde");
    assert!(runner.done());
}

#[test]
fn feed_can_run_initial_empty_expectation() {
    let mut runner = ScriptRunner::new(vec![script("", "a"), script("ready", "b")]).unwrap();
    assert_eq!(runner.feed(b"ready"), b"ab");
}

#[test]
fn buffered_tail_serves_next_script() {
    let mut runner =
        ScriptRunner::new(vec![script("login:", "a"), script("Password:", "b")]).unwrap();
    assert_eq!(runner.feed(b"login:Password:"), b"ab");
    assert!(runner.done());
}

#[test]
fn consumed_prefix_is_not_reused() {
    let mut runner = ScriptRunner::new(vec![script("end", "a"), script("begin", "b")]).unwrap();
    assert_eq!(runner.feed(b"begin end"), b"a");
    assert!(runner.feed(b"").is_empty());
    assert_eq!(runner.feed(b"begin"), b"b");
}

#[test]
fn invalid_regex_reports_zero_based_index() {
    let mut invalid = script("[", "");
    invalid.regex = true;
    let error = ScriptRunner::new(vec![script("ok", ""), invalid])
        .err()
        .unwrap();
    assert!(error.contains('1'));
    assert!(error.contains("unclosed"));
}

#[test]
fn empty_script_list_is_done() {
    let mut runner = ScriptRunner::new(vec![]).unwrap();
    assert!(runner.done());
    assert!(runner.start().is_empty());
    assert!(runner.feed(b"anything").is_empty());
}

#[test]
fn buffer_keeps_last_4096_characters() {
    let mut runner = ScriptRunner::new(vec![script("prefix", "a"), script("tail", "b")]).unwrap();
    assert!(runner
        .feed(format!("prefix{}", "я".repeat(4096)).as_bytes())
        .is_empty());
    assert!(runner.feed(b"tail").is_empty());
    assert_eq!(runner.feed(b"prefix tail"), b"ab");
}

#[test]
fn unicode_limit_counts_characters_not_bytes() {
    let expected = format!("я{}", "b".repeat(4095));
    let mut runner = ScriptRunner::new(vec![script(&expected, "ok")]).unwrap();
    assert_eq!(runner.feed(format!("discard{expected}").as_bytes()), b"ok");
}

#[test]
fn utf8_can_cross_feed_boundaries() {
    let mut runner = ScriptRunner::new(vec![script("пароль:", "ok")]).unwrap();
    let bytes = "пароль:".as_bytes();
    for byte in &bytes[..bytes.len() - 1] {
        assert!(runner.feed(&[*byte]).is_empty());
    }
    assert_eq!(runner.feed(b":"), b"ok");
}

#[test]
fn invalid_utf8_is_lossy_text() {
    let mut runner = ScriptRunner::new(vec![script("�ready", "ok")]).unwrap();
    assert_eq!(runner.feed(b"\xffready"), b"ok");
}

#[test]
fn zero_length_regex_advances_script() {
    let mut entry = script("^", "ok");
    entry.regex = true;
    let mut runner = ScriptRunner::new(vec![entry, script("", "done")]).unwrap();
    assert_eq!(runner.feed(b""), b"okdone");
    assert!(runner.done());
}
