use super::*;
use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockModeEncrypt, KeyIvInit};
use sha2::Sha512;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn tabby_stored(plaintext: &str, passphrase: &str, salt: &[u8], iv: &[u8; 16]) -> String {
    let mut key = [0u8; 32];
    pbkdf2::pbkdf2_hmac::<Sha512>(passphrase.as_bytes(), salt, 100_000, &mut key);
    let encrypted = cbc::Encryptor::<aes::Aes256>::new_from_slices(&key, iv)
        .unwrap()
        .encrypt_padded_vec::<Pkcs7>(plaintext.as_bytes());
    serde_json::json!({
        "version": 1,
        "contents": STANDARD.encode(encrypted),
        "keySalt": hex(salt),
        "iv": hex(iv),
    })
    .to_string()
}

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
    assert_eq!(vault.get("a", "b"), None);
    vault.set("a", "b", "1");
    vault.set("a", "c", "2");
    assert_eq!(vault.get("a", "b"), Some("1"));
    vault.set("a", "b", "3");
    assert_eq!(vault.get("a", "b"), Some("3"));
    assert_eq!(vault.secrets.len(), 2);
    assert_eq!(vault.get("x", "b"), None);
    assert!(vault.remove("a", "b"));
    assert!(!vault.remove("a", "b"));
    assert_eq!(vault.get("a", "b"), None);
    assert_eq!(vault.get("a", "c"), Some("2"));
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
fn import_tabby_vector() {
    let plaintext = serde_json::json!({
        "config": {"ignored": true},
        "secrets": [
            {
                "type": "ssh:password",
                "key": {"user": "root", "host": "example.com", "port": 22},
                "value": "hunter2"
            },
            {
                "type": "file",
                "key": {"id": "f1", "description": "note"},
                "value": "contents"
            }
        ]
    })
    .to_string();
    let stored = tabby_stored(&plaintext, "tabby-pass", b"01234567", b"abcdefghijklmnop");
    let vault = Vault::import_tabby(&stored, "tabby-pass").unwrap();
    assert_eq!(vault.secrets.len(), 2);
    assert_eq!(vault.secrets[0].kind, "ssh:password");
    assert_eq!(
        vault.secrets[0].key,
        r#"{"host":"example.com","port":22,"user":"root"}"#
    );
    assert_eq!(vault.secrets[0].value, "hunter2");
    assert_eq!(
        vault.get("file", r#"{"description":"note","id":"f1"}"#),
        Some("contents")
    );
}

#[test]
fn import_tabby_without_secrets_field() {
    let stored = tabby_stored("{}", "p", b"saltsalt", b"0123456789abcdef");
    assert_eq!(Vault::import_tabby(&stored, "p").unwrap(), Vault::default());
}

#[test]
fn import_tabby_wrong_passphrase() {
    let stored = tabby_stored(
        r#"{"secrets":[]}"#,
        "right",
        b"saltsalt",
        b"0123456789abcdef",
    );
    assert_eq!(
        Vault::import_tabby(&stored, "wrong"),
        Err(VaultError::WrongPassphrase)
    );
}

#[test]
fn import_tabby_rejects_bad_input() {
    assert!(matches!(
        Vault::import_tabby("not json", "p"),
        Err(VaultError::Corrupted(_))
    ));
    let stored = tabby_stored("{}", "p", b"saltsalt", b"0123456789abcdef");
    let other_version = stored.replace("\"version\":1", "\"version\":2");
    assert!(matches!(
        Vault::import_tabby(&other_version, "p"),
        Err(VaultError::Unsupported(_))
    ));
    let bad_hex = stored.replace("\"iv\":\"", "\"iv\":\"zz");
    assert!(matches!(
        Vault::import_tabby(&bad_hex, "p"),
        Err(VaultError::Corrupted(_))
    ));
}
