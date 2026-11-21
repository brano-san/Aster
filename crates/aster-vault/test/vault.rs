use super::*;

fn sample() -> Vault {
    let mut vault = Vault::default();
    vault.set("ssh:password", "root@example.com", "hunter2");
    vault.set("ssh:key-passphrase", "id_ed25519", "p\u{e4}ss \"quoted\"");
    vault
}

#[test]
fn seal_open_roundtrip() {
    let vault = sample();
    let sealed = vault.seal("master");
    assert!(sealed.starts_with("aster-vault:1:"));
    assert!(sealed.is_ascii());
    assert!(!sealed.contains('\n'));
    assert_eq!(Vault::open(&sealed, "master").unwrap(), vault);
}

#[test]
fn empty_vault_roundtrip() {
    let sealed = Vault::default().seal("");
    assert_eq!(Vault::open(&sealed, "").unwrap(), Vault::default());
}

#[test]
fn wrong_passphrase_is_rejected() {
    let sealed = sample().seal("master");
    assert_eq!(
        Vault::open(&sealed, "other"),
        Err(VaultError::WrongPassphrase)
    );
}

#[test]
fn tampered_ciphertext_is_rejected() {
    let sealed = sample().seal("master");
    let mut blob = STANDARD.decode(&sealed["aster-vault:1:".len()..]).unwrap();
    let last = blob.len() - 1;
    blob[last] ^= 1;
    let tampered = format!("aster-vault:1:{}", STANDARD.encode(blob));
    assert_eq!(
        Vault::open(&tampered, "master"),
        Err(VaultError::WrongPassphrase)
    );
}

#[test]
fn corrupted_strings_are_rejected() {
    assert!(matches!(
        Vault::open("garbage", "master"),
        Err(VaultError::Corrupted(_))
    ));
    assert!(matches!(
        Vault::open("aster-vault:1:!!!not-base64!!!", "master"),
        Err(VaultError::Corrupted(_))
    ));
    assert!(matches!(
        Vault::open("aster-vault:1:AAAA", "master"),
        Err(VaultError::Corrupted(_))
    ));
    assert!(matches!(
        Vault::open("aster-vault:1:", "master"),
        Err(VaultError::Corrupted(_))
    ));
}

#[test]
fn foreign_version_is_unsupported() {
    let sealed = sample().seal("master");
    let other = sealed.replacen("aster-vault:1:", "aster-vault:2:", 1);
    assert!(matches!(
        Vault::open(&other, "master"),
        Err(VaultError::Unsupported(_))
    ));
}

#[test]
fn set_get_remove() {
    let mut vault = Vault::default();
    assert_eq!(vault.get("other", "b"), None);
    vault.set("other", "b", "1");
    vault.set("other", "c", "2");
    assert_eq!(vault.get("other", "b"), Some("1"));
    vault.set("other", "b", "3");
    assert_eq!(vault.get("other", "b"), Some("3"));
    assert_eq!(vault.secrets.len(), 2);
    assert_eq!(vault.get("x", "b"), None);
    assert!(vault.remove("other", "b"));
    assert!(!vault.remove("other", "b"));
    assert_eq!(vault.get("other", "b"), None);
    assert_eq!(vault.get("other", "c"), Some("2"));
}

#[test]
fn seal_uses_fresh_salt_and_nonce() {
    let vault = sample();
    let first = vault.seal("master");
    let second = vault.seal("master");
    assert_ne!(first, second);
    assert_eq!(Vault::open(&first, "master").unwrap(), vault);
    assert_eq!(Vault::open(&second, "master").unwrap(), vault);
}

#[test]
fn legacy_entries_get_typed_labels() {
    let data = br#"{"secrets":[
        {"kind":"password","key":"ssh:me@example.com:22","value":"ssh"},
        {"kind":"password","key":"key:SHA256:abc","value":"phrase"},
        {"kind":"password","key":"key-file:id_ed25519","value":"private"},
        {"kind":"password","key":"sudo:local","value":"other"}
    ]}"#;
    let vault = decode_contents(data).unwrap();
    let entries: Vec<_> = vault.secrets.iter().map(|s| (s.kind.as_str(), s.label.as_str())).collect();
    assert_eq!(entries, [
        ("ssh-password", "me@example.com:22"),
        ("key-passphrase", "SHA256:abc"),
        ("key-file", "id_ed25519"),
        ("other", "sudo:local"),
    ]);
    assert_eq!(vault.key_file("id_ed25519"), Some("private"));
    assert_eq!(Vault::open(&vault.seal("master"), "master").unwrap(), vault);
}
