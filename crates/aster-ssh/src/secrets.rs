use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use russh::keys::{HashAlg, PrivateKey, PublicKey};
use russh::keys::ssh_encoding::base64::{Base64Unpadded, Encoding};

pub trait Secrets: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str) -> bool;
    fn delete(&self, key: &str);

    fn key_file(&self, fingerprint_or_label: &str) -> Option<String> {
        self.get(&key_file_key(fingerprint_or_label))
    }
}

#[derive(Default)]
pub struct Memory {
    values: Mutex<HashMap<String, String>>,
}

impl Secrets for Memory {
    fn get(&self, key: &str) -> Option<String> {
        self.values.lock().ok()?.get(key).cloned()
    }

    fn set(&self, key: &str, value: &str) -> bool {
        self.values.lock().map(|mut v| v.insert(key.to_owned(), value.to_owned())).is_ok()
    }

    fn delete(&self, key: &str) {
        if let Ok(mut v) = self.values.lock() {
            v.remove(key);
        }
    }
}

pub fn password_key(user: &str, host: &str, port: u16) -> String {
    format!("ssh:{user}@{host}:{port}")
}

pub fn key_file_key(fingerprint_or_label: &str) -> String {
    format!("key-file:{fingerprint_or_label}")
}

pub fn passphrase_key(path: &Path) -> String {
    format!("key:{}", path.display())
}

pub fn private_key_fingerprint(encoded: &str) -> Option<String> {
    PrivateKey::from_openssh(encoded.trim()).ok()
        .or_else(|| russh::keys::decode_secret_key(encoded, None).ok())
        .map(|key| key.public_key().fingerprint(HashAlg::Sha256).to_string())
}

fn fingerprint_key(key: &PublicKey) -> String {
    format!("key:{}", key.fingerprint(HashAlg::Sha256))
}

pub(crate) struct EncryptedIdentity<'a> {
    encoded: &'a str,
    lookup: PassphraseLookup,
}

enum PassphraseLookup {
    PublicKey(String),
    EncryptedFile(String),
}

impl<'a> EncryptedIdentity<'a> {
    pub(crate) fn new(encoded: &'a str) -> Self {
        let lookup = match PrivateKey::from_openssh(encoded.trim()) {
            Ok(key) => PassphraseLookup::PublicKey(fingerprint_key(key.public_key())),
            Err(_) => {
                let hash = Base64Unpadded::encode_string(&HashAlg::Sha256.digest(encoded.as_bytes()));
                PassphraseLookup::EncryptedFile(format!("key-index:SHA256:{hash}"))
            }
        };
        Self { encoded, lookup }
    }

    pub(crate) fn unlock_saved(&self, path: &Path, secrets: &dyn Secrets) -> Option<PrivateKey> {
        let fingerprint = match &self.lookup {
            PassphraseLookup::PublicKey(id) => Some(id.clone()),
            PassphraseLookup::EncryptedFile(index) => secrets.get(index).filter(|id| id.starts_with("key:SHA256:")),
        };
        let legacy = passphrase_key(path);
        for id in fingerprint.as_deref().into_iter().chain(Some(legacy.as_str())) {
            let Some(pass) = secrets.get(id) else { continue };
            if let Ok(key) = russh::keys::decode_secret_key(self.encoded, Some(&pass)) {
                if id == legacy {
                    if self.save(secrets, key.public_key(), &pass) {
                        secrets.delete(&legacy);
                    }
                    return Some(key);
                }
                if id == fingerprint_key(key.public_key()) {
                    return Some(key);
                }
            }
            match &self.lookup {
                PassphraseLookup::EncryptedFile(index) if id != legacy => secrets.delete(index),
                _ => secrets.delete(id),
            }
        }
        None
    }

    pub(crate) fn save(&self, secrets: &dyn Secrets, key: &PublicKey, pass: &str) -> bool {
        let id = fingerprint_key(key);
        secrets.set(&id, pass) && match &self.lookup {
            PassphraseLookup::PublicKey(_) => true,
            PassphraseLookup::EncryptedFile(index) => secrets.set(index, &id),
        }
    }
}

#[cfg(test)]
#[path = "../test/secrets.rs"]
mod tests;
