use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SshOptions {
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_files: Option<Vec<String>>,
    pub keepalive_ms: Option<u64>,
    pub keepalive_count: Option<usize>,
    pub ready_timeout_ms: Option<u64>,
    pub skip_banner: Option<bool>,
    pub term: Option<String>,
    pub remember_cwd: Option<bool>,
    pub x11: Option<bool>,
    pub algorithms: Option<AlgorithmOptions>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct AlgorithmOptions {
    pub kex: Option<Vec<String>>,
    pub cipher: Option<Vec<String>>,
    pub mac: Option<Vec<String>>,
    pub host_key: Option<Vec<String>>,
    pub compression: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Algorithms {
    pub kex: Vec<String>,
    pub cipher: Vec<String>,
    pub mac: Vec<String>,
    pub host_key: Vec<String>,
    pub compression: Vec<String>,
}
