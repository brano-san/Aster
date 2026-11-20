use super::*;
use crate::console::Input;
use crate::test_server::*;

#[tokio::test]
async fn handshake_cancellation_releases_a_stalled_stream() {
    use tokio::io::AsyncReadExt;
    let (stream, _peer) = tokio::io::duplex(64);
    let (ready, pending) = tokio::sync::oneshot::channel();
    let mut guarded = HandshakeStream { stream: Some(stream), pending: Some(pending) };
    drop(ready);
    assert_eq!(guarded.read_u8().await.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
}

#[tokio::test]
async fn completed_handshake_keeps_the_transport_readable() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (stream, mut peer) = tokio::io::duplex(64);
    let (ready, pending) = tokio::sync::oneshot::channel();
    let mut guarded = HandshakeStream { stream: Some(stream), pending: Some(pending) };
    ready.send(()).unwrap();
    peer.write_u8(42).await.unwrap();
    assert_eq!(guarded.read_u8().await.unwrap(), 42);
}

#[tokio::test]
async fn cancelled_handshake_releases_the_unanswered_host_key_prompt() {
    use russh::client::Handler;
    let dir = temp_ssh_dir();
    let req = request("ann@host".into(), dir.clone());
    let (out, messages) = crossbeam_channel::unbounded();
    let (_input, rx) = tokio::sync::mpsc::unbounded_channel();
    let console = Arc::new(Mutex::new(Console::new(out, rx, (80, 24))));
    let dest = context_destination(&req.destination, &req, &req.profile, &console).await.unwrap();
    let (ready, pending) = tokio::sync::oneshot::channel();
    let mut handler = Client::new(&dest, &req, console.clone(), pending);
    let key = PublicKey::from_openssh(CLIENT_PUB).unwrap().into();
    let check = handler.check_server_key(&key);
    tokio::pin!(check);
    tokio::select! {
        biased;
        _ = &mut check => panic!("the new key requires confirmation"),
        _ = std::future::ready(()) => {}
    }
    assert!(messages.try_iter().any(|b| b.starts_with(b"Are you sure")));
    drop(ready);
    assert!(matches!(check.await, Err(russh::Error::ConnectionTimeout)));
    assert!(console.try_lock().is_ok());
    assert!(!dir.join("known_hosts").exists());
}

