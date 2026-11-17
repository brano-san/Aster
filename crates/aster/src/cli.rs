use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Command {
    Window,
    Open { dir: String },
    Run { command: Vec<String> },
    Profile { name: String },
    Paste { text: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub cwd: Option<String>,
    pub command: Command,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parsed {
    Request(Command),
    Version,
    Help,
    Error(String),
}

pub const USAGE: &str = "Usage:
  aster                     open a new window
  aster [open] DIR          open a tab in DIR
  aster run [--] CMD ARGS   open a tab running CMD
  aster profile NAME        open a tab with the profile NAME
  aster paste TEXT          paste TEXT into the active tab
  aster --version | --help";

pub fn parse(args: &[String], cwd: Option<&Path>) -> Parsed {
    let absolute = |dir: &str| match cwd {
        Some(base) if Path::new(dir).is_relative() => base.join(dir).to_string_lossy().into_owned(),
        _ => dir.to_owned(),
    };
    let rest = |from: usize| args[from..].to_vec();
    match args.first().map(String::as_str) {
        None => Parsed::Request(Command::Window),
        Some("--version" | "-V") => Parsed::Version,
        Some("--help" | "-h") => Parsed::Help,
        Some("open") => match args.len() {
            1 => Parsed::Request(Command::Open { dir: absolute(".") }),
            2 => Parsed::Request(Command::Open { dir: absolute(&args[1]) }),
            _ => Parsed::Error("open takes one folder".into()),
        },
        Some("run") => {
            let command = if args.get(1).map(String::as_str) == Some("--") { rest(2) } else { rest(1) };
            if command.is_empty() {
                Parsed::Error("run needs a command".into())
            } else {
                Parsed::Request(Command::Run { command })
            }
        }
        Some("profile") => match args.get(1..).unwrap_or_default() {
            [] => Parsed::Error("profile needs a name".into()),
            name => Parsed::Request(Command::Profile { name: name.join(" ") }),
        },
        Some("paste") => Parsed::Request(Command::Paste { text: rest(1).join(" ") }),
        Some(flag) if flag.starts_with('-') => Parsed::Error(format!("unknown option {flag}")),
        Some(dir) if args.len() == 1 => Parsed::Request(Command::Open { dir: absolute(dir) }),
        Some(word) => Parsed::Error(format!("unknown command {word}")),
    }
}

impl Request {
    pub fn encode(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn decode(text: &str) -> Option<Request> {
        serde_json::from_str(text).ok()
    }
}

#[cfg(test)]
#[path = "../test/cli.rs"]
mod tests;
