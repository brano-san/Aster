use super::*;

#[test]
fn a_secret_round_trips_through_the_credential_store_and_can_be_removed() {
    let key = format!("test:{}:{}", std::process::id(), "round-trip");
    assert_eq!(get(&key), None);
    assert!(set(&key, "p\u{e4}ss w0rd"));
    assert_eq!(get(&key).as_deref(), Some("p\u{e4}ss w0rd"));
    assert!(set(&key, "second"));
    assert_eq!(get(&key).as_deref(), Some("second"));
    delete(&key);
    assert_eq!(get(&key), None);
}
