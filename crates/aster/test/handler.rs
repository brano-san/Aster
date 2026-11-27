use super::*;
use crate::cli::{Command, Request};

#[test]
fn open_on_start_only_controls_the_first_window_without_a_restored_session() {
    let new_app = |open: bool| App::new(
        Config::parse(&format!("[terminal]\nopen-on-start = {open}")).unwrap(),
        vec![Profile::new("local:a", "A", "shell", &[])], true, None, Arc::new(|| {}),
    );
    let mut initial = new_app(true);
    initialize_tabs(&mut initial, None, true, None);
    assert_eq!(initial.tab_count(), 1);
    let saved = initial.session_snapshot();
    let mut start = new_app(false);
    initialize_tabs(&mut start, None, true, None);
    assert_eq!(start.tab_count(), 0);
    initialize_tabs(&mut start, Some(saved), true, None);
    assert_eq!(start.tab_count(), 1);
    let mut next = new_app(false);
    initialize_tabs(&mut next, None, false, None);
    assert_eq!(next.tab_count(), 1);
    let mut paste = new_app(false);
    initialize_tabs(&mut paste, None, true, Some(&Command::Paste { text: "a".into() }));
    assert_eq!(paste.tab_count(), 1);
    let mut explicit = new_app(true);
    initialize_tabs(&mut explicit, None, true, Some(&Command::Profile { name: "A".into() }));
    assert_eq!(explicit.tab_count(), 0);
}

#[test]
fn startup_paste_opens_a_tab_in_an_empty_restored_workspace() {
    let mut app = App::new(Config::default(), vec![Profile::new("local:a", "A", "shell", &[])], true, None, Arc::new(|| {}));
    let saved = WindowState { workspace: Some("Build".into()), ..WindowState::default() };
    let request = Request { cwd: None, command: Command::Paste { text: "hello".into() } };
    populate_window(&mut app, Some(saved), true, Some(request));
    assert_eq!(app.tab_count(), 1);
    assert_eq!(app.snapshot().workspace.as_deref(), Some("Build"));
}

#[test]
fn background_wakes_preserve_the_first_request_and_queue_one_window() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut startup = None;
    let requests = Some(&rx);
    assert!(!queue_background_request(&mut startup, requests));
    tx.send(String::new()).unwrap();
    tx.send(Request { cwd: None, command: Command::Hidden }.encode()).unwrap();
    let first = Request { cwd: None, command: Command::Profile { name: "Build".into() } };
    let next = Request { cwd: None, command: Command::Window };
    tx.send(first.encode()).unwrap();
    tx.send(next.encode()).unwrap();
    assert!(queue_background_request(&mut startup, requests));
    assert!(!queue_background_request(&mut startup, requests));
    assert_eq!(startup.take(), Some(first));
    assert_eq!(Request::decode(&rx.try_recv().unwrap()), Some(next));
    assert!(!queue_background_request(&mut startup, requests));
}
