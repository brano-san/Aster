use super::*;
use std::io::{Read, Write};

#[test]
fn a_shell_runs_in_the_pty_with_its_size_and_reports_the_exit_code() {
    let req = SpawnRequest { command: "sh".into(), args: vec!["-c".into(), "stty size; echo aster-$ASTER_TEST; exit 3".into()], env: vec![("ASTER_TEST".into(), "ok".into())], cols: 91, rows: 17, ..Default::default() };
    let mut spawned = spawn(&req).expect("spawn");
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
    let _ = spawned.writer.flush();
    assert_eq!((spawned.wait)(), Some(3));
    drop(spawned.pty);
    drop(spawned.writer);
    let out = String::from_utf8_lossy(&collector.join().unwrap()).into_owned();
    assert!(out.contains("17 91"), "{out:?}");
    assert!(out.contains("aster-ok"), "{out:?}");
}

#[test]
fn environment_overrides_replace_inherited_variables() {
    let env = environment(&[("HOME".into(), "/x".into())]);
    let homes: Vec<_> = env.iter().filter(|e| e.to_bytes().starts_with(b"HOME=")).collect();
    assert_eq!(homes.len(), 1);
    assert_eq!(homes[0].to_bytes(), b"HOME=/x");
}
