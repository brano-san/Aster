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

fn simulate_shutdown(exit_on: Option<libc::c_int>) -> (Vec<libc::c_int>, Duration) {
    let alive = std::cell::Cell::new(true);
    let mut signals = Vec::new();
    let mut elapsed = Duration::ZERO;
    let mut signal = |sig| {
        signals.push(sig);
        if Some(sig) == exit_on {
            alive.set(false);
        }
    };
    hang_up(&mut signal);
    escalate(|| alive.get(), signal, |interval| {
        assert_eq!(interval, Duration::from_millis(50));
        elapsed += interval;
    });
    (signals, elapsed)
}

#[test]
fn a_group_that_exits_on_hangup_needs_no_escalation() {
    assert_eq!(simulate_shutdown(Some(libc::SIGHUP)), (vec![libc::SIGHUP, libc::SIGCONT], Duration::ZERO));
}

#[test]
fn a_group_that_exits_on_term_is_spared_kill() {
    assert_eq!(simulate_shutdown(Some(libc::SIGTERM)), (vec![libc::SIGHUP, libc::SIGCONT, libc::SIGTERM], TERM_AFTER));
}

#[test]
fn a_surviving_group_receives_kill_after_both_delays() {
    assert_eq!(simulate_shutdown(None), (vec![libc::SIGHUP, libc::SIGCONT, libc::SIGTERM, libc::SIGKILL], TERM_AFTER + KILL_AFTER));
}

#[test]
fn a_shell_that_ignores_hangup_exits_on_term() {
    let req = SpawnRequest { command: "sh".into(), args: vec!["-c".into(), r#"trap "" HUP; printf ready; sleep 30"#.into()], ..Default::default() };
    let mut spawned = spawn(&req).expect("spawn");
    let mut ready = [0; 5];
    spawned.reader.read_exact(&mut ready).expect("shell ready");
    assert_eq!(&ready, b"ready");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || tx.send((spawned.wait)()).unwrap());
    spawned.pty.kill();
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).expect("shell exit"), Some(128 + libc::SIGTERM as u32));
}
