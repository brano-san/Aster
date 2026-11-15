use super::*;
use std::io::{Read, Write};

fn cmd(script: &str) -> SpawnRequest {
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    SpawnRequest { command: format!(r"{system}\System32\cmd.exe"), args: vec!["/c".into(), script.into()], cwd: None, env: Vec::new(), cols: 80, rows: 24 }
}

fn delayed(script: &str) -> SpawnRequest {
    let mut req = cmd(script);
    req.args.insert(0, "/v:on".into());
    req
}

fn in_process_host(base: &str, token: &str) -> Result<(), PlatformError> {
    let (base, token) = (base.to_owned(), token.to_owned());
    std::thread::spawn(move || {
        let (reader, writer) = connect(&base).expect("the host connects");
        let _ = aster_platform::host::serve(reader, writer, &token, &|req| crate::conpty::spawn(req));
    });
    Ok(())
}

fn read_until(reader: &mut dyn Read, writer: &mut dyn Write, want: &str) -> String {
    let mut seen = String::new();
    let mut answered = false;
    let mut buf = [0u8; 4096];
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !seen.contains(want) && std::time::Instant::now() < deadline {
        let n = reader.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        seen.push_str(&String::from_utf8_lossy(&buf[..n]));
        if !answered && seen.contains("\x1b[6n") {
            answered = true;
            let _ = writer.write_all(b"\x1b[1;1R");
        }
    }
    seen
}

#[test]
fn a_shell_runs_through_the_pipe_host_and_reports_its_exit_code() {
    let Spawned { mut reader, mut writer, wait, .. } = hosted(&cmd("echo hosted-ok && exit /b 4"), in_process_host).expect("hosted spawn");
    let seen = read_until(&mut *reader, &mut *writer, "hosted-ok");
    assert!(seen.contains("hosted-ok"), "{seen:?}");
    assert_eq!(wait(), Some(4));
}

#[test]
fn input_reaches_a_hosted_shell() {
    let Spawned { mut reader, mut writer, pty, .. } = hosted(&delayed("set /p v=? & echo got-!v!"), in_process_host).expect("hosted spawn");
    let prompt = read_until(&mut *reader, &mut *writer, "?");
    assert!(prompt.contains('?'), "{prompt:?}");
    writer.write_all(b"abc\r").unwrap();
    let seen = read_until(&mut *reader, &mut *writer, "got-abc");
    assert!(seen.contains("got-abc"), "{seen:?}");
    pty.kill();
}

#[test]
fn a_host_that_never_starts_is_reported() {
    let failed = hosted(&cmd("echo"), |_, _| Err(PlatformError::new(1223, "The administrator request was declined")));
    assert_eq!(failed.err().map(|e| e.code), Some(1223));
}
