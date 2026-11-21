pub mod config;

use argon2::{Algorithm, Argon2, Params, Version};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

const PREFIX: &str = "aster-vault:1:";
const FAMILY_PREFIX: &str = "aster-vault:";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;
const KEY_LEN: usize = 32;
const TAG_LEN: usize = 16;
const MEMORY_KIB: u32 = 19456;
const ITERATIONS: u32 = 2;
const LANES: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Secret {
    pub kind: String,
    pub key: String,
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Vault {
    #[serde(deserialize_with = "deserialize_secrets")]
    pub secrets: Vec<Secret>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum VaultError {
    WrongPassphrase,
    Corrupted(String),
    Unsupported(String),
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VaultError::WrongPassphrase => f.write_str("wrong passphrase"),
            VaultError::Corrupted(reason) => write!(f, "corrupted vault: {reason}"),
            VaultError::Unsupported(reason) => write!(f, "unsupported vault: {reason}"),
        }
    }
}

impl std::error::Error for VaultError {}

fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    getrandom::fill(&mut bytes).expect("operating system random source is unavailable");
    bytes
}

fn derive_key(passphrase: &str, salt: &[u8]) -> [u8; KEY_LEN] {
    let params = Params::new(MEMORY_KIB, ITERATIONS, LANES, Some(KEY_LEN))
        .expect("argon2 parameters are constant and valid");
    let mut key = [0u8; KEY_LEN];
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .expect("argon2 inputs are within limits");
    key
}

pub fn kind_for_key(key: &str) -> &'static str {
    if key.starts_with("ssh:") {
        "ssh-password"
    } else if key.starts_with("key:") {
        "key-passphrase"
    } else if key.starts_with("key-file:") {
        "key-file"
    } else {
        "other"
    }
}

fn canonical_kind<'a>(kind: &'a str, key: &str) -> &'a str {
    match kind {
        "password" | "" => kind_for_key(key),
        "ssh:password" => "ssh-password",
        "ssh:key-passphrase" => "key-passphrase",
        "ssh:key-file" => "key-file",
        _ => kind,
    }
}

fn label_for(kind: &str, key: &str) -> String {
    match kind {
        "ssh-password" => key.strip_prefix("ssh:"),
        "key-passphrase" => key.strip_prefix("key:"),
        "key-file" => key.strip_prefix("key-file:"),
        _ => None,
    }.unwrap_or(key).to_owned()
}

fn deserialize_secrets<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<Secret>, D::Error> {
    #[derive(serde::Deserialize)]
    struct Entry {
        #[serde(default)]
        kind: String,
        key: String,
        label: Option<String>,
        value: String,
    }

    let entries: Vec<Entry> = serde::Deserialize::deserialize(deserializer)?;
    Ok(entries.into_iter().map(|entry| {
        let kind = canonical_kind(&entry.kind, &entry.key);
        let kind = match kind {
            "ssh-password" | "key-passphrase" | "key-file" | "other" => kind,
            _ => "other",
        };
        let label = entry.label.unwrap_or_else(|| label_for(kind, &entry.key));
        Secret { kind: kind.to_owned(), key: entry.key, label, value: entry.value }
    }).collect())
}

fn decode_contents(plaintext: &[u8]) -> Result<Vault, VaultError> {
    serde_json::from_slice(plaintext)
        .map_err(|error| VaultError::Corrupted(format!("invalid contents: {error}")))
}

impl Vault {
    pub fn key_file(&self, fingerprint_or_label: &str) -> Option<&str> {
        let key = format!("key-file:{fingerprint_or_label}");
        self.get("key-file", &key).or_else(|| {
            self.secrets.iter()
                .find(|secret| secret.kind == "key-file" && (secret.key == fingerprint_or_label || secret.label == fingerprint_or_label))
                .map(|secret| secret.value.as_str())
        })
    }

    pub fn get(&self, kind: &str, key: &str) -> Option<&str> {
        let kind = canonical_kind(kind, key);
        self.secrets
            .iter()
            .find(|secret| secret.kind == kind && secret.key == key)
            .map(|secret| secret.value.as_str())
    }

    pub fn set(&mut self, kind: &str, key: &str, value: &str) {
        let kind = canonical_kind(kind, key);
        let kind = match kind {
            "ssh-password" | "key-passphrase" | "key-file" | "other" => kind,
            _ => "other",
        };
        match self
            .secrets
            .iter_mut()
            .find(|secret| secret.kind == kind && secret.key == key)
        {
            Some(secret) => secret.value = value.to_owned(),
            None => self.secrets.push(Secret {
                kind: kind.to_owned(),
                key: key.to_owned(),
                label: label_for(kind, key),
                value: value.to_owned(),
            }),
        }
    }

    pub fn remove(&mut self, kind: &str, key: &str) -> bool {
        let kind = canonical_kind(kind, key);
        let before = self.secrets.len();
        self.secrets
            .retain(|secret| !(secret.kind == kind && secret.key == key));
        self.secrets.len() != before
    }

    pub fn seal(&self, passphrase: &str) -> String {
        let salt = random_bytes::<SALT_LEN>();
        let nonce = random_bytes::<NONCE_LEN>();
        let key = derive_key(passphrase, &salt);
        let cipher = XChaCha20Poly1305::new((&key).into());
        let plaintext = serde_json::to_vec(self).expect("vault serializes to JSON");
        let ciphertext = cipher
            .encrypt(&XNonce::from(nonce), plaintext.as_slice())
            .expect("encryption of an in-memory buffer cannot fail");
        let mut blob = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
        blob.extend_from_slice(&salt);
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ciphertext);
        format!("{PREFIX}{}", STANDARD.encode(blob))
    }

    pub fn open(sealed: &str, passphrase: &str) -> Result<Vault, VaultError> {
        let encoded = match sealed.strip_prefix(PREFIX) {
            Some(encoded) => encoded,
            None => {
                return Err(match sealed.strip_prefix(FAMILY_PREFIX) {
                    Some(rest) => {
                        let version = rest.split(':').next().unwrap_or_default();
                        VaultError::Unsupported(format!("format version {version}"))
                    }
                    None => VaultError::Corrupted("missing aster-vault prefix".to_owned()),
                })
            }
        };
        let blob = STANDARD
            .decode(encoded)
            .map_err(|error| VaultError::Corrupted(format!("invalid base64: {error}")))?;
        if blob.len() < SALT_LEN + NONCE_LEN + TAG_LEN {
            return Err(VaultError::Corrupted("payload is too short".to_owned()));
        }
        let (salt, rest) = blob.split_at(SALT_LEN);
        let (nonce, ciphertext) = rest.split_at(NONCE_LEN);
        let key = derive_key(passphrase, salt);
        let cipher = XChaCha20Poly1305::new((&key).into());
        let plaintext = cipher
            .decrypt(
                &XNonce::try_from(nonce).expect("nonce length is fixed by split"),
                ciphertext,
            )
            .map_err(|_| VaultError::WrongPassphrase)?;
        decode_contents(&plaintext)
    }
}

#[cfg(test)]
#[path = "../test/vault.rs"]
mod tests;
