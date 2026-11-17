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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socket: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<Stream>,
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

    pub fn is_local(&self) -> bool {
        self.ssh.is_none() && self.serial.is_none() && self.socket.is_none()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InputMode {
    #[default]
    Normal,
    Echo,
    Line,
    LineHex,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutputMode {
    #[default]
    Normal,
    Hex,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Newlines {
    #[default]
    Keep,
    Strip,
    Cr,
    Lf,
    Crlf,
    ImplicitCr,
    ImplicitLf,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backspace {
    #[default]
    Backspace,
    CtrlH,
    #[serde(rename = "ctrl-?")]
    CtrlQuestion,
    Delete,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LoginStep {
    pub expect: String,
    pub send: String,
    pub regex: bool,
    pub optional: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Stream {
    pub input: InputMode,
    pub output: OutputMode,
    pub newlines_in: Newlines,
    pub newlines_out: Newlines,
    pub backspace: Backspace,
    pub login: Vec<LoginStep>,
}

impl Stream {
    pub fn options(&self) -> aster_proto::stream::StreamOptions {
        use aster_proto::{input, newline};
        let newlines = |n: Newlines| match n {
            Newlines::Keep => newline::Newlines::Keep,
            Newlines::Strip => newline::Newlines::Strip,
            Newlines::Cr => newline::Newlines::Cr,
            Newlines::Lf => newline::Newlines::Lf,
            Newlines::Crlf => newline::Newlines::CrLf,
            Newlines::ImplicitCr => newline::Newlines::ImplicitCr,
            Newlines::ImplicitLf => newline::Newlines::ImplicitLf,
        };
        aster_proto::stream::StreamOptions {
            input: match self.input {
                InputMode::Normal => input::InputMode::Normal,
                InputMode::Echo => input::InputMode::LocalEcho,
                InputMode::Line => input::InputMode::Readline,
                InputMode::LineHex => input::InputMode::ReadlineHex,
            },
            hex_output: self.output == OutputMode::Hex,
            newlines_in: newlines(self.newlines_in),
            newlines_out: newlines(self.newlines_out),
            backspace: match self.backspace {
                Backspace::Backspace => input::Backspace::Backspace,
                Backspace::CtrlH => input::Backspace::CtrlH,
                Backspace::CtrlQuestion => input::Backspace::CtrlQuestion,
                Backspace::Delete => input::Backspace::Delete,
            },
            login: self.login.iter().map(|s| aster_proto::scripts::Script { expect: s.expect.clone(), send: s.send.clone(), regex: s.regex, optional: s.optional }).collect(),
        }
    }
}

#[cfg(test)]
#[path = "../test/profile.rs"]
mod tests;
