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

#[test]
fn delegated_forwarding_checks_the_binary_user_and_actual_session() {
    let scope = DelegatedScope::current().expect("current scope");
    let endpoint = format!(r"\\.\pipe\aster-delegated-test-{}", std::process::id());
    let rx = listen_at(endpoint.clone(), true, Arc::new(|| {})).expect("private listener");
    let wrong_binary = DelegatedScope { image: scope.image.with_file_name("other-test-binary.exe"), ..scope.clone() };
    let wrong_user = DelegatedScope { owner: InstanceOwner { user: "S-1-0-0".into(), ..scope.owner.clone() }, ..scope.clone() };
    let wrong_session = DelegatedScope { owner: InstanceOwner { session: scope.owner.session.wrapping_add(1), ..scope.owner.clone() }, ..scope.clone() };
    for foreign in [&wrong_binary, &wrong_user, &wrong_session] {
        assert_ne!(scope.name(), foreign.name());
        assert!(!send_at(&endpoint, "wrong destination", Some(foreign)));
    }
    assert_ne!(name(), scope.name());
    assert!(send_at(&endpoint, "valid destination", Some(&scope)));
    assert_eq!(rx.recv_timeout(Duration::from_secs(5)).as_deref(), Ok("valid destination"));
}

#[test]
fn installation_stops_the_scoped_copy_when_another_instance_owns_the_legacy_pipe() {
    let folder = std::env::temp_dir().join(format!("aster-installation-ipc-{}", std::process::id()));
    std::fs::create_dir(&folder).unwrap();
    let installed = folder.join("installed.exe");
    std::fs::copy(std::env::current_exe().unwrap(), &installed).unwrap();
    let ready = folder.join("ready");
    let mut child = std::process::Command::new(&installed)
        .args(["--exact", "instance::tests::installation_stop_test_child"])
        .env("ASTER_INSTALLATION_IPC_TEST_READY", &ready)
        .spawn().unwrap();
    let legacy = format!(r"\\.\pipe\aster-installation-ipc-test-{}", std::process::id());
    let other = listen_at(legacy.clone(), false, Arc::new(|| {})).unwrap();
    let start = Instant::now();
    while !ready.exists() {
        if start.elapsed() >= Duration::from_secs(10) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("installation test listener did not start");
        }
        assert!(child.try_wait().unwrap().is_none());
        std::thread::sleep(Duration::from_millis(10));
    }
    let stopped = stop_for_installation_at(&legacy, &installed, None);
    let exit = child.try_wait().unwrap();
    if exit.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    std::fs::remove_file(ready).unwrap();
    std::fs::remove_file(installed).unwrap();
    std::fs::remove_dir(folder).unwrap();
    assert!(exit.is_some_and(|status| status.success()));
    stopped.unwrap();
    assert!(matches!(other.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)));
}

#[test]
fn installation_stop_test_child() {
    let Some(ready) = std::env::var_os("ASTER_INSTALLATION_IPC_TEST_READY") else { return };
    let rx = delegated_listen(Arc::new(|| {})).unwrap();
    std::fs::write(ready, []).unwrap();
    assert_eq!(rx.recv_timeout(Duration::from_secs(15)).as_deref(), Ok(aster_platform::INSTALLATION_QUIT));
}
