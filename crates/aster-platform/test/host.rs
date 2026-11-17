use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

fn request() -> SpawnRequest {
    SpawnRequest {
        command: "shell".into(),
        args: vec!["-l".into(), "two words".into()],
        cwd: Some(PathBuf::from("/srv/app")),
        env: vec![("TERM".into(), "xterm-256color".into()), ("EMPTY".into(), String::new())],
        cols: 120,
        rows: 40,
    }
}

#[derive(Clone, Default)]
struct Seen {
    sizes: Arc<Mutex<Vec<(u16, u16)>>>,
    killed: Arc<AtomicBool>,
}

struct FakePty(Seen, Arc<Mutex<Option<std::sync::mpsc::Sender<u32>>>>);

impl Pty for FakePty {
    fn resize(&self, cols: u16, rows: u16) {
        self.0.sizes.lock().unwrap().push((cols, rows));
    }

    fn kill(&self) {
        self.0.killed.store(true, Ordering::SeqCst);
        if let Some(tx) = self.1.lock().unwrap().take() {
            let _ = tx.send(9);
        }
    }

    fn pid(&self) -> u32 {
        4242
    }
}

fn fake_shell(seen: Seen) -> Spawned {
    let (in_read, in_write) = std::io::pipe().unwrap();
    let (out_read, mut out_write) = std::io::pipe().unwrap();
    let (exit_tx, exit_rx) = channel();
    let exit = Arc::new(Mutex::new(Some(exit_tx)));
    let ender = exit.clone();
    std::thread::spawn(move || {
        let mut input = in_read;
        let mut buf = [0u8; 256];
        while let Ok(n) = input.read(&mut buf) {
            if n == 0 {
                break;
            }
            let text = String::from_utf8_lossy(&buf[..n]).into_owned();
            if text.trim() == "exit" {
                if let Some(tx) = ender.lock().unwrap().take() {
                    let _ = tx.send(3);
                }
                break;
            }
            if out_write.write_all(format!("echo:{text}").as_bytes()).is_err() {
                break;
            }
        }
    });
    Spawned { pty: Box::new(FakePty(seen, exit)), reader: Box::new(out_read), writer: Box::new(in_write), raw: None, wait: Box::new(move || exit_rx.recv().ok()) }
}

fn connected(token_host: &'static str, token_client: &str, seen: Seen, spawn_fails: bool) -> Result<Spawned, PlatformError> {
    let (to_host_read, to_host_write) = std::io::pipe().unwrap();
    let (to_client_read, to_client_write) = std::io::pipe().unwrap();
    std::thread::spawn(move || {
        let spawn = move |req: &SpawnRequest| {
            assert_eq!(req, &request());
            if spawn_fails {
                Err(PlatformError::new(2, "no such shell"))
            } else {
                Ok(fake_shell(seen.clone()))
            }
        };
        let _ = serve(Box::new(to_host_read), Box::new(to_client_write), token_host, &spawn);
    });
    client(Box::new(to_client_read), Box::new(to_host_write), token_client, &request())
}

fn read_some(r: &mut dyn Read, want: &str) -> String {
    let mut seen = String::new();
    let mut buf = [0u8; 256];
    while !seen.contains(want) {
        let n = r.read(&mut buf).unwrap();
        assert!(n > 0, "closed before {want:?}: {seen:?}");
        seen.push_str(&String::from_utf8_lossy(&buf[..n]));
    }
    seen
}

#[test]
fn spawn_requests_survive_encoding() {
    assert_eq!(decode_request(&encode_request(&request())), Some(request()));
    let mut bare = request();
    bare.cwd = None;
    bare.env.clear();
    assert_eq!(decode_request(&encode_request(&bare)), Some(bare));
    assert_eq!(decode_request(&[1, 2, 3]), None);
}

#[test]
fn frames_round_trip_and_oversized_ones_are_refused() {
    let mut buf = Vec::new();
    write_frame(&mut buf, DATA, b"hello").unwrap();
    write_frame(&mut buf, KILL, &[]).unwrap();
    let mut r: &[u8] = &buf;
    assert_eq!(read_frame(&mut r).unwrap(), Some((DATA, b"hello".to_vec())));
    assert_eq!(read_frame(&mut r).unwrap(), Some((KILL, Vec::new())));
    assert_eq!(read_frame(&mut r).unwrap(), None);
    let huge = [DATA, 0xff, 0xff, 0xff, 0x7f];
    assert!(read_frame(&mut &huge[..]).is_err());
}

#[test]
fn a_hosted_shell_echoes_resizes_and_reports_its_exit_code() {
    let seen = Seen::default();
    let Spawned { pty, mut reader, mut writer, wait, .. } = connected("t0ken", "t0ken", seen.clone(), false).unwrap();
    assert_eq!(pty.pid(), 4242);
    writer.write_all(b"ls").unwrap();
    assert!(read_some(&mut *reader, "echo:ls").contains("echo:ls"));
    pty.resize(100, 30);
    writer.write_all(b"pwd").unwrap();
    read_some(&mut *reader, "echo:pwd");
    assert_eq!(seen.sizes.lock().unwrap().as_slice(), [(100, 30)]);
    writer.write_all(b"exit").unwrap();
    assert_eq!(wait(), Some(3));
    let mut rest = Vec::new();
    assert!(reader.read_to_end(&mut rest).is_ok(), "output closes after the exit");
}

#[test]
fn killing_through_the_host_stops_the_shell() {
    let seen = Seen::default();
    let spawned = connected("k", "k", seen.clone(), false).unwrap();
    spawned.pty.kill();
    assert_eq!((spawned.wait)(), Some(9));
    assert!(seen.killed.load(Ordering::SeqCst));
}

#[test]
fn a_wrong_token_or_a_failed_spawn_is_reported_to_the_client() {
    let wrong = connected("right", "wrong", Seen::default(), false);
    assert!(wrong.is_err());
    let failed = connected("t", "t", Seen::default(), true);
    assert_eq!(failed.err().map(|e| e.message), Some("no such shell".to_owned()));
}

#[test]
fn closing_the_client_kills_the_hosted_shell() {
    let seen = Seen::default();
    let spawned = connected("c", "c", seen.clone(), false).unwrap();
    drop(spawned);
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !seen.killed.load(Ordering::SeqCst) {
        assert!(std::time::Instant::now() < deadline, "the host never killed the shell");
        std::thread::sleep(Duration::from_millis(10));
    }
}
