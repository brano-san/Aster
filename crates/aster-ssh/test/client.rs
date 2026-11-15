use super::*;
use russh::server::{self, Auth, Msg, Session};
use russh::{Channel, ChannelId};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

const HOST_KEY: &str = include_str!("keys/host");
const CLIENT_KEY: &str = include_str!("keys/client");
const CLIENT_PUB: &str = include_str!("keys/client.pub");

#[derive(Clone, Default)]
struct Seen {
    sizes: Arc<std::sync::Mutex<Vec<(u32, u32)>>>,
    tunnels: Arc<std::sync::Mutex<Vec<String>>>,
}

struct Server {
    seen: Seen,
    client_key: PublicKey,
    sessions: Vec<ChannelId>,
}

impl server::Handler for Server {
    type Error = russh::Error;

    async fn auth_password(&mut self, user: &str, password: &str) -> Result<Auth, Self::Error> {
        Ok(if user == "ann" && password == "secret" { Auth::Accept } else { Auth::reject() })
    }

    async fn auth_publickey(&mut self, user: &str, key: server::VerifiedPublicKey<'_>) -> Result<Auth, Self::Error> {
        Ok(if user == "ann" && key.public_key.key_data() == self.client_key.key_data() { Auth::Accept } else { Auth::reject() })
    }

    async fn channel_open_session(&mut self, channel: Channel<Msg>, reply: server::ChannelOpenHandle, _session: &mut Session) -> Result<(), Self::Error> {
        self.sessions.push(channel.id());
        reply.accept().await;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn pty_request(&mut self, _c: ChannelId, _term: &str, cols: u32, rows: u32, _pw: u32, _ph: u32, _modes: &[(russh::Pty, u32)], _s: &mut Session) -> Result<(), Self::Error> {
        self.seen.sizes.lock().unwrap().push((cols, rows));
        Ok(())
    }

    async fn window_change_request(&mut self, _c: ChannelId, cols: u32, rows: u32, _pw: u32, _ph: u32, _s: &mut Session) -> Result<(), Self::Error> {
        self.seen.sizes.lock().unwrap().push((cols, rows));
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn channel_open_direct_tcpip(&mut self, channel: Channel<Msg>, host: &str, port: u32, _oa: &str, _op: u32, reply: server::ChannelOpenHandle, _s: &mut Session) -> Result<(), Self::Error> {
        self.seen.tunnels.lock().unwrap().push(format!("{host}:{port}"));
        let target = tokio::net::TcpStream::connect((host.to_owned(), port as u16)).await;
        let Ok(mut target) = target else { return Ok(()) };
        reply.accept().await;
        tokio::spawn(async move {
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut target).await;
        });
        Ok(())
    }

    async fn shell_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        session.data(channel, &b"welcome\r\n"[..])?;
        Ok(())
    }

    async fn data(&mut self, channel: ChannelId, data: &[u8], session: &mut Session) -> Result<(), Self::Error> {
        if !self.sessions.contains(&channel) {
            return Ok(());
        }
        if data == b"exit\r" {
            session.exit_status_request(channel, 7)?;
            session.close(channel)?;
        } else {
            session.data(channel, format!("got:{}\r\n", String::from_utf8_lossy(data).trim_end()).into_bytes())?;
        }
        Ok(())
    }
}

fn serve() -> (u16, Seen) {
    let seen = Seen::default();
    let shared = seen.clone();
    let (port_tx, port_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            port_tx.send(listener.local_addr().unwrap().port()).unwrap();
            let config = Arc::new(server::Config {
                keys: vec![russh::keys::PrivateKey::from_openssh(HOST_KEY).unwrap()],
                auth_rejection_time: Duration::from_millis(1),
                auth_rejection_time_initial: Some(Duration::ZERO),
                ..Default::default()
            });
            let client_key = PublicKey::from_openssh(CLIENT_PUB.trim()).unwrap();
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let handler = Server { seen: shared.clone(), client_key: client_key.clone(), sessions: Vec::new() };
                let config = config.clone();
                tokio::spawn(async move {
                    if let Ok(running) = server::run_stream(config, socket, handler).await {
                        let _ = running.await;
                    }
                });
            }
        });
    });
    (port_rx.recv().unwrap(), seen)
}

fn temp_ssh_dir() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!("aster-ssh-test-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn request(destination: String, ssh_dir: PathBuf) -> Request {
    Request { destination, cols: 90, rows: 25, term: "xterm-256color".into(), home: ssh_dir.clone(), ssh_dir, user: "ann".into(), use_agent: false }
}

struct Pane {
    conn: Connection,
    seen: String,
}

impl Pane {
    fn wait_for(&mut self, text: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.seen.contains(text) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.conn.output.recv_timeout(left) {
                Ok(bytes) => self.seen.push_str(&String::from_utf8_lossy(&bytes)),
                Err(_) => panic!("{text:?} never appeared in {:?}", self.seen),
            }
        }
    }

    fn send(&self, text: &str) {
        self.conn.input.send(Input::Data(text.as_bytes().to_vec())).unwrap();
    }

    fn exit(&self) -> u32 {
        self.conn.exit.recv_timeout(Duration::from_secs(10)).unwrap()
    }
}

fn connect_to(port: u16, ssh_dir: PathBuf) -> Pane {
    Pane { conn: start(request(format!("ann@127.0.0.1:{port}"), ssh_dir)), seen: String::new() }
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
