use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
pub use aster_proto::ssh::SshOptions;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OnExit {
    #[serde(skip)]
    #[default]
    Inherit,
    Auto,
    Keep,
    Close,
    Reconnect,
}

impl OnExit {
    fn is_inherited(&self) -> bool {
        *self == Self::Inherit
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ProfileDefaults {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub scheme: Option<String>,
    pub color: Option<String>,
    pub stream: Option<Stream>,
    pub on_exit: Option<OnExit>,
    pub ssh_options: Option<SshOptions>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct ProfileGroup {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub defaults: BTreeMap<String, ProfileDefaults>,
}

fn group_chain<'a>(name: &str, config: &'a Config) -> Vec<&'a ProfileGroup> {
    let groups = &config.groups;
    let mut group = groups.iter().find(|g| g.name == name);
    let mut chain: Vec<&ProfileGroup> = Vec::new();
    while let Some(current) = group {
        if chain.iter().any(|&seen| std::ptr::eq(seen, current)) {
            break;
        }
        chain.push(current);
        group = current.parent.as_deref().filter(|p| !p.is_empty()).and_then(|p| {
            groups.iter().find(|g| g.id == p).or_else(|| groups.iter().find(|g| g.name == p))
        });
    }
    chain
}

pub fn group_path(name: &str, config: &Config) -> String {
    let chain = group_chain(name, config);
    if chain.is_empty() {
        return name.to_owned();
    }
    chain.iter().rev().map(|g| g.name.as_str()).collect::<Vec<_>>().join(" › ")
}

pub fn effective(profile: &Profile, config: &Config) -> Profile {
    let kind = profile.defaults_kind();
    let mut result = profile.clone();
    if let Some(name) = profile.group.as_deref().map(str::trim).filter(|g| !g.is_empty()) {
        for group in group_chain(name, config) {
            if let Some(defaults) = kind.and_then(|key| group.defaults.get(key)) {
                result.inherit(defaults);
            }
            fill(&mut result.icon, &group.icon);
            fill(&mut result.color, &group.color);
        }
    }
    if let Some(defaults) = kind.and_then(|key| config.profile_defaults.get(key)) {
        result.inherit(defaults);
    }
    if result.on_exit == OnExit::Inherit {
        result.on_exit = OnExit::Auto;
    }
    result
}

fn fill<T: Clone>(value: &mut Option<T>, fallback: &Option<T>) {
    if value.is_none() {
        value.clone_from(fallback);
    }
}

fn inherit_ssh(options: &mut SshOptions, defaults: &SshOptions) {
    fill(&mut options.user, &defaults.user);
    fill(&mut options.port, &defaults.port);
    fill(&mut options.identity_files, &defaults.identity_files);
    fill(&mut options.keepalive_ms, &defaults.keepalive_ms);
    fill(&mut options.keepalive_count, &defaults.keepalive_count);
    fill(&mut options.ready_timeout_ms, &defaults.ready_timeout_ms);
    fill(&mut options.skip_banner, &defaults.skip_banner);
    fill(&mut options.term, &defaults.term);
    fill(&mut options.remember_cwd, &defaults.remember_cwd);
    fill(&mut options.x11, &defaults.x11);
    if let Some(algorithms) = &defaults.algorithms {
        let target = options.algorithms.get_or_insert_default();
        fill(&mut target.kex, &algorithms.kex);
        fill(&mut target.cipher, &algorithms.cipher);
        fill(&mut target.mac, &algorithms.mac);
        fill(&mut target.host_key, &algorithms.host_key);
        fill(&mut target.compression, &algorithms.compression);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExitDecision {
    Close,
    Keep,
    Reconnect,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<crate::session::ProfileLayout>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub hidden: bool,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh: Option<String>,
    #[serde(default, rename = "ssh-options", skip_serializing_if = "Option::is_none")]
    pub ssh_options: Option<SshOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub socket: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<Stream>,
    #[serde(rename = "on-exit", skip_serializing_if = "OnExit::is_inherited")]
    pub on_exit: OnExit,
    #[serde(skip)]
    pub builtin: bool,
    #[serde(skip)]
    pub elevated: bool,
    #[serde(skip)]
    pub hold: bool,
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
        self.layout.is_none() && self.ssh.is_none() && self.serial.is_none() && self.socket.is_none()
    }

    pub(crate) fn is_wsl(&self) -> bool {
        self.id.starts_with("local:wsl") || self.command.rsplit(['/', '\\']).next().is_some_and(|name| name.eq_ignore_ascii_case("wsl.exe") || name.eq_ignore_ascii_case("wsl"))
    }

    fn defaults_kind(&self) -> Option<&'static str> {
        if self.layout.is_some() {
            None
        } else if self.ssh.is_some() {
            Some("ssh")
        } else if self.serial.is_some() {
            Some("serial")
        } else if self.socket.is_some() {
            Some("socket")
        } else {
            Some("local")
        }
    }

    fn inherit(&mut self, defaults: &ProfileDefaults) {
        for (key, value) in &defaults.env {
            self.env.entry(key.clone()).or_insert_with(|| value.clone());
        }
        fill(&mut self.cwd, &defaults.cwd);
        fill(&mut self.scheme, &defaults.scheme);
        fill(&mut self.color, &defaults.color);
        if let Some(stream) = &defaults.stream {
            self.stream.get_or_insert_default().inherit(stream);
        }
        if let Some(ssh) = &defaults.ssh_options {
            inherit_ssh(self.ssh_options.get_or_insert_default(), ssh);
        }
        if self.on_exit == OnExit::Inherit {
            self.on_exit = defaults.on_exit.unwrap_or(OnExit::Inherit);
        }
    }

    pub(crate) fn exit_decision(&self, code: Option<u32>) -> ExitDecision {
        match self.on_exit {
            OnExit::Close => ExitDecision::Close,
            OnExit::Auto | OnExit::Inherit if code == Some(0) && !self.hold => ExitDecision::Close,
            OnExit::Reconnect if !self.is_local() => ExitDecision::Reconnect,
            _ => ExitDecision::Keep,
        }
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
    pub input: Option<InputMode>,
    pub output: Option<OutputMode>,
    pub newlines_in: Option<Newlines>,
    pub newlines_out: Option<Newlines>,
    pub backspace: Option<Backspace>,
    pub login: Option<Vec<LoginStep>>,
}

impl Stream {
    fn inherit(&mut self, defaults: &Self) {
        fill(&mut self.input, &defaults.input);
        fill(&mut self.output, &defaults.output);
        fill(&mut self.newlines_in, &defaults.newlines_in);
        fill(&mut self.newlines_out, &defaults.newlines_out);
        fill(&mut self.backspace, &defaults.backspace);
        fill(&mut self.login, &defaults.login);
    }

    pub fn input_mode(&self) -> InputMode {
        self.input.unwrap_or_default()
    }

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
            input: match self.input_mode() {
                InputMode::Normal => input::InputMode::Normal,
                InputMode::Echo => input::InputMode::LocalEcho,
                InputMode::Line => input::InputMode::Readline,
                InputMode::LineHex => input::InputMode::ReadlineHex,
            },
            hex_output: self.output == Some(OutputMode::Hex),
            newlines_in: newlines(self.newlines_in.unwrap_or_default()),
            newlines_out: newlines(self.newlines_out.unwrap_or_default()),
            backspace: match self.backspace.unwrap_or_default() {
                Backspace::Backspace => input::Backspace::Backspace,
                Backspace::CtrlH => input::Backspace::CtrlH,
                Backspace::CtrlQuestion => input::Backspace::CtrlQuestion,
                Backspace::Delete => input::Backspace::Delete,
            },
            login: self.login.as_deref().unwrap_or_default().iter().map(|s| aster_proto::scripts::Script { expect: s.expect.clone(), send: s.send.clone(), regex: s.regex, optional: s.optional }).collect(),
        }
    }
}

#[cfg(test)]
#[path = "../test/profile.rs"]
mod tests;
