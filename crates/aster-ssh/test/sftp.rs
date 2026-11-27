use super::*;
use crate::console::Input;
use crate::test_server::*;

#[test]
fn remote_paths_join_and_split_like_posix() {
    assert_eq!(join("/home/ann", "a.txt"), "/home/ann/a.txt");
    assert_eq!(join("/", "etc"), "/etc");
    assert_eq!(join(".", "x"), "x");
    assert_eq!(parent("/home/ann/"), "/home");
    assert_eq!(parent("/etc"), "/");
    assert_eq!(parent("/"), "/");
    assert_eq!(parent("notes"), ".");
    assert_eq!(file_name("/var/log/syslog"), "syslog");
}

#[test]
fn remote_names_become_safe_local_names() {
    assert_eq!(local_name("/tmp/a:b?.log"), "a_b_.log");
    assert_eq!(local_name("/tmp/trailing. "), "trailing");
    assert_eq!(local_name("/"), "download");
}

#[test]
fn an_existing_local_file_gets_a_numbered_name() {
    let dir = temp_ssh_dir();
    assert_eq!(unique(&dir, "a.txt"), dir.join("a.txt"));
    std::fs::write(dir.join("a.txt"), "1").unwrap();
    std::fs::write(dir.join("a (1).txt"), "2").unwrap();
    assert_eq!(unique(&dir, "a.txt"), dir.join("a (2).txt"));
    std::fs::write(dir.join("Makefile"), "").unwrap();
    assert_eq!(unique(&dir, "Makefile"), dir.join("Makefile (1)"));
    std::fs::write(dir.join(".bashrc"), "").unwrap();
    assert_eq!(unique(&dir, ".bashrc"), dir.join(".bashrc (1)"));
}

#[test]
fn folders_come_first_then_names_ignoring_case() {
    let e = |name: &str, dir: bool| Entry { name: name.into(), dir, link: false, size: 0, modified: None, mode: None };
    let mut list = vec![e("b.txt", false), e("Zed", true), e("A.txt", false), e("apps", true)];
    sort(&mut list);
    let names: Vec<_> = list.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["apps", "Zed", "A.txt", "b.txt"]);
}

fn logged_in(port: u16) -> Pane {
    let mut pane = connect_to(port, temp_ssh_dir());
    pane.wait_for("(yes/no)? ");
    pane.send("yes\r");
    pane.wait_for("password: ");
    pane.send("secret\r");
    pane.wait_for("welcome");
    pane
}

fn ask(pane: &Pane, request: Request) {
    pane.conn.input.send(Input::Sftp(request)).unwrap();
}

fn answer(pane: &Pane) -> Reply {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let reply = pane.conn.files.recv_deadline(deadline).expect("an SFTP reply within 30 seconds");
        if !matches!(reply, Reply::Progress { .. }) {
            return reply;
        }
    }
}

#[test]
fn opening_a_link_follows_the_target_type() {
    use russh_sftp::protocol::*;
    struct Links(Fs);
    impl russh_sftp::server::Handler for Links {
        type Error = StatusCode;
        fn unimplemented(&self) -> Self::Error { StatusCode::OpUnsupported }
        async fn stat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
            self.0.stat(id, path).await
        }
        async fn lstat(&mut self, id: u32, _: String) -> Result<Attrs, Self::Error> {
            let mut attrs = FileAttributes::empty();
            attrs.permissions = Some(0o120777);
            Ok(Attrs { id, attrs })
        }
        async fn realpath(&mut self, id: u32, path: String) -> Result<Name, Self::Error> {
            self.0.realpath(id, path).await
        }
        async fn opendir(&mut self, id: u32, path: String) -> Result<Handle, Self::Error> {
            self.0.opendir(id, path).await
        }
        async fn readdir(&mut self, id: u32, handle: String) -> Result<Name, Self::Error> {
            self.0.readdir(id, handle).await
        }
        async fn close(&mut self, id: u32, handle: String) -> Result<Status, Self::Error> {
            self.0.close(id, handle).await
        }
    }

    let root = temp_ssh_dir();
    std::fs::create_dir(root.join("folder")).unwrap();
    std::fs::write(root.join("folder/child"), b"hello").unwrap();
    std::fs::write(root.join("file"), b"hello").unwrap();
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let (client, server) = tokio::io::duplex(65_536);
        let task = tokio::spawn(russh_sftp::server::run(server, Links(Fs::new(root.clone()))));
        let files = Files::new(SftpSession::new(client).await.unwrap(), crossbeam_channel::unbounded().0);
        let cancel = Notify::new();
        let request = Request::OpenLink { id: 20, path: "/folder".into() };
        assert!(matches!(files.run(&request, &cancel).await, Ok(Reply::Listing { id: 20, path, entries }) if path == "/folder" && entries[0].name == "child"));
        let request = Request::OpenLink { id: 21, path: "/file".into() };
        assert_eq!(files.run(&request, &cancel).await.unwrap(), Reply::LinkFile { id: 21, path: "/file".into() });
        let request = Request::OpenLink { id: 22, path: "/broken".into() };
        assert!(files.run(&request, &cancel).await.is_err());
        task.abort();
    });
}

