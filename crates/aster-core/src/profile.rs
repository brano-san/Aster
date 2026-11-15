use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: Vec<(String, String)>,
    pub icon: Option<String>,
    pub color: Option<String>,
    #[serde(skip)]
    pub builtin: bool,
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