#[test]
fn remembered_cwd_is_a_single_shell_argument() {
    assert_eq!(cwd_command(true, Some("/src/it's $(odd)")), Some(r#"cd '/src/it'\''s $(odd)' && exec "$SHELL" -l"#.into()));
    assert_eq!(cwd_command(false, Some("/tmp")), None);
    assert_eq!(cwd_command(true, Some("-")), None);
    assert_eq!(cwd_command(true, None), None);
}

#[tokio::test]
async fn missing_login_is_prompted_in_the_pane_before_pool_lookup() {
    let (out, shown) = crossbeam_channel::unbounded();
    let (send, input) = tokio::sync::mpsc::unbounded_channel();
    let console = Arc::new(Mutex::new(Console::new(out, input, (90, 25))));
    let req = request("example".into(), temp_ssh_dir());
    send.send(Input::Data(b"\r".to_vec())).unwrap();
    send.send(Input::Data(b"alice\r".to_vec())).unwrap();
    let destination = context_destination(&req.destination, &req, &req.profile, &console).await.unwrap();
    assert_eq!(destination.user, "alice");
    let shown: Vec<u8> = shown.try_iter().flatten().collect();
    assert_eq!(String::from_utf8(shown).unwrap().matches("Login as: ").count(), 2);
    send.send(Input::Close).unwrap();
    assert!(context_destination(&req.destination, &req, &req.profile, &console).await.is_err());
}

#[test]
fn a_new_host_is_confirmed_then_a_password_opens_a_shell_that_follows_the_pane() {
    let (port, seen) = serve();
    let dir = temp_ssh_dir();
    let mut pane = connect_to(port, dir.clone());
    pane.wait_for("continue connecting (yes/no)? ");
    assert!(pane.seen.contains("SHA256:"));
    pane.send("yes\r");
    pane.wait_for("ann@127.0.0.1's password: ");
    pane.send("secret\r");
    pane.wait_for("welcome");
    assert!(pane.seen.contains("\x1b[H\x1b[2Jwelcome"));
    assert!(!pane.seen.contains("secret"), "the password is never echoed");
    pane.send("hi\r");
    pane.wait_for("got:hi");
    pane.conn.input.send(Input::Resize(120, 40)).unwrap();
    pane.send("exit\r");
    assert_eq!(pane.exit(), 7);
    assert_eq!(seen.sizes.lock().unwrap().as_slice(), [(90, 25), (120, 40)]);
    let known = std::fs::read_to_string(dir.join("known_hosts")).unwrap();
    assert!(known.contains(&format!("[127.0.0.1]:{port}")));

    let mut again = connect_to(port, dir);
    again.wait_for("password: ");
    assert!(!again.seen.contains("authenticity"), "a known host is not asked about again");
    for _ in 0..3 {
        again.send("wrong\r");
    }
    again.wait_for("permission denied");
    assert_eq!(again.exit(), FAILED);
}

#[test]
fn an_encrypted_key_from_the_config_logs_in_after_its_passphrase() {
    let (port, _) = serve();
    let dir = temp_ssh_dir();
    let key = dir.join("work_key");
    std::fs::write(&key, CLIENT_KEY).unwrap();
    std::fs::write(dir.join("config"), format!("Host work\n  HostName 127.0.0.1\n  Port {port}\n  User ann\n  IdentityFile {}\n", key.display())).unwrap();
    let mut pane = Pane { conn: start(request("work".into(), dir)), seen: String::new() };
    pane.wait_for("(yes/no)? ");
    pane.send("yes\r");
    pane.wait_for("Enter passphrase for key");
    pane.send("nope\r");
    pane.wait_for("Bad passphrase");
    pane.send("pw\r");
    pane.wait_for("welcome");
    assert!(!pane.seen.contains("password:"));
    pane.send("exit\r");
    assert_eq!(pane.exit(), 7);
}

#[test]
fn a_changed_host_key_is_refused_with_a_warning() {
    let (port, _) = serve();
    let dir = temp_ssh_dir();
    std::fs::write(dir.join("known_hosts"), format!("[127.0.0.1]:{port} {}\n", CLIENT_PUB.trim())).unwrap();
    let mut pane = connect_to(port, dir);
    pane.wait_for("has changed");
    assert_eq!(pane.exit(), FAILED);
    assert!(!pane.seen.contains("password"));
}

#[test]
fn declining_the_host_or_closing_the_pane_ends_the_session() {
    let (port, _) = serve();
    let mut pane = connect_to(port, temp_ssh_dir());
    pane.wait_for("(yes/no)? ");
    pane.send("no\r");
    assert_eq!(pane.exit(), FAILED);
    let mut closed = connect_to(port, temp_ssh_dir());
    closed.wait_for("(yes/no)? ");
    closed.conn.input.send(Input::Close).unwrap();
    assert_eq!(closed.exit(), FAILED);
}

#[test]
fn an_unreachable_host_reports_the_reason() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let mut pane = connect_to(port, temp_ssh_dir());
    pane.wait_for("cannot connect to 127.0.0.1");
    assert_eq!(pane.exit(), FAILED);
}

#[test]
fn a_jump_host_tunnels_to_the_target_and_each_hop_authenticates() {
    let (jump_port, jump_seen) = serve();
    let (target_port, _) = serve();
    let dir = temp_ssh_dir();
    std::fs::write(dir.join("config"), format!("Host inner\n  HostName 127.0.0.1\n  Port {target_port}\n  User ann\n  ProxyJump ann@127.0.0.1:{jump_port}\n")).unwrap();
    let mut pane = Pane { conn: start(request("inner".into(), dir)), seen: String::new() };
    pane.wait_for(&format!("'[127.0.0.1]:{jump_port}'"));
    pane.send("yes\r");
    pane.wait_for("password: ");
    pane.send("secret\r");
    pane.wait_for(&format!("'[127.0.0.1]:{target_port}'"));
    pane.send("yes\r");
    pane.wait_for("Permanently added");
    pane.seen.clear();
    pane.wait_for("password: ");
    pane.send("secret\r");
    pane.wait_for("welcome");
    assert_eq!(jump_seen.tunnels.lock().unwrap().as_slice(), [format!("127.0.0.1:{target_port}")]);
    pane.send("exit\r");
    assert_eq!(pane.exit(), 7);
}

fn shared(port: u16, dir: PathBuf) -> Pane {
    let mut req = request(format!("ann@127.0.0.1:{port}"), dir);
    req.share = true;
    Pane { conn: start(req), seen: String::new() }
}

#[test]
fn tabs_to_the_same_host_share_one_login_until_the_last_one_closes() {
    let (port, seen) = serve();
    let dir = temp_ssh_dir();
    let mut first = shared(port, dir.clone());
    first.wait_for("(yes/no)? ");
    first.send("yes\r");
    first.wait_for("password: ");
    first.send("secret\r");
    first.wait_for("welcome");

    let mut req = request(format!("ann@127.0.0.1:{port}"), dir.clone());
    req.share = true;
    req.profile.term = Some("vt100".into());
    req.profile.remember_cwd = Some(true);
    req.cwd = Some("/remote folder".into());
    req.clear_after_connect = false;
    let mut second = Pane { conn: start(req), seen: String::new() };
    second.wait_for("cd '/remote folder' && exec \"$SHELL\" -l");
    assert_eq!(seen.terms.lock().unwrap().as_slice(), ["xterm-256color", "vt100"]);
    assert!(!second.seen.contains("\x1b[2J"));
    assert!(!second.seen.contains("password"), "the second tab reuses the login: {:?}", second.seen);

    first.send("exit\r");
    assert_eq!(first.exit(), 7);
    second.send("still\r");
    second.wait_for("got:still");
    second.conn.input.send(Input::Close).unwrap();
    assert_eq!(second.exit(), 0);

    std::thread::sleep(std::time::Duration::from_millis(200));
    let mut third = shared(port, dir);
    third.wait_for("password: ");
}

#[test]
fn unshared_tabs_each_log_in() {
    let (port, _) = serve();
    let dir = temp_ssh_dir();
    let mut req = request(format!("ann@127.0.0.1:{port}"), dir.clone());
    req.profile.x11 = Some(true);
    req.x11_display = "localhost:12.3".into();
    let mut first = Pane { conn: start(req), seen: String::new() };
    first.wait_for("(yes/no)? ");
    first.send("yes\r");
    first.wait_for("password: ");
    first.send("secret\r");
    first.wait_for("welcome");
    assert!(first.seen.contains("X11 forwarding enabled."));
    let mut second = connect_to(port, dir);
    second.wait_for("password: ");
}

fn remembering(port: u16, dir: PathBuf, secrets: Arc<crate::secrets::Memory>) -> Pane {
    let mut req = request(format!("ann@127.0.0.1:{port}"), dir);
    req.secrets = Some(secrets);
    req.remember = true;
    Pane { conn: start(req), seen: String::new() }
}

#[test]
fn a_typed_password_can_be_saved_and_is_then_used_without_asking() {
    use crate::secrets::Secrets;
    let (port, _) = serve();
    let dir = temp_ssh_dir();
    let secrets = Arc::new(crate::secrets::Memory::default());
    let mut first = remembering(port, dir.clone(), secrets.clone());
    first.wait_for("(yes/no)? ");
    first.send("yes\r");
    first.wait_for("password: ");
    first.send("secret\r");
    first.wait_for("for future connections (yes/no)? ");
    first.send("yes\r");
    first.wait_for("welcome");
    let key = crate::secrets::password_key("ann", "127.0.0.1", port);
    assert_eq!(secrets.get(&key).as_deref(), Some("secret"));

    let mut second = remembering(port, dir.clone(), secrets.clone());
    second.wait_for("welcome");
    assert!(!second.seen.contains("password"), "{:?}", second.seen);

    secrets.set(&key, "outdated");
    let mut third = remembering(port, dir, secrets.clone());
    third.wait_for("The saved password was not accepted");
    third.wait_for("password: ");
    assert_eq!(secrets.get(&key), None, "a rejected password is forgotten");
}

#[test]
fn declining_keeps_nothing_and_a_saved_passphrase_unlocks_the_key() {
    use crate::secrets::Secrets;
    let (port, _) = serve();
    let dir = temp_ssh_dir();
    let key = dir.join("work_key");
    std::fs::write(&key, CLIENT_KEY).unwrap();
    std::fs::write(dir.join("config"), format!("Host work\n  HostName 127.0.0.1\n  Port {port}\n  User ann\n  IdentityFile {}\n", key.display())).unwrap();
    let secrets = Arc::new(crate::secrets::Memory::default());
    let open = |secrets: Arc<crate::secrets::Memory>| {
        let mut req = request("work".into(), dir.clone());
        req.secrets = Some(secrets);
        req.remember = true;
        Pane { conn: start(req), seen: String::new() }
    };
    let mut first = open(secrets.clone());
    first.wait_for("(yes/no)? ");
    first.send("yes\r");
    first.wait_for("Enter passphrase");
    first.send("pw\r");
    first.wait_for("for future connections (yes/no)? ");
    first.send("no\r");
    first.wait_for("welcome");
    assert_eq!(secrets.get(&crate::secrets::passphrase_key(&key)), None);
    secrets.set(&crate::secrets::passphrase_key(&key), "pw");
    let mut second = open(secrets);
    second.wait_for("welcome");
    assert!(!second.seen.contains("passphrase"));
}

fn fake_agent() -> String {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            #[cfg(windows)]
            let (path, mut stream) = {
                let path = format!(r"\\.\pipe\{}", temp_ssh_dir().file_name().unwrap().to_string_lossy());
                let server = tokio::net::windows::named_pipe::ServerOptions::new().first_pipe_instance(true).create(&path).unwrap();
                tx.send(path.clone()).unwrap();
                server.connect().await.unwrap();
                (path, server)
            };
            #[cfg(not(windows))]
            let (path, mut stream) = {
                let path = temp_ssh_dir().join("agent.sock").display().to_string();
                let listener = tokio::net::UnixListener::bind(&path).unwrap();
                tx.send(path.clone()).unwrap();
                (path.clone(), listener.accept().await.unwrap().0)
            };
            let _ = path;
            let mut buf = [0u8; 4];
            stream.read_exact(&mut buf).await.unwrap();
            assert_eq!(&buf, b"ping");
            stream.write_all(b"pong").await.unwrap();
            stream.flush().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        });
    });
    rx.recv().unwrap()
}