#[test]
fn cancelling_a_directory_download_removes_the_partial_copy() {
    let root = temp_ssh_dir();
    std::fs::create_dir(root.join("folder")).unwrap();
    std::fs::write(root.join("folder/large"), vec![1; 2_000_000]).unwrap();
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let (client, server) = tokio::io::duplex(65_536);
        let task = tokio::spawn(russh_sftp::server::run(server, Fs::new(root.clone())));
        let session = SftpSession::new(client).await.unwrap();
        let target = root.join("local");
        let cancel = Notify::new();
        let mut progress = |done, _| {
            assert!(done > 0);
            cancel.notify_one();
        };
        let result = crate::sftp_tree::download_with_cancel(&session, "/folder", &target, &mut progress, Some(&cancel)).await;
        assert_eq!(result.unwrap_err(), "cancelled");
        assert!(!target.join("folder").exists());
        task.abort();
    });
}

#[test]
fn simultaneous_uploads_with_the_same_name_never_overwrite_the_winner() {
    let root = temp_ssh_dir();
    let left = root.join("left/file");
    let right = root.join("right/file");
    let remote = root.join("remote");
    for path in [&left, &right] { std::fs::create_dir(path.parent().unwrap()).unwrap(); }
    std::fs::create_dir(&remote).unwrap();
    std::fs::write(&left, b"left").unwrap();
    std::fs::write(&right, b"right").unwrap();
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let (client, server) = tokio::io::duplex(65_536);
        let task = tokio::spawn(russh_sftp::server::run(server, Fs::new(remote.clone())));
        let files = Files::new(SftpSession::new(client).await.unwrap(), crossbeam_channel::unbounded().0);
        let cancel = Notify::new();
        let (first, second) = tokio::join!(files.upload(1, &left, "/", &cancel), files.upload(2, &right, "/", &cancel));
        assert_ne!(first.is_ok(), second.is_ok());
        let expected = if first.is_ok() { b"left".as_slice() } else { b"right".as_slice() };
        assert_eq!(std::fs::read(remote.join("file")).unwrap(), expected);
        task.abort();
    });
}

