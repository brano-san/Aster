use super::*;

fn session(id: u64) -> SessionInfo {
    SessionInfo { id, pid: 124, title: "Shell".into(), cwd: "work".into(), cols: 120, rows: 40 }
}

#[test]
fn restore_attaches_each_live_saved_id_once_and_kills_unmatched_sessions() {
    let saved = [Some(2), None, Some(32), Some(3), Some(2)];
    let (panes, kill) = match_sessions(&saved, vec![session(17), session(3), session(2), session(8)]);
    assert_eq!(panes, [Some(session(2)), None, None, Some(session(3)), None]);
    assert_eq!(kill, [8, 17]);
    assert_eq!(match_sessions(&saved, Vec::new()), (vec![None; saved.len()], Vec::new()));
}

#[test]
fn idle_exit_requires_thirty_continuous_seconds_without_clients_or_sessions() {
    let now = Instant::now();
    let mut idle = Idle::default();
    assert!(!idle.expired(now, true));
    assert!(!idle.expired(now + Duration::from_secs(29), true));
    assert!(!idle.expired(now + Duration::from_secs(30), false));
    assert!(!idle.expired(now + Duration::from_secs(31), true));
    assert!(!idle.expired(now + Duration::from_secs(60), true));
    assert!(idle.expired(now + Duration::from_secs(61), true));
}
