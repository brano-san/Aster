use super::*;
use crate::test_server::{temp_ssh_dir, CLIENT_KEY, HOST_KEY};
use std::path::Path;

#[test]
fn passphrase_follows_the_public_key_after_a_move() {
    let store = Memory::default();
    let key = russh::keys::decode_secret_key(CLIENT_KEY, Some("pw")).unwrap();
    let signature = fingerprint_key(key.public_key());
    assert_eq!(signature, "key:SHA256:ZvPymiIOyh5SyrErsjRNa89hMH0mkCrGwJA8XeQPGfY");
    assert_ne!(fingerprint_key(russh::keys::decode_secret_key(HOST_KEY, None).unwrap().public_key()), signature);
    let dir = temp_ssh_dir();
    let old = dir.join("original");
    let moved = dir.join("moved");
    std::fs::write(&old, CLIENT_KEY).unwrap();
    let legacy = passphrase_key(&old);
    store.set(&legacy, "pw");
    let original = std::fs::read_to_string(&old).unwrap();
    assert!(EncryptedIdentity::new(&original).unlock_saved(&old, &store).is_some());
    assert_eq!(store.get(&legacy), None);
    assert_eq!(store.get(&signature).as_deref(), Some("pw"));
    std::fs::rename(&old, &moved).unwrap();
    let contents = std::fs::read_to_string(&moved).unwrap();
    assert!(EncryptedIdentity::new(&contents).unlock_saved(&moved, &store).is_some());
    std::fs::remove_file(moved).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn opaque_encrypted_key_finds_its_public_fingerprint_after_a_move() {
    let key = russh::keys::decode_secret_key(HOST_KEY, None).unwrap();
    let mut encoded = Vec::new();
    russh::keys::encode_pkcs8_pem_encrypted(&key, b"phrase", 100, &mut encoded).unwrap();
    let identity = EncryptedIdentity::new(std::str::from_utf8(&encoded).unwrap());
    let store = Memory::default();
    assert!(identity.save(&store, key.public_key(), "phrase"));
    assert!(identity.unlock_saved(Path::new("another/path"), &store).is_some());
    assert_eq!(store.get(&fingerprint_key(key.public_key())).as_deref(), Some("phrase"));

    let other = russh::keys::decode_secret_key(CLIENT_KEY, Some("pw")).unwrap();
    let other_id = fingerprint_key(other.public_key());
    let PassphraseLookup::EncryptedFile(index) = &identity.lookup else { panic!() };
    store.set(index, &other_id);
    store.set(&other_id, "phrase");
    assert!(identity.unlock_saved(Path::new("another/path"), &store).is_none());
    assert_eq!(store.get(&other_id).as_deref(), Some("phrase"));
    assert_eq!(store.get(index), None);
}
