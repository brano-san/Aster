use super::*;
use crate::cli::{Command, Request};

#[test]
fn background_wakes_preserve_the_first_request_and_queue_one_window() {
    let (tx, rx) = std::sync::mpsc::channel();
    let mut startup = None;
    let requests = Some(&rx);
    assert!(!queue_background_request(&mut startup, requests));
    tx.send(String::new()).unwrap();
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
