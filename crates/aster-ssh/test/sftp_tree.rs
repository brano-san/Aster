use super::*;
use crate::test_server::{serve_files, sftp_session, temp_ssh_dir};

fn run(f: impl AsyncFnOnce(PathBuf, SftpSession)) {
    let dir = temp_ssh_dir();
    std::fs::create_dir_all(dir.join("tree/sub")).unwrap();
    std::fs::write(dir.join("tree/a"), b"hello").unwrap();
    std::fs::write(dir.join("tree/sub/b"), b"world!").unwrap();
    let (port, _) = serve_files(Some(dir.clone()));
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        f(dir.clone(), sftp_session(port).await).await;
    });
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn folder_size_counts_all_entries() { run(async |_, s| { assert_eq!(size(&s, "/tree").await.unwrap(), Size { files: 2, dirs: 2, bytes: 11 }); }); }
#[test]
fn file_size_counts_one_file() { run(async |_, s| { assert_eq!(size(&s, "/tree/a").await.unwrap(), Size { files: 1, dirs: 0, bytes: 5 }); }); }
#[test]
fn local_size_matches_remote_size() { run(async |dir, s| { assert_eq!(local_size(&dir.join("tree")).await.unwrap(), size(&s, "/tree").await.unwrap()); }); }
#[test]
fn download_preserves_tree_and_reports_progress() { run(async |dir, s| {
    let mut progress = Vec::new();
    let root = download(&s, "/tree", &dir.join("out"), &mut |d, t| progress.push((d, t))).await.unwrap();
    assert_eq!(std::fs::read(root.join("a")).unwrap(), b"hello");
    assert_eq!(std::fs::read(root.join("sub/b")).unwrap(), b"world!");
    assert_eq!(progress.last(), Some(&(11, 11)));
}); }
#[test]
fn duplicate_download_uses_unique_root() { run(async |dir, s| {
    let out = dir.join("out");
    download(&s, "/tree", &out, &mut |_, _| {}).await.unwrap();
    assert_eq!(download(&s, "/tree", &out, &mut |_, _| {}).await.unwrap(), out.join("tree (1)"));
}); }
#[test]
fn download_copies_single_file() { run(async |dir, s| {
    let root = download(&s, "/tree/a", &dir.join("out"), &mut |_, _| {}).await.unwrap();
    assert_eq!(std::fs::read(root).unwrap(), b"hello");
}); }
#[test]
fn missing_download_creates_nothing() { run(async |dir, s| {
    let out = dir.join("out");
    assert!(download(&s, "/absent", &out, &mut |_, _| {}).await.unwrap_err().contains("/absent"));
    assert!(!out.exists());
}); }
#[test]
fn upload_preserves_tree_and_progress() { run(async |dir, s| {
    std::fs::create_dir(dir.join("out")).unwrap();
    let mut progress = Vec::new();
    assert_eq!(upload(&s, &dir.join("tree"), "/out", &mut |d, t| progress.push((d, t))).await.unwrap(), "/out/tree");
    assert_eq!(std::fs::read(dir.join("out/tree/sub/b")).unwrap(), b"world!");
    assert_eq!(progress.last(), Some(&(11, 11)));
}); }
#[test]
fn upload_refuses_existing_root() { run(async |dir, s| {
    assert!(upload(&s, &dir.join("tree"), "/", &mut |_, _| {}).await.unwrap_err().contains("already exists"));
    assert_eq!(std::fs::read(dir.join("tree/a")).unwrap(), b"hello");
}); }
#[test]
fn upload_copies_single_file() { run(async |dir, s| {
    assert_eq!(upload(&s, &dir.join("tree/a"), "/", &mut |_, _| {}).await.unwrap(), "/a");
    assert_eq!(std::fs::read(dir.join("a")).unwrap(), b"hello");
}); }
#[test]
fn remove_deletes_tree_and_counts_entries() { run(async |dir, s| {
    assert_eq!(remove(&s, "/tree").await.unwrap(), Size { files: 2, dirs: 2, bytes: 11 });
    assert!(!dir.join("tree").exists());
}); }
#[test]
fn recursive_chmod_counts_all_objects() { run(async |dir, s| {
    assert_eq!(chmod(&s, "/tree", 0o750, true).await.unwrap(), 4);
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; assert_eq!(std::fs::metadata(dir.join("tree/a")).unwrap().permissions().mode() & 0o7777, 0o750); }
    #[cfg(not(unix))] let _ = dir;
}); }
#[test]
fn nonrecursive_chmod_changes_only_root() { run(async |_, s| { assert_eq!(chmod(&s, "/tree", 0o700, false).await.unwrap(), 1); }); }