#[test]
fn a_forwarded_agent_channel_reaches_the_local_agent_only_when_asked() {
    let (port, _) = serve();
    let dir = temp_ssh_dir();
    let agent = fake_agent();
    std::fs::write(dir.join("config"), format!("Host fwd\n  HostName 127.0.0.1\n  Port {port}\n  User ann\n  ForwardAgent yes\n  IdentityAgent {agent}\n")).unwrap();
    let mut pane = Pane { conn: start(request("fwd".into(), dir.clone())), seen: String::new() };
    pane.wait_for("(yes/no)? ");
    pane.send("yes\r");
    pane.wait_for("password: ");
    pane.send("secret\r");
    pane.wait_for("agent said pong");

    std::fs::write(dir.join("config"), format!("Host plain\n  HostName 127.0.0.1\n  Port {port}\n  User ann\n")).unwrap();
    let mut plain = Pane { conn: start(request("plain".into(), dir)), seen: String::new() };
    plain.wait_for("password: ");
    plain.send("secret\r");
    plain.wait_for("welcome");
    plain.send("hi\r");
    plain.wait_for("got:hi");
    assert!(!plain.seen.contains("agent said"), "no agent channel without ForwardAgent");
}

fn echo_server() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                use std::io::{Read, Write};
                let mut stream = stream;
                let mut buf = [0u8; 1024];
                while let Ok(n) = stream.read(&mut buf) {
                    if n == 0 || stream.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
            });
        }
    });
    port
}

