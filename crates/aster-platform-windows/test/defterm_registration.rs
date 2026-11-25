use super::*;
use std::cell::RefCell;

#[test]
fn only_console_providers_with_the_v3_handoff_are_offered() {
    let package = |version| format!("Microsoft.WindowsTerminal_{version}_x64__8wekyb3d8bbwe");
    assert!(!compatible_provider(&package("1.21.10351.0")));
    assert!(compatible_provider(&package("1.22.10352.0")));
    assert!(compatible_provider(&package("2.0.0.0")));
    assert!(!compatible_provider(&package("1.22")));
}
use std::thread;

fn test_mutex_name(suffix: &str) -> String {
    let user = sid(unsafe { GetCurrentProcess() }).unwrap();
    format!(r"Global\Aster.DefaultTerminal.Test-{}-{user}-{suffix}", std::process::id())
}

#[test]
fn another_owner_serializes_access_and_a_wait_has_a_deadline() {
    let name = test_mutex_name("timeout");
    let lock = RegistrationLock::named(&name, 1_000).unwrap();
    let peer_name = name.clone();
    let timeout = thread::spawn(move || RegistrationLock::named(&peer_name, 20).err().unwrap().kind()).join().unwrap();
    assert_eq!(timeout, io::ErrorKind::TimedOut);
    drop(lock);
    assert!(RegistrationLock::named(&name, 1_000).is_ok());
}

#[test]
fn an_abandoned_registration_mutex_is_acquired_for_recovery() {
    let name = test_mutex_name("abandoned");
    let peer_name = name.clone();
    let _handle = thread::spawn(move || {
        let handle = open_mutex(&peer_name).unwrap();
        assert_eq!(unsafe { WaitForSingleObject(HANDLE(handle.as_raw_handle()), 1_000) }, WAIT_OBJECT_0);
        handle
    }).join().unwrap();
    assert!(RegistrationLock::named(&name, 1_000).is_ok());
}

#[test]
fn backup_round_trips_missing_values_and_unmodified_registry_data() {
    let previous = [None, Some(Raw { kind: REG_SZ.0, bytes: vec![65, 0, 66, 0] })];
    let backup = encode_backup(&previous);
    assert_eq!(decode_backup(&backup).unwrap(), previous);
    assert_ne!(decode_backup(&backup).unwrap(), [Some(Raw::text("")), previous[1].clone()]);
    assert!(decode_backup(&backup[..backup.len() - 1]).is_err());
    let non_string = [Some(Raw { kind: 4, bytes: vec![2, 0, 0, 0] }), None];
    assert_eq!(decode_backup(&encode_backup(&non_string)).unwrap(), non_string);
}

#[test]
fn active_choice_requires_both_guids_even_if_the_terminal_is_ours() {
    let mut pair = selection();
    assert!(is_selected(&pair));
    pair[0] = Some(Raw::text("{00000000-0000-0000-0000-000000000000}"));
    assert!(!is_selected(&pair));
    pair[0] = Some(Raw::text("{2eaca947-7f5f-4cfa-ba87-8f7fbeefbe69}"));
    assert!(is_selected(&pair));
}

#[test]
fn com_registration_quotes_non_ascii_executable_and_names_it_separately() {
    let pair = registration(Path::new(r"C:\Some User\Программы\Aster\aster.exe"));
    assert_eq!(pair, [
        Some(Raw::text(r"C:\Some User\Программы\Aster\aster.exe")),
        Some(Raw::text(r#""C:\Some User\Программы\Aster\aster.exe" --defterm-server"#)),
    ]);
}

#[test]
fn changing_console_selection_rolls_back_after_the_second_write_fails() {
    let previous = [Some(Raw::text("old console")), None];
    let next = selection();
    for order in [CONSOLE_FIRST, TERMINAL_FIRST] {
        let store = RefCell::new(previous.clone());
        let result = replace_pair(order, &previous, &next, || Ok(store.borrow().clone()), |index, value| {
            if index == order[1] { return Err(io::Error::from(io::ErrorKind::PermissionDenied)); }
            store.borrow_mut()[index] = value.cloned();
            Ok(())
        });
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(*store.borrow(), previous);
    }
}

#[test]
fn a_later_external_choice_is_preserved_during_disable() {
    let expected = selection();
    let changed = [expected[0].clone(), Some(Raw::text("new external terminal"))];
    let result = replace_pair(CONSOLE_FIRST, &expected, &[None, None], || Ok(changed.clone()), |_, _| {
        panic!("selection changed since it was read")
    });
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
}

#[test]
fn a_choice_changed_between_writes_is_preserved() {
    let expected = selection();
    let external = [Some(Raw::text("external console")), Some(Raw::text("external terminal"))];
    let store = RefCell::new(expected.clone());
    let result = replace_pair(CONSOLE_FIRST, &expected, &[None, None], || Ok(store.borrow().clone()), |index, _| {
        assert_eq!(index, 0, "second write would overwrite the external change");
        *store.borrow_mut() = external.clone();
        Ok(())
    });
    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(*store.borrow(), external);
}

#[test]
fn partial_local_server_registration_is_recognized_only_if_present_values_match() {
    let expected = registration(Path::new(r"C:\Aster\aster.exe"));
    for partial in [[expected[0].clone(), None], [None, expected[1].clone()]] {
        assert!(compatible_registration(&partial, &expected));
    }
    let conflict = [expected[0].clone(), Some(Raw::text(r"C:\Other\host.exe"))];
    assert!(!compatible_registration(&conflict, &expected));
}

#[test]
fn interrupted_preference_transition_keeps_aster_as_the_ownership_marker() {
    let original = [None, Some(Raw::text("previous terminal"))];
    let selected = selection();
    for (from, to, order) in [(&original, &selected, TERMINAL_FIRST), (&selected, &original, CONSOLE_FIRST)] {
        let store = RefCell::new(from.clone());
        let midpoint = RefCell::new(None);
        replace_pair(order, from, to, || Ok(store.borrow().clone()), |index, value| {
            store.borrow_mut()[index] = value.cloned();
            if midpoint.borrow().is_none() { *midpoint.borrow_mut() = Some(store.borrow().clone()); }
            Ok(())
        }).unwrap();
        let midpoint = midpoint.into_inner().unwrap();
        assert_eq!(midpoint, [original[0].clone(), selected[1].clone()]);
        assert!(owns_choice(&midpoint, Some(&original)));
        assert!(!owns_choice(&midpoint, None));
        assert!(!owns_choice(&midpoint, Some(&midpoint)));
    }
    let external = [selected[0].clone(), Some(Raw::text("external terminal"))];
    assert!(!owns_choice(&external, Some(&original)));
    let changed_console = [Some(Raw::text("external console")), selected[1].clone()];
    assert!(!owns_choice(&changed_console, Some(&original)));
}
