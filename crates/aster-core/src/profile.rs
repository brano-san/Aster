use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    #[serde(skip)]
    pub builtin: bool,
    #[serde(skip)]
    pub elevated: bool,
}

impl Profile {
    pub fn new(id: &str, name: &str, command: &str, args: &[&str]) -> Self {
        Self {
            id: id.to_owned(),
            name: name.to_owned(),
            command: command.to_owned(),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
            builtin: true,
            ..Self::default()
        }
    }
}
