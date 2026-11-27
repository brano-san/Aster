use super::*;
use std::io::{Read, Write};
use std::time::Duration;

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

#[test]
fn interactive_powershell_marks_output_and_reports_native_exit_code() {
    let modules = std::path::PathBuf::from(std::env::var_os("ProgramFiles").unwrap()).join("WindowsPowerShell").join("Modules");
    let req = SpawnRequest {
        command: "powershell.exe".into(),
        args: vec!["-NoLogo".into(), "-NoProfile".into()],
        local_environment: Some(Box::default()),
        env: vec![("PSModulePath".into(), modules.to_string_lossy().into_owned())],
        cols: 80,
        rows: 24,
        ..Default::default()
    };
    let mut spawned = spawn(&req).expect("conpty spawn");
    let mut reader = spawned.reader;
    let (tx, rx) = std::sync::mpsc::channel();
    let collector = std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() { break; }
        }
    });
    spawned.writer.write_all(b"\x1b[1;1R").unwrap();
    let mut output = String::new();
    let mut wait_for = |needle| {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !output.contains(needle) {
            let Ok(bytes) = rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())) else { return false };
            output.push_str(&String::from_utf8_lossy(&bytes));
        }
        true
    };
    let started = wait_for("\x1b]133;B");
    let finished = if started {
        spawned.writer.write_all(b"cmd /d /c \"echo aster-native & exit 7\"\r").unwrap();
        wait_for("\x1b]133;D;7")
    } else {
        false
    };
    spawned.pty.kill();
    let _ = (spawned.wait)();
    collector.join().unwrap();
    assert!(started && finished, "{output:?}");
    assert!(output.contains("\x1b]133;C"));
    assert!(output.contains("aster-native"));
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
