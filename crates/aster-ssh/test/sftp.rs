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
    loop {
        let reply = pane.conn.files.recv_timeout(std::time::Duration::from_secs(10)).expect("an SFTP reply");
        if !matches!(reply, Reply::Progress { .. }) {
            return reply;
        }
    }
}

#[test]
fn files_are_listed_downloaded_uploaded_and_managed_over_the_same_connection() {
    let remote = temp_ssh_dir();
    std::fs::write(remote.join("a.txt"), "hello").unwrap();
    std::fs::create_dir(remote.join("docs")).unwrap();
    let (port, _) = serve_files(Some(remote.clone()));
    let pane = logged_in(port);

    ask(&pane, Request::List { path: String::new() });
    let Reply::Listing { path, entries } = answer(&pane) else { panic!("no listing") };
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
    ask(&pane, Request::List { path: String::new() });
    assert!(matches!(answer(&pane), Reply::Failed { id: None, error } if error.contains("SFTP")));
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
    assert!(matches!(answer(&pane), Reply::Failed { id: Some(9), .. }));
    std::fs::write(&local, "a=5").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert_eq!(std::fs::read_to_string(remote.join("conf.ini")).unwrap(), "a=4", "nothing is uploaded after editing stops");
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
