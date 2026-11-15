use std::collections::HashMap;
use std::sync::Mutex;

pub trait Secrets: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str) -> bool;
    fn delete(&self, key: &str);
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

pub fn passphrase_key(path: &std::path::Path) -> String {
    format!("key:{}", path.display())
}