fn port_after(text: &str, marker: &str) -> u16 {
    let at = text.find(marker).unwrap_or_else(|| panic!("{marker:?} not in {text:?}")) + marker.len();
    text[at..].chars().take_while(char::is_ascii_digit).collect::<String>().parse().unwrap()
}

fn round_trip(stream: &mut std::net::TcpStream, payload: &[u8]) -> Vec<u8> {
    exchange(stream, payload, payload.len())
}

fn exchange(stream: &mut std::net::TcpStream, payload: &[u8], answer: usize) -> Vec<u8> {
    use std::io::{Read, Write};
    stream.set_read_timeout(Some(std::time::Duration::from_secs(5))).unwrap();
    stream.write_all(payload).unwrap();
    let mut got = vec![0u8; answer];
    stream.read_exact(&mut got).unwrap();
    got
}

#[test]
fn forwards_from_the_config_start_with_the_connection_and_carry_data() {
    let (port, _) = serve();
    let echo = echo_server();
    let dir = temp_ssh_dir();
    std::fs::write(dir.join("config"), format!("Host fw\n  HostName 127.0.0.1\n  Port {port}\n  User ann\n  LocalForward 0 127.0.0.1:{echo}\n  DynamicForward 0\n  RemoteForward 0 127.0.0.1:{echo}\n  LocalForward nonsense\n")).unwrap();
    let mut pane = Pane { conn: start(request("fw".into(), dir)), seen: String::new() };
    pane.wait_for("(yes/no)? ");
    pane.send("yes\r");
    pane.wait_for("password: ");
    pane.send("secret\r");
    pane.wait_for("welcome");
    assert!(pane.seen.contains("Ignored local forward"), "{:?}", pane.seen);

    let local = port_after(&pane.seen, "Forwarding 127.0.0.1:");
    let mut s = std::net::TcpStream::connect(("127.0.0.1", local)).unwrap();
    assert_eq!(round_trip(&mut s, b"through the tunnel"), b"through the tunnel");

    let socks = port_after(&pane.seen, "SOCKS proxy on 127.0.0.1:");
    let mut s = std::net::TcpStream::connect(("127.0.0.1", socks)).unwrap();
    assert_eq!(exchange(&mut s, &[5, 1, 0], 2), [5, 0]);
    let mut connect = vec![5, 1, 0, 1, 127, 0, 0, 1];
    connect.extend(echo.to_be_bytes());
    assert_eq!(round_trip(&mut s, &connect)[1], 0, "the proxy connects");
    assert_eq!(round_trip(&mut s, b"socks"), b"socks");

    let remote = port_after(&pane.seen, "The server listens on 127.0.0.1:");
    let mut s = std::net::TcpStream::connect(("127.0.0.1", remote)).unwrap();
    assert_eq!(round_trip(&mut s, b"from the server side"), b"from the server side");
}
