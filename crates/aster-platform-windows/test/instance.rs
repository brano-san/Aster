use super::*;
use std::time::Duration;

#[test]
fn the_first_listener_gets_messages_and_a_second_one_is_refused() {
    std::env::set_var("ASTER_INSTANCE", format!("aster-test-{}", std::process::id()));
    let (woke_tx, woke) = channel();
    let rx = listen(Arc::new(move || {
        let _ = woke_tx.send(());
    }))
    .expect("first listener");
    assert!(listen(Arc::new(|| {})).is_none(), "only one instance listens");
    for text in ["one", "two \u{442}\u{435}\u{441}\u{442}"] {
        assert!(send(text));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).as_deref(), Ok(text));
        assert!(woke.recv_timeout(Duration::from_secs(5)).is_ok());
    }
}
