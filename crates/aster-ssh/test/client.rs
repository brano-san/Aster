use super::*;
use crate::console::Input;
use crate::test_server::*;

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

fn shared(port: u16, dir: PathBuf) -> Pane {
    let mut req = request(format!("ann@127.0.0.1:{port}"), dir);
    req.share = true;
    Pane { conn: start(req), seen: String::new() }
}

#[test]
fn tabs_to_the_same_host_share_one_login_until_the_last_one_closes() {
    let (port, _) = serve();
    let dir = temp_ssh_dir();
    let mut first = shared(port, dir.clone());
    first.wait_for("(yes/no)? ");
    first.send("yes\r");
    first.wait_for("password: ");
    first.send("secret\r");
    first.wait_for("welcome");

    let mut second = shared(port, dir.clone());
    second.wait_for("welcome");
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
    let mut first = connect_to(port, dir.clone());
    first.wait_for("(yes/no)? ");
    first.send("yes\r");
    first.wait_for("password: ");
    first.send("secret\r");
    first.wait_for("welcome");
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
    first.wait_for("system credential store (yes/no)? ");
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
    first.wait_for("credential store (yes/no)? ");
    first.send("no\r");
    first.wait_for("welcome");
    assert_eq!(secrets.get(&crate::secrets::passphrase_key(&key)), None);
    secrets.set(&crate::secrets::passphrase_key(&key), "pw");
    let mut second = open(secrets);
    second.wait_for("welcome");
    assert!(!second.seen.contains("passphrase"));
}
