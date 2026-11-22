use super::*;

#[test]
fn file_dates_follow_the_interface_language() {
    crate::i18n::set_language("en");
    assert_eq!(date(1_709_164_800), "2024-02-29");
    crate::i18n::set_language("ru");
    assert_eq!(date(1_709_164_800), "29.02.2024");
    crate::i18n::set_language("en");
}
use crate::config::Config;
use crate::input::{Event, MouseButton};
use crate::keymap::Action;
use crate::profile::Profile;
use std::sync::Arc;

fn ssh_app() -> (App, PaneId) {
    let mut profile = Profile::new("ssh:box", "SSH: box", "ssh", &["box"]);
    profile.ssh = Some("box".into());
    let mut a = App::new(Config::default(), vec![profile], true, None, Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    a.open_default_tab();
    a.chrome = a.tab_chrome();
    let pane = a.focused_pane().unwrap();
    a.take_effects();
    (a, pane)
}

fn requests(a: &mut App) -> Vec<FileRequest> {
    a.take_effects().into_iter().filter_map(|e| if let Effect::Files { request, .. } = e { Some(request) } else { None }).collect()
}

fn entry(name: &str, dir: bool, size: u64) -> FileEntry {
    FileEntry { name: name.into(), dir, link: false, size, modified: Some(1_763_164_800), mode: Some(if dir { 0o40755 } else { 0o100644 }) }
}

fn key(a: &mut App, k: Key) {
    a.on_event(Event::Key { key: k, mods: Mods::NONE, pressed: true });
}

fn listed(a: &mut App, pane: PaneId, path: &str) {
    a.files_reply(pane, FileReply::Listing { path: path.into(), entries: vec![entry("docs", true, 0), entry("notes.txt", false, 2048), entry("photo.jpg", false, 3_500_000)] });
}

#[test]
fn opening_lists_the_home_folder_and_enter_walks_into_folders() {
    let (mut a, pane) = ssh_app();
    a.run(Action::OpenFiles);
    assert_eq!(requests(&mut a), [FileRequest::List(String::new())]);
    listed(&mut a, pane, "/home/ann");
    let page = a.files_page().unwrap();
    assert_eq!(page.rows(), 4, "an up row comes first below the root");
    key(&mut a, Key::Down);
    key(&mut a, Key::Enter);
    assert_eq!(requests(&mut a), [FileRequest::List("/home/ann/docs".into())]);
    key(&mut a, Key::Backspace);
    assert_eq!(requests(&mut a), [FileRequest::List("/home".into())]);
}

#[test]
fn enter_on_a_file_downloads_it_and_progress_updates_the_transfer() {
    let (mut a, pane) = ssh_app();
    a.run(Action::OpenFiles);
    listed(&mut a, pane, "/srv");
    a.take_effects();
    for _ in 0..2 {
        key(&mut a, Key::Down);
    }
    key(&mut a, Key::Enter);
    let reqs = requests(&mut a);
    let FileRequest::Download { id, remote } = &reqs[0] else { panic!("{reqs:?}") };
    assert_eq!(remote, "/srv/notes.txt");
    a.files_reply(pane, FileReply::Progress { id: *id, done: 1024, total: 2048 });
    assert_eq!(a.transfers_of(pane)[0].done, 1024);
    a.files_reply(pane, FileReply::Downloaded { id: *id, local: "C:/Users/a/Downloads/notes.txt".into() });
    assert!(matches!(a.transfers_of(pane)[0].state, TransferState::Done(_)));
}

#[test]
fn typing_filters_and_escape_clears_before_closing() {
    let (mut a, pane) = ssh_app();
    a.run(Action::OpenFiles);
    listed(&mut a, pane, "/srv");
    for c in "PHO".chars() {
        a.on_event(Event::Char(c));
    }
    let page = a.files_page().unwrap();
    assert_eq!((page.rows(), page.entry(0).map(|e| e.name.as_str())), (1, Some("photo.jpg")));
    key(&mut a, Key::Escape);
    assert_eq!(a.files_page().unwrap().rows(), 4);
    key(&mut a, Key::Escape);
    assert!(!a.files_open());
}

#[test]
fn prompts_create_rename_and_delete_in_the_current_folder() {
    let (mut a, pane) = ssh_app();
    a.run(Action::OpenFiles);
    listed(&mut a, pane, "/srv");
    a.take_effects();
    key(&mut a, Key::F(7));
    for c in "logs".chars() {
        a.on_event(Event::Char(c));
    }
    key(&mut a, Key::Enter);
    assert_eq!(requests(&mut a), [FileRequest::Mkdir("/srv/logs".into())]);
    key(&mut a, Key::Down);
    key(&mut a, Key::Down);
    key(&mut a, Key::F(2));
    key(&mut a, Key::Backspace);
    key(&mut a, Key::Backspace);
    key(&mut a, Key::Backspace);
    for c in "md".chars() {
        a.on_event(Event::Char(c));
    }
    key(&mut a, Key::Enter);
    assert_eq!(requests(&mut a), [FileRequest::Rename { from: "/srv/notes.txt".into(), to: "/srv/notes.md".into() }]);
    key(&mut a, Key::Delete);
    key(&mut a, Key::Escape);
    assert!(requests(&mut a).is_empty(), "escape keeps the file");
    key(&mut a, Key::Delete);
    key(&mut a, Key::Enter);
    assert_eq!(requests(&mut a), [FileRequest::Remove { path: "/srv/notes.txt".into(), dir: false }]);
    a.files_reply(pane, FileReply::Changed { dir: "/srv".into() });
    assert_eq!(requests(&mut a), [FileRequest::List("/srv".into())]);
}

#[test]
fn dropped_files_upload_into_the_open_folder() {
    let (mut a, pane) = ssh_app();
    a.run(Action::OpenFiles);
    listed(&mut a, pane, "/srv");
    a.take_effects();
    a.on_event(Event::DroppedFiles(vec![r"C:\data\a.zip".into()]));
    let reqs = requests(&mut a);
    assert!(matches!(&reqs[0], FileRequest::Upload { local, dir, .. } if dir == "/srv" && local == &PathBuf::from(r"C:\data\a.zip")));
    let FileRequest::Upload { id, .. } = reqs[0] else { unreachable!() };
    a.files_reply(pane, FileReply::Uploaded { id, remote: "/srv/a.zip".into() });
    assert_eq!(requests(&mut a), [FileRequest::List("/srv".into())], "the folder refreshes after an upload");
}

#[test]
fn clicking_a_running_transfer_cancels_it() {
    let (mut a, pane) = ssh_app();
    a.run(Action::OpenFiles);
    listed(&mut a, pane, "/srv");
    a.take_effects();
    a.on_event(Event::DroppedFiles(vec!["/tmp/x.bin".into()]));
    a.take_effects();
    let (id, r) = a.files_geometry().unwrap().transfers[0];
    a.on_event(Event::MouseDown { x: r.x + 5.0, y: r.y + 5.0, button: MouseButton::Left, clicks: 1 });
    assert_eq!(requests(&mut a), [FileRequest::Cancel(id)]);
}

#[test]
fn local_panes_cannot_open_the_file_panel() {
    let mut a = App::new(Config::default(), vec![Profile::new("local:a", "A", "cmd.exe", &[])], true, None, Arc::new(|| {}));
    a.open_default_tab();
    a.run(Action::OpenFiles);
    assert!(!a.files_open());
}

#[test]
fn sizes_and_dates_read_naturally() {
    assert_eq!(human_size(512), "512 B");
    assert_eq!(human_size(2048), "2.0 KB");
    assert_eq!(human_size(3_500_000), "3.3 MB");
    assert_eq!(human_size(50 * 1024 * 1024), "50 MB");
    assert_eq!(date(0), "1970-01-01");
    assert_eq!(date(1_763_164_800), "2025-11-15");
    assert_eq!(date(951_782_400), "2000-02-29");
}

#[test]
fn dropped_paths_are_quoted_for_the_shell_of_the_pane() {
    let paths = vec![r"C:\My Files\a.txt".to_owned(), r"C:\b.txt".to_owned()];
    let cmd = Profile::new("local:cmd", "CMD", r"C:\Windows\System32\cmd.exe", &[]);
    assert_eq!(crate::app::dropped_text(&cmd, &paths), r#""C:\My Files\a.txt" C:\b.txt"#);
    let pwsh = Profile::new("local:pwsh", "PowerShell", "pwsh.exe", &[]);
    assert_eq!(crate::app::dropped_text(&pwsh, &paths), r"'C:\My Files\a.txt' C:\b.txt");
    let wsl = Profile::new("local:wsl:u", "WSL", "wsl.exe", &[]);
    assert_eq!(crate::app::dropped_text(&wsl, &paths), "'/mnt/c/My Files/a.txt' /mnt/c/b.txt");
}

#[test]
fn f4_edits_a_file_locally_opens_it_and_counts_saves_until_stopped() {
    let (mut a, pane) = ssh_app();
    a.run(Action::OpenFiles);
    listed(&mut a, pane, "/etc");
    a.take_effects();
    key(&mut a, Key::Down);
    key(&mut a, Key::Down);
    key(&mut a, Key::F(4));
    let reqs = requests(&mut a);
    let FileRequest::Edit { id, remote } = &reqs[0] else { panic!("{reqs:?}") };
    assert_eq!(remote, "/etc/notes.txt");
    let id = *id;
    a.files_reply(pane, FileReply::Editing { id, local: "C:/tmp/aster-edit/1/notes.txt".into() });
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Open(p) if p.ends_with("notes.txt"))));
    a.files_reply(pane, FileReply::Saved { id, remote: "/etc/notes.txt".into() });
    a.files_reply(pane, FileReply::Saved { id, remote: "/etc/notes.txt".into() });
    assert_eq!(a.transfers_of(pane)[0].state, TransferState::Editing { saves: 2 });
    let (_, r) = a.files_geometry().unwrap().transfers[0];
    a.take_effects();
    a.on_event(Event::MouseDown { x: r.x + 5.0, y: r.y + 5.0, button: MouseButton::Left, clicks: 1 });
    assert_eq!(requests(&mut a), [FileRequest::Cancel(id)]);
    a.files_reply(pane, FileReply::Failed { id: Some(id), error: "cancelled".into() });
    assert!(matches!(&a.transfers_of(pane)[0].state, TransferState::Done(m) if m.contains("2 saves")));
}

#[test]
fn ctrl_d_downloads_folders_and_f9_sets_permissions_from_octal() {
    let (mut a, pane) = ssh_app();
    a.run(Action::OpenFiles);
    listed(&mut a, pane, "/srv");
    a.take_effects();
    key(&mut a, Key::Down);
    a.on_event(Event::Key { key: Key::Char('D'), mods: Mods { ctrl: true, ..Mods::NONE }, pressed: true });
    assert!(matches!(&requests(&mut a)[0], FileRequest::Download { remote, .. } if remote == "/srv/docs"));
    key(&mut a, Key::F(9));
    assert!(matches!(&a.files_page().unwrap().prompt, Some(Prompt::Chmod { text, .. }) if text == "755"));
    key(&mut a, Key::Backspace);
    key(&mut a, Key::Backspace);
    for c in "00".chars() {
        a.on_event(Event::Char(c));
    }
    key(&mut a, Key::Enter);
    assert_eq!(requests(&mut a), [FileRequest::Chmod { path: "/srv/docs".into(), mode: 0o700, recursive: true }]);
    assert_eq!(permissions(0o100754), "rwxr-xr--");
}
