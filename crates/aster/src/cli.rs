use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Command {
    Window,
    Hidden,
    Open { dir: String },
    Run {
        command: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        dir: Option<String>,
    },
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
  aster [--dir DIR] -e CMD [ARGS...]
  aster [--working-directory DIR] -- CMD [ARGS...]
  aster profile NAME        open a tab with the profile NAME
  aster paste TEXT          paste TEXT into the active tab
  aster --hidden            start in the background when tray or dock is enabled
  aster --version | --help";

pub fn parse(args: &[String], cwd: Option<&Path>) -> Parsed {
    let absolute = |dir: &str| match cwd {
        Some(base) if Path::new(dir).is_relative() => base.join(dir).to_string_lossy().into_owned(),
        _ => dir.to_owned(),
    };
    let rest = |from: usize| args[from..].to_vec();
    if let Some(flag) = args.first() {
        let (option, value) = flag.split_once('=').map_or((flag.as_str(), None), |(name, value)| (name, Some(value)));
        if matches!(option, "--dir" | "--working-directory") {
            let (dir, tail) = match value {
                Some(dir) => (dir, &args[1..]),
                None if args.len() > 1 => (args[1].as_str(), &args[2..]),
                None => return Parsed::Error(format!("{option} needs a folder")),
            };
            if dir.is_empty() {
                return Parsed::Error(format!("{option} needs a folder"));
            }
            return match tail.first().map(String::as_str) {
                Some("-e" | "--" | "run") => parse_run(tail, Some(absolute(dir))),
                _ => Parsed::Error(format!("{option} needs -e, -- or run followed by a command")),
            };
        }
    }
    match args.first().map(String::as_str) {
        None => Parsed::Request(Command::Window),
        Some("--version" | "-V") => Parsed::Version,
        Some("--help" | "-h") => Parsed::Help,
        Some("--hidden") if args.len() == 1 => Parsed::Request(Command::Hidden),
        Some("open") => match args.len() {
            1 => Parsed::Request(Command::Open { dir: absolute(".") }),
            2 => Parsed::Request(Command::Open { dir: absolute(&args[1]) }),
            _ => Parsed::Error("open takes one folder".into()),
        },
        Some("run" | "-e" | "--") => parse_run(args, None),
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

fn parse_run(args: &[String], dir: Option<String>) -> Parsed {
    let start = if args[0] == "run" && args.get(1).is_some_and(|arg| arg == "--") { 2 } else { 1 };
    let command = &args[start..];
    if command.first().is_none_or(String::is_empty) {
        Parsed::Error(format!("{} needs a command", args[0]))
    } else {
        Parsed::Request(Command::Run { command: command.to_vec(), dir })
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
