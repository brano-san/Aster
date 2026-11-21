use crate::{derive_key, random_bytes, VaultError, NONCE_LEN, SALT_LEN, TAG_LEN};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

#[derive(serde::Serialize, serde::Deserialize)]
struct Envelope {
    #[serde(rename = "aster-config-encrypted")]
    version: u32,
    ciphertext: String,
}

pub struct ConfigCipher {
    salt: [u8; SALT_LEN],
    cipher: XChaCha20Poly1305,
}

pub fn is_encrypted(text: &str) -> bool {
    text.trim_start_matches('\u{feff}').trim_start().starts_with('{')
}

fn envelope(text: &str) -> Result<Vec<u8>, VaultError> {
    let envelope: Envelope = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .map_err(|_| VaultError::Corrupted("invalid config envelope".to_owned()))?;
    if envelope.version != 1 {
        return Err(VaultError::Unsupported(format!("config envelope version {}", envelope.version)));
    }
    let blob = STANDARD.decode(&envelope.ciphertext).map_err(|_| VaultError::Corrupted("invalid config ciphertext".to_owned()))?;
    if blob.len() < SALT_LEN + NONCE_LEN + TAG_LEN {
        return Err(VaultError::Corrupted("config ciphertext is too short".to_owned()));
    }
    Ok(blob)
}

impl ConfigCipher {
    pub fn new(passphrase: &str) -> Self {
        Self::from_salt(passphrase, random_bytes())
    }

    fn from_salt(passphrase: &str, salt: [u8; SALT_LEN]) -> Self {
        let key = derive_key(passphrase, &salt);
        Self { salt, cipher: XChaCha20Poly1305::new((&key).into()) }
    }

    pub fn open(text: &str, passphrase: &str) -> Result<(String, Self), VaultError> {
        let blob = envelope(text)?;
        let salt = blob[..SALT_LEN].try_into().expect("checked config envelope length");
        let cipher = Self::from_salt(passphrase, salt);
        Ok((cipher.decrypt_blob(&blob)?, cipher))
    }

    pub fn encrypt(&self, config: &str) -> String {
        let nonce = random_bytes::<NONCE_LEN>();
        let ciphertext = self.cipher.encrypt(&XNonce::from(nonce), config.as_bytes())
            .expect("in-memory config encryption must succeed");
        let mut blob = Vec::with_capacity(SALT_LEN + NONCE_LEN + ciphertext.len());
        blob.extend_from_slice(&self.salt);
        blob.extend_from_slice(&nonce);
        blob.extend_from_slice(&ciphertext);
        serde_json::to_string(&Envelope { version: 1, ciphertext: STANDARD.encode(blob) })
            .expect("config envelope serializes to JSON")
    }

    pub fn decrypt(&self, text: &str) -> Result<String, VaultError> {
        self.decrypt_blob(&envelope(text)?)
    }

    fn decrypt_blob(&self, blob: &[u8]) -> Result<String, VaultError> {
        if blob[..SALT_LEN] != self.salt {
            return Err(VaultError::WrongPassphrase);
        }
        let (nonce, ciphertext) = blob[SALT_LEN..].split_at(NONCE_LEN);
        let nonce = XNonce::try_from(nonce).expect("checked config nonce length");
        let bytes = self.cipher.decrypt(&nonce, ciphertext).map_err(|_| VaultError::WrongPassphrase)?;
        String::from_utf8(bytes).map_err(|_| VaultError::Corrupted("config is not UTF-8".to_owned()))
    }
}