#[test]
fn files_are_listed_downloaded_uploaded_and_managed_over_the_same_connection() {
    let remote = temp_ssh_dir();
    std::fs::write(remote.join("a.txt"), "hello").unwrap();
    std::fs::create_dir(remote.join("docs")).unwrap();
    let (port, _) = serve_files(Some(remote.clone()));
    let pane = logged_in(port);

    ask(&pane, Request::List { id: 17, paths: vec!["/missing".into(), String::new()] });
    let Reply::Listing { id, path, entries } = answer(&pane) else { panic!("no listing") };
    assert_eq!(id, 17);
    assert_eq!(path, "/");
    let names: Vec<_> = entries.iter().map(|e| (e.name.as_str(), e.dir)).collect();
    assert_eq!(names, [("docs", true), ("a.txt", false)]);
    assert_eq!(entries[1].size, 5);
    assert!(entries[1].modified.is_some());

    let downloads = temp_ssh_dir();
    ask(&pane, Request::Download { id: 1, remote: "/a.txt".into(), dir: downloads.clone() });
    assert_eq!(answer(&pane), Reply::Downloaded { id: 1, local: downloads.join("a.txt") });
    ask(&pane, Request::Download { id: 2, remote: "/a.txt".into(), dir: downloads.clone() });
    assert_eq!(answer(&pane), Reply::Downloaded { id: 2, local: downloads.join("a (1).txt") });
    assert_eq!(std::fs::read_to_string(downloads.join("a (1).txt")).unwrap(), "hello");

    let local = downloads.join("big.bin");
    let body: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&local, &body).unwrap();
    ask(&pane, Request::Upload { id: 3, local: local.clone(), dir: "/docs".into() });
    let mut progressed = false;
    let done = loop {
        match pane.conn.files.recv_timeout(std::time::Duration::from_secs(10)).unwrap() {
            Reply::Progress { id: 3, done, total } => {
                assert_eq!(total, body.len() as u64);
                progressed |= done > 0;
            }
            other => break other,
        }
    };
    assert_eq!(done, Reply::Uploaded { id: 3, remote: "/docs/big.bin".into() });
    assert!(progressed);
    assert_eq!(std::fs::read(remote.join("docs").join("big.bin")).unwrap(), body);
    ask(&pane, Request::Upload { id: 4, local, dir: "/docs".into() });
    assert!(matches!(answer(&pane), Reply::Failed { id: Some(4), error } if error.contains("already exists")));

    ask(&pane, Request::Mkdir { path: "/new".into() });
    assert_eq!(answer(&pane), Reply::Changed { dir: "/".into() });
    ask(&pane, Request::Rename { from: "/new".into(), to: "/renamed".into() });
    assert_eq!(answer(&pane), Reply::Changed { dir: "/".into() });
    assert!(remote.join("renamed").is_dir());
    ask(&pane, Request::Remove { path: "/renamed".into(), dir: true });
    assert_eq!(answer(&pane), Reply::Changed { dir: "/".into() });
    ask(&pane, Request::Remove { path: "/a.txt".into(), dir: false });
    assert_eq!(answer(&pane), Reply::Changed { dir: "/".into() });
    assert!(!remote.join("a.txt").exists());

    ask(&pane, Request::Download { id: 5, remote: "/missing".into(), dir: downloads.clone() });
    assert!(matches!(answer(&pane), Reply::Failed { id: Some(5), .. }));
    assert!(!downloads.join("missing").exists(), "a failed download leaves nothing behind");
    pane.send("exit\r");
    assert_eq!(pane.exit(), 7);
}

#[test]
fn a_server_without_sftp_reports_it_once_asked() {
    let (port, _) = serve();
    let pane = logged_in(port);
    ask(&pane, Request::List { id: 11, paths: vec![String::new()] });
    assert!(matches!(answer(&pane), Reply::NavigationFailed { id: 11, error } if error.contains("SFTP")));
}

#[test]
fn a_file_edited_locally_is_uploaded_after_each_save_until_editing_stops() {
    let remote = temp_ssh_dir();
    std::fs::write(remote.join("conf.ini"), "a=1").unwrap();
    let (port, _) = serve_files(Some(remote.clone()));
    let pane = logged_in(port);
    let scratch = temp_ssh_dir();
    ask(&pane, Request::Edit { id: 9, remote: "/conf.ini".into(), dir: scratch.clone() });
    let Reply::Editing { id: 9, local } = answer(&pane) else { panic!("no editing reply") };
    assert_eq!(std::fs::read_to_string(&local).unwrap(), "a=1");
    assert!(local.starts_with(&scratch));

    std::fs::write(&local, "a=2\nb=3").unwrap();
    assert_eq!(answer(&pane), Reply::Saved { id: 9, remote: "/conf.ini".into() });
    assert_eq!(std::fs::read_to_string(remote.join("conf.ini")).unwrap(), "a=2\nb=3");

    std::fs::write(&local, "a=4").unwrap();
    assert_eq!(answer(&pane), Reply::Saved { id: 9, remote: "/conf.ini".into() });
    assert_eq!(std::fs::read_to_string(remote.join("conf.ini")).unwrap(), "a=4");

    ask(&pane, Request::Cancel { id: 9 });
    assert_eq!(answer(&pane), Reply::Failed { id: Some(9), error: "cancelled".into() });
    std::fs::write(&local, "a=5").unwrap();
    assert_eq!(std::fs::read_to_string(remote.join("conf.ini")).unwrap(), "a=4", "nothing is uploaded after editing stops");
}