#[test]
fn failed_download_removes_partial_tree() { run(async |dir, s| {
    let out = dir.join("out");
    let mut removed = false;
    let error = download(&s, "/tree", &out, &mut |done, _| {
        if !removed {
            let other = if done == 5 { "tree/sub/b" } else { "tree/a" };
            std::fs::remove_file(dir.join(other)).unwrap();
            removed = true;
        }
    }).await.unwrap_err();
    assert!(removed);
    assert!(error.contains("/tree/"));
    assert_eq!(std::fs::read_dir(out).unwrap().count(), 0);
}); }

#[test]
fn failed_upload_removes_partial_tree() { run(async |dir, s| {
    std::fs::create_dir(dir.join("out")).unwrap();
    let mut removed = false;
    let error = upload(&s, &dir.join("tree"), "/out", &mut |done, _| {
        if !removed {
            let other = if done == 5 { "tree/sub/b" } else { "tree/a" };
            std::fs::remove_file(dir.join(other)).unwrap();
            removed = true;
        }
    }).await.unwrap_err();
    assert!(removed);
    assert!(!error.is_empty());
    assert!(!dir.join("out/tree").exists());
}); }

#[test]
fn download_accepts_trailing_slash() { run(async |dir, s| {
    let root = download(&s, "/tree/", &dir.join("out"), &mut |_, _| {}).await.unwrap();
    assert_eq!(std::fs::read(root.join("a")).unwrap(), b"hello");
}); }

#[test]
fn download_accepts_remote_root() { run(async |dir, s| {
    let out = temp_ssh_dir();
    let root = download(&s, "/", &out, &mut |_, _| {}).await.unwrap();
    assert_eq!(std::fs::read(root.join("tree/a")).unwrap(), b"hello");
    assert!(dir.join("tree").exists());
    std::fs::remove_dir_all(out).unwrap();
}); }

#[test]
fn download_accepts_current_directory() { run(async |_, s| {
    let out = temp_ssh_dir();
    let result = download(&s, ".", &out, &mut |_, _| {}).await;
    if let Ok(root) = &result {
        assert_eq!(std::fs::read(root.join("tree/a")).unwrap(), b"hello");
        assert_eq!(std::fs::read(root.join("tree/sub/b")).unwrap(), b"world!");
    }
    std::fs::remove_dir_all(out).unwrap();
    result.unwrap();
}); }

