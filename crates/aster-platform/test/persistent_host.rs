use super::*;

#[test]
fn session_lists_round_trip_and_reject_truncated_records() {
    let sessions = [
        SessionInfo { id: 0xaabb_ccdd_1122_3344, pid: 1, title: "Оболочка".into(), cwd: "D:\\Проект".into(), cols: 120, rows: 38 },
        SessionInfo { id: 28, pid: u32::MAX, title: String::new(), cwd: String::new(), cols: 75, rows: 20 },
    ];
    let encoded = encode_sessions(&sessions);
    assert_eq!(decode_sessions(&encoded).as_deref(), Some(sessions.as_slice()));
    assert_eq!(decode_sessions(&encode_sessions(&[])), Some(Vec::new()));
    assert_eq!(decode_sessions(&encoded[..encoded.len() - 1]), None);
    assert_eq!(decode_sessions(&[0xff; 20]), None);
}

#[test]
fn replay_keeps_exactly_two_mebibytes_after_wrapping_and_large_chunks() {
    let mut replay = Replay::default();
    replay.push(&vec![b'a'; REPLAY_LIMIT - 1]);
    replay.push(b"bcde");
    let snapshot = replay.snapshot();
    assert_eq!(snapshot.len(), REPLAY_LIMIT);
    assert!(snapshot[..REPLAY_LIMIT - 4].iter().all(|&byte| byte == b'a'));
    assert_eq!(&snapshot[REPLAY_LIMIT - 4..], b"bcde");

    let mut chunk = vec![b'z'; REPLAY_LIMIT + 3];
    chunk[..3].copy_from_slice(b"old");
    replay.push(&chunk);
    assert_eq!(replay.snapshot(), chunk[3..]);
}

#[test]
fn a_quick_shell_exit_can_be_attached_with_its_output() {
    struct Finished;
    impl Pty for Finished {
        fn resize(&self, _: u16, _: u16) {}
        fn kill(&self) {}
        fn pid(&self) -> u32 { 42 }
    }
    let host = Arc::new(Host::new(12));
    let (requests, write) = io::pipe().unwrap();
    let (read, replies) = io::pipe().unwrap();
    let server = host.clone();
    let worker = std::thread::spawn(move || server.serve(Connection::new(Box::new(requests), Box::new(replies), || Ok(())), "token", &|_, _| {
        Ok(Spawned { pty: Box::new(Finished), reader: Box::new(io::Cursor::new(b"completed")), writer: Box::new(io::sink()), raw: None, wait: Box::new(|| Some(17)) })
    }));
    let mut client = Client::connect(Connection::new(Box::new(read), Box::new(write), || Ok(())), "token").unwrap();
    let id = client.spawn(&SpawnRequest::default(), "Quick shell", "").unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    while !host.is_empty() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    let (spawned, replay) = client.attach(id).unwrap();
    assert_eq!(replay, b"completed");
    assert_eq!((spawned.wait)(), Some(17));
    worker.join().unwrap().unwrap();
}