#[test]
fn edits_with_the_same_size_and_modification_time_are_uploaded() {
    let remote = temp_ssh_dir();
    let original = remote.join("conf.ini");
    std::fs::write(&original, "a=1").unwrap();
    let (port, _) = serve_files(Some(remote));
    let pane = logged_in(port);
    ask(&pane, Request::Edit { id: 1, remote: "/conf.ini".into(), dir: temp_ssh_dir() });
    let Reply::Editing { id: 1, local } = answer(&pane) else { panic!("no editing reply") };
    let modified = std::fs::metadata(&local).unwrap().modified().unwrap();
    let replacement = local.with_extension("tmp");
    for body in ["a=2", "a=3"] {
        std::fs::write(&replacement, body).unwrap();
        std::fs::File::options().write(true).open(&replacement).unwrap().set_modified(modified).unwrap();
        std::fs::rename(&replacement, &local).unwrap();
        assert_eq!(std::fs::metadata(&local).unwrap().modified().unwrap(), modified);
        assert_eq!(answer(&pane), Reply::Saved { id: 1, remote: "/conf.ini".into() });
        assert_eq!(std::fs::read_to_string(&original).unwrap(), body);
    }
    ask(&pane, Request::Cancel { id: 1 });
    assert_eq!(answer(&pane), Reply::Failed { id: Some(1), error: "cancelled".into() });
}

#[test]
fn folders_download_upload_and_delete_with_their_contents() {
    let remote = temp_ssh_dir();
    std::fs::create_dir_all(remote.join("site").join("css")).unwrap();
    std::fs::write(remote.join("site").join("index.html"), "<h1>hi</h1>").unwrap();
    std::fs::write(remote.join("site").join("css").join("a.css"), "body{}").unwrap();
    let (port, _) = serve_files(Some(remote.clone()));
    let pane = logged_in(port);
    let downloads = temp_ssh_dir();
    ask(&pane, Request::Download { id: 1, remote: "/site".into(), dir: downloads.clone() });
    assert_eq!(answer(&pane), Reply::Downloaded { id: 1, local: downloads.join("site") });
    assert_eq!(std::fs::read_to_string(downloads.join("site").join("css").join("a.css")).unwrap(), "body{}");

    ask(&pane, Request::Upload { id: 2, local: downloads.join("site"), dir: "/backup".into() });
    assert!(matches!(answer(&pane), Reply::Failed { id: Some(2), .. }), "the target folder must exist");
    std::fs::create_dir(remote.join("backup")).unwrap();
    ask(&pane, Request::Upload { id: 3, local: downloads.join("site"), dir: "/backup".into() });
    assert_eq!(answer(&pane), Reply::Uploaded { id: 3, remote: "/backup/site".into() });
    assert_eq!(std::fs::read_to_string(remote.join("backup").join("site").join("index.html")).unwrap(), "<h1>hi</h1>");

    ask(&pane, Request::Remove { path: "/site".into(), dir: true });
    assert_eq!(answer(&pane), Reply::Changed { dir: "/".into() });
    assert!(!remote.join("site").exists(), "a non-empty folder is removed with its contents");

    ask(&pane, Request::Chmod { path: "/backup".into(), mode: 0o700, recursive: true });
    assert_eq!(answer(&pane), Reply::Changed { dir: "/".into() });
}
