use crate::{Secret, Vault, VaultError};
use aes::cipher::block_padding::Pkcs7;
use aes::cipher::{BlockModeDecrypt, KeyIvInit};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use sha2::Sha512;

const SUPPORTED_VERSION: u64 = 1;
const PBKDF_ITERATIONS: u32 = 100_000;
const KEY_LEN: usize = 32;
const IV_LEN: usize = 16;

#[derive(serde::Deserialize)]
struct StoredVault {
    version: u64,
    contents: String,
    #[serde(rename = "keySalt")]
    key_salt: String,
    iv: String,
}

#[derive(serde::Deserialize)]
struct EncryptedVault {
    #[serde(default)]
    secrets: Vec<EncryptedSecret>,
}

#[derive(serde::Deserialize)]
struct EncryptedSecret {
    #[serde(rename = "type")]
    kind: String,
    key: serde_json::Value,
    value: String,
}

fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&text[index..index + 2], 16).ok())
        .collect()
}

fn sorted(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let ordered: std::collections::BTreeMap<String, serde_json::Value> = map
                .into_iter()
                .map(|(key, inner)| (key, sorted(inner)))
                .collect();
            serde_json::Value::Object(ordered.into_iter().collect())
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(sorted).collect())
        }
        other => other,
    }
}

fn key_to_string(key: serde_json::Value) -> String {
    match key {
        serde_json::Value::String(text) => text,
        other => sorted(other).to_string(),
    }
}

pub fn import(stored_json: &str, passphrase: &str) -> Result<Vault, VaultError> {
    let stored: StoredVault = serde_json::from_str(stored_json)
        .map_err(|error| VaultError::Corrupted(format!("invalid stored vault: {error}")))?;
    if stored.version != SUPPORTED_VERSION {
        return Err(VaultError::Unsupported(format!(
            "encrypted vault version {}",
            stored.version
        )));
    }
    let salt = decode_hex(&stored.key_salt)
        .ok_or_else(|| VaultError::Corrupted("keySalt is not valid hex".to_owned()))?;
    let iv = decode_hex(&stored.iv)
        .ok_or_else(|| VaultError::Corrupted("iv is not valid hex".to_owned()))?;
    if iv.len() != IV_LEN {
        return Err(VaultError::Corrupted("iv has wrong length".to_owned()));
    }
    let encrypted = STANDARD
        .decode(stored.contents.as_bytes())
        .map_err(|error| VaultError::Corrupted(format!("invalid contents base64: {error}")))?;
    if encrypted.is_empty() || !encrypted.len().is_multiple_of(IV_LEN) {
        return Err(VaultError::Corrupted(
            "contents length is not a multiple of the block size".to_owned(),
        ));
    }

    let mut key = [0u8; KEY_LEN];
    pbkdf2::pbkdf2_hmac::<Sha512>(passphrase.as_bytes(), &salt, PBKDF_ITERATIONS, &mut key);
    let decryptor = cbc::Decryptor::<aes::Aes256>::new_from_slices(&key, &iv)
        .map_err(|_| VaultError::Corrupted("invalid key or iv length".to_owned()))?;
    let plaintext = decryptor
        .decrypt_padded_vec::<Pkcs7>(&encrypted)
        .map_err(|_| VaultError::WrongPassphrase)?;
    let parsed: EncryptedVault =
        serde_json::from_slice(&plaintext).map_err(|_| VaultError::WrongPassphrase)?;

    Ok(Vault {
        secrets: parsed
            .secrets
            .into_iter()
            .map(|secret| Secret {
                kind: secret.kind,
                key: key_to_string(secret.key),
                value: secret.value,
            })
            .collect(),
    })
}