#[cfg(unix)]
#[test]
fn recursive_chmod_restricts_children_before_parents() { run(async |dir, s| {
    use std::os::unix::fs::PermissionsExt;
    for mode in [0o600, 0] {
        let result = chmod(&s, "/tree", mode, true).await;
        let mut actual = Vec::new();
        for path in ["tree", "tree/sub", "tree/a", "tree/sub/b"] {
            let path = dir.join(path);
            actual.push(std::fs::metadata(&path).unwrap().permissions().mode() & 0o7777);
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(result.unwrap(), 4);
        assert_eq!(actual, vec![mode; 4]);
    }
}); }

#[test]
fn large_files_transfer_in_chunks_with_exact_bytes() { run(async |dir, s| {
    let bytes: Vec<u8> = (0..150_000).map(|n| n as u8).collect();
    std::fs::write(dir.join("large"), &bytes).unwrap();
    let mut progress = Vec::new();
    let downloaded = download(&s, "/large", &dir.join("out"), &mut |d, t| progress.push((d, t))).await.unwrap();
    assert_eq!(std::fs::read(&downloaded).unwrap(), bytes);
    assert!(progress.len() >= 3);
    assert_eq!(progress.last(), Some(&(150_000, 150_000)));
    std::fs::create_dir(dir.join("uploaded")).unwrap();
    progress.clear();
    upload(&s, &downloaded, "/uploaded", &mut |d, t| progress.push((d, t))).await.unwrap();
    assert_eq!(std::fs::read(dir.join("uploaded/large")).unwrap(), bytes);
    assert!(progress.len() >= 3);
    assert_eq!(progress.last(), Some(&(150_000, 150_000)));
}); }

#[cfg(unix)]
#[test]
fn local_links_are_not_counted_or_uploaded() { run(async |dir, s| {
    std::os::unix::fs::symlink(dir.join("tree"), dir.join("tree/sub/loop")).unwrap();
    assert_eq!(local_size(&dir.join("tree")).await.unwrap(), Size { files: 2, dirs: 2, bytes: 11 });
    std::fs::create_dir(dir.join("out")).unwrap();
    upload(&s, &dir.join("tree"), "/out", &mut |_, _| {}).await.unwrap();
    assert!(!dir.join("out/tree/sub/loop").exists());
}); }

#[cfg(unix)]
#[test]
fn remote_links_are_skipped_and_removed_without_touching_targets() {
    use russh_sftp::protocol::*;
    use std::os::unix::fs::PermissionsExt;
    struct Links { root: PathBuf, fs: crate::test_server::Fs }
    macro_rules! delegate {
        ($name:ident, $result:ty, $( $arg:ident: $ty:ty ),*) => {
            async fn $name(&mut self, $( $arg: $ty ),*) -> Result<$result, Self::Error> {
                russh_sftp::server::Handler::$name(&mut self.fs, $( $arg ),*).await
            }
        };
    }
    impl russh_sftp::server::Handler for Links {
        type Error = StatusCode;
        fn unimplemented(&self) -> Self::Error { StatusCode::OpUnsupported }
        async fn lstat(&mut self, id: u32, path: String) -> Result<Attrs, Self::Error> {
            let meta = std::fs::symlink_metadata(self.root.join(path.trim_start_matches('/'))).map_err(|_| StatusCode::NoSuchFile)?;
            let mut attrs = FileAttributes::empty();
            attrs.permissions = Some(meta.permissions().mode());
            attrs.size = Some(meta.len());
            Ok(Attrs { id, attrs })
        }
        delegate!(opendir, Handle, id: u32, path: String);
        delegate!(readdir, Name, id: u32, handle: String);
        delegate!(open, Handle, id: u32, filename: String, pflags: OpenFlags, attrs: FileAttributes);
        delegate!(read, Data, id: u32, handle: String, offset: u64, len: u32);
        delegate!(close, Status, id: u32, handle: String);
        delegate!(remove, Status, id: u32, filename: String);
        delegate!(rmdir, Status, id: u32, path: String);
    }
    let root = temp_ssh_dir();
    std::fs::create_dir(root.join("tree")).unwrap();
    std::fs::create_dir(root.join("target")).unwrap();
    std::fs::write(root.join("tree/a"), b"hello").unwrap();
    std::fs::write(root.join("target/keep"), b"safe").unwrap();
    std::os::unix::fs::symlink(root.join("target"), root.join("tree/link")).unwrap();
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        let (client, server) = tokio::io::duplex(65536);
        let task = tokio::spawn(russh_sftp::server::run(server, Links { root: root.clone(), fs: crate::test_server::Fs::new(root.clone()) }));
        let s = SftpSession::new(client).await.unwrap();
        assert_eq!(size(&s, "/tree").await.unwrap(), Size { files: 1, dirs: 1, bytes: 5 });
        let downloaded = download(&s, "/tree", &root.join("out"), &mut |_, _| {}).await.unwrap();
        assert!(!downloaded.join("link").exists());
        assert_eq!(std::fs::read(downloaded.join("a")).unwrap(), b"hello");
        assert_eq!(remove(&s, "/tree").await.unwrap(), Size { files: 2, dirs: 1, bytes: 5 });
        assert_eq!(std::fs::read(root.join("target/keep")).unwrap(), b"safe");
        task.abort();
    });
    std::fs::remove_dir_all(root).unwrap();
}
