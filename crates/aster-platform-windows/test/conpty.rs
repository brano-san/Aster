use super::*;
use std::io::{Read, Write};

#[test]
fn arguments_are_quoted_by_the_windows_rules() {
    assert_eq!(quote_arg("plain"), "plain");
    assert_eq!(quote_arg(""), "\"\"");
    assert_eq!(quote_arg("with space"), "\"with space\"");
    assert_eq!(quote_arg("say \"hi\""), "\"say \\\"hi\\\"\"");
    assert_eq!(quote_arg("C:\\dir with space\\"), "\"C:\\dir with space\\\\\"");
    assert_eq!(command_line("cmd.exe", &["/k".into(), "echo a b".into()]), "cmd.exe /k \"echo a b\"");
}

#[test]
fn environment_overrides_replace_variables_case_insensitively() {
    let block = environment(&[("PATH".into(), "x".into()), ("ASTER_TEST_VAR".into(), "1".into())], None);
    let text = String::from_utf16_lossy(&block);
    let vars: Vec<&str> = text.split('\0').filter(|s| !s.is_empty()).collect();
    assert_eq!(vars.iter().filter(|v| v.to_uppercase().starts_with("PATH=")).count(), 1);
    assert!(vars.contains(&"PATH=x"));
    assert!(vars.contains(&"ASTER_TEST_VAR=1"));
    assert!(block.ends_with(&[0, 0]));
}

#[test]
fn a_spawned_shell_echoes_through_the_pseudo_console_and_reports_its_exit_code() {
    let req = SpawnRequest { command: "cmd.exe".into(), args: vec!["/c".into(), "echo aster-ok && exit 3".into()], cols: 80, rows: 24, ..Default::default() };
    let (exit, output) = run(&req);
    assert_eq!(exit, Some(3));
    assert!(output.contains("aster-ok"), "{output:?}");
}

fn run(req: &SpawnRequest) -> (Option<u32>, String) {
    let mut spawned = spawn(req).expect("conpty spawn");
    let mut reader = spawned.reader;
    let collector = std::thread::spawn(move || {
        let mut all = Vec::new();
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            all.extend_from_slice(&buf[..n]);
        }
        all
    });
    let _ = spawned.writer.write_all(b"\x1b[1;1R");
    let exit = (spawned.wait)();
    (exit, String::from_utf8_lossy(&collector.join().unwrap()).into_owned())
}

#[test]
fn cmd_keeps_a_working_comspec_and_receives_expanded_aster_environment() {
    let spec = aster_core::environment::LocalEnvironment {
        global: [("ASTER_TEST_BASE".into(), "global".into())].into(),
        profile: [("ASTER_ENV_TEST".into(), "$ASTER_TEST_BASE".into())].into(),
        refresh: true, set_comspec: true,
    };
    let req = SpawnRequest {
        command: "cmd.exe".into(),
        args: vec!["/d".into(), "/c".into(), "%COMSPEC% /d /c echo aster-%ASTER%-%ASTER_ENV_TEST%".into()],
        local_environment: Some(Box::new(spec)),
        cols: 80, rows: 24,
        ..Default::default()
    };
    let (exit, output) = run(&req);
    assert_eq!(exit, Some(0), "{output:?}");
    assert!(output.contains("aster-1-global"), "{output:?}");
}
