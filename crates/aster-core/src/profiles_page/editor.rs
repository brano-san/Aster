use crate::config::Config;
use crate::i18n::{t, tf};
use crate::input::{Key, Mods};
use crate::profile::{group_path, LoginStep, OnExit, Profile, ProfileDefaults, SshOptions, Stream};
use crate::settings::{join_args, split_args};
use aster_proto::serial::{self, Defaults as SerialDefaults};
use aster_proto::ssh::{AlgorithmCatalog, AlgorithmOptions};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::BTreeMap;

#[path = "ssh.rs"]
mod ssh;
#[path = "forwards.rs"]
mod forwards;
#[path = "algorithms.rs"]
mod algorithms;
pub(super) use ssh::SshEditorAction;

type Choices = Vec<(String, String)>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Name, Group, Icon, Color, Scheme, Exit, Hidden, KeepTitle, ClearAfterConnect, Command, Cwd, Admin, Env, Destination, User, Port,
    Identity, Keepalive, Count, Timeout, Banner, Remember, X11, Term, Device, Speed, Bits, Parity,
    Stop, Flow, Slow, Address, Input, Output, InLines, OutLines, Backspace, Expect, Send, Regex, Optional,
    ProxyMethod, ProxyTarget, Auth, Password, SetPassword, ForgetPassword, BrowseKey, ImportKey, VaultKey,
    ForwardType, ForwardListen, ForwardDestination, ForwardDescription, AddForward,
    ForwardAgent, ReuseSession,
    AlgorithmCategory, AlgorithmCheck,
}

impl Field {
    fn label(self) -> &'static str {
        match self {
            Self::Name => t("Name"), Self::Group => t("Group"), Self::Color => t("Color"),
            Self::Icon => t("Icon"), Self::KeepTitle => t("Ignore tab titles from terminal output"),
            Self::ClearAfterConnect => t("Clear the terminal after connecting"),
            Self::Scheme => t("Color scheme"), Self::Exit => t("When the session ends"), Self::Hidden => t("Hidden"),
            Self::Command => t("Command line"), Self::Cwd => t("Start folder"), Self::Admin => t("Run as administrator"),
            Self::Env => t("Environment (key=value)"), Self::Destination => t("SSH destination"), Self::User => t("User"),
            Self::Port => t("Port"), Self::Identity => t("Identity files"), Self::Keepalive => t("Keepalive interval (ms)"),
            Self::Count => t("Keepalive count"), Self::Timeout => t("Ready timeout (ms)"), Self::Banner => t("Skip banner"),
            Self::Remember => t("Remember folder"), Self::X11 => t("X11 forwarding"), Self::Term => t("$TERM"),
            Self::Device => t("Serial port"), Self::Speed => t("Speed"), Self::Bits => t("Data bits"),
            Self::Parity => t("Parity"), Self::Stop => t("Stop bits"), Self::Flow => t("Flow control"),
            Self::Slow => t("Slow send"), Self::Address => t("Telnet or TCP address"),
            Self::Input => t("Input mode"), Self::Output => t("Output mode"), Self::InLines => t("Newlines in"),
            Self::OutLines => t("Newlines out"), Self::Backspace => t("Backspace"), Self::Expect => t("Login script · Expect"),
            Self::Send => t("Send"), Self::Regex => t("Regular expressions"), Self::Optional => t("Optional"),
            Self::ProxyMethod => t("Connection method"), Self::ProxyTarget => "", Self::Auth => t("Authentication"),
            Self::Password => t("Password"), Self::SetPassword => t("Set password"), Self::ForgetPassword => t("Forget password"),
            Self::BrowseKey => t("Choose private key file"), Self::ImportKey => t("Import a key into the vault"), Self::VaultKey => t("Choose a vault key"),
            Self::ForwardType => t("Forward type"), Self::ForwardListen => t("Listening address"),
            Self::ForwardDestination => t("Forward to"), Self::ForwardDescription => t("Forward description"),
            Self::AddForward => t("Add port forward"),
            Self::ForwardAgent => t("Forward the SSH agent"), Self::ReuseSession => t("Reuse SSH connection"),
            Self::AlgorithmCategory => t("Algorithm category"), Self::AlgorithmCheck => "",
        }
    }

    fn text(self, value: impl Into<String>) -> EditorField {
        EditorField { id: self, label: self.label().into(), value: value.into(), choices: Vec::new(), editable: true }
    }

    fn choice(self, value: impl Into<String>, choices: Choices, editable: bool) -> EditorField {
        EditorField { choices, editable, ..self.text(value) }
    }

    fn toggle(self, value: Option<bool>, inherit: bool) -> EditorField {
        self.choice(value.map(|v| v.to_string()).unwrap_or_default(), options(&[("false", "Off"), ("true", "On")], inherit), false)
    }

    fn is_step(self) -> bool { matches!(self, Self::Expect | Self::Send | Self::Regex | Self::Optional) }
}

#[derive(Clone, PartialEq, Eq)]
pub struct EditorField {
    id: Field,
    pub label: String,
    pub value: String,
    pub choices: Choices,
    pub editable: bool,
}

impl std::fmt::Debug for EditorField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = if self.id == Field::Password { "•••" } else { &self.value };
        f.debug_struct("EditorField").field("id", &self.id).field("value", &value).finish()
    }
}

impl EditorField {
    pub fn is_icon(&self) -> bool { self.id == Field::Icon }

    pub fn list_hint(&self) -> Option<&'static str> {
        if self.id.is_step() {
            Some(t("Enter add step  ·  Del remove step  ·  Ctrl+Enter save"))
        } else if matches!(self.id, Field::Env | Field::Identity) {
            Some(t("Enter add row  ·  Del remove row  ·  Ctrl+Enter save"))
        } else if self.id == Field::User {
            Some(t("Leave the user empty to ask when connecting"))
        } else if self.id.is_forward() {
            Some(t("Del remove this port forward"))
        } else if self.id == Field::AlgorithmCheck {
            Some(t("Keep at least one algorithm in each category"))
        } else { None }
    }

    pub fn switch(&self) -> Option<bool> {
        (self.choices.len() == 2 && self.choices[0].0 == "false" && self.choices[1].0 == "true").then(|| self.value == "true")
    }

    pub fn display(&self) -> &str {
        if self.id == Field::ForwardDestination && !self.editable { return ""; }
        if self.id == Field::Color { return &self.value; }
        if self.is_icon() && self.value.is_empty() { return t("Automatic"); }
        self.choices.iter().find(|(k, _)| k == &self.value).map_or(&self.value, |(_, v)| v)
    }

    fn cycle(&mut self, backwards: bool) {
        if self.choices.is_empty() { return; }
        let len = self.choices.len();
        let next = self.choices.iter().position(|(k, _)| *k == self.value).map_or(0, |i| {
            (i + if backwards { len - 1 } else { 1 }) % len
        });
        self.value.clone_from(&self.choices[next].0);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorTab {
    pub label: String,
    pub fields: Vec<EditorField>,
}

impl EditorTab {
    fn new(label: &str, fields: Vec<EditorField>) -> Self { Self { label: t(label).into(), fields } }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProfileEditor {
    pub original: Option<String>,
    pub error: Option<String>,
    pub tab: usize,
    pub focus: usize,
    pub tabs: Vec<EditorTab>,
    source: Profile,
    defaults: Option<ProfileDefaults>,
    kind: &'static str,
    groups: Vec<crate::profile::ProfileGroup>,
    global: BTreeMap<String, ProfileDefaults>,
    jump_choices: Choices,
    proxy_arguments: BTreeMap<String, String>,
    algorithm_catalog: AlgorithmCatalog,
    algorithm_options: AlgorithmOptions,
}

fn options(items: &[(&str, &str)], inherit: bool) -> Choices {
    let mut choices = Vec::new();
    if inherit { choices.push((String::new(), t("Inherit").into())); }
    choices.extend(items.iter().map(|(key, label)| ((*key).into(), t(label).into())));
    choices
}

fn names(values: &[String]) -> Choices {
    values.iter().map(|v| (v.clone(), v.clone())).collect()
}

fn optional<V: ToString>(value: Option<V>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

fn enum_text<T: Serialize>(value: Option<T>) -> String {
    value.and_then(|v| serde_json::to_value(v).ok()).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

fn enum_value<T: DeserializeOwned>(value: &str) -> Option<T> {
    serde_json::from_value(serde_json::Value::String(value.into())).ok()
}

const EXITS: &[(&str, &str)] = &[("auto", "Automatic"), ("keep", "Keep open"), ("reconnect", "Reconnect"), ("close", "Close")];
const INPUTS: &[(&str, &str)] = &[("normal", "Normal"), ("echo", "Local echo"), ("line", "Line"), ("line-hex", "Hex line")];
const OUTPUTS: &[(&str, &str)] = &[("normal", "Normal"), ("hex", "Hex")];
const NEWLINES: &[(&str, &str)] = &[("keep", "Keep"), ("strip", "Strip"), ("cr", "CR"), ("lf", "LF"), ("crlf", "CRLF"), ("implicit-cr", "Implicit CR"), ("implicit-lf", "Implicit LF")];
const BACKSPACES: &[(&str, &str)] = &[("backspace", "Backspace"), ("ctrl-h", "Ctrl+H"), ("ctrl-?", "Ctrl+?"), ("delete", "Delete")];
const PARITIES: &[(&str, &str)] = &[("none", "None"), ("odd", "Odd"), ("even", "Even")];
const WINDOWS_PARITIES: &[(&str, &str)] = &[("mark", "Mark"), ("space", "Space")];
const FLOWS: &[(&str, &str)] = &[("none", "None"), ("software", "XON/XOFF"), ("hardware", "RTS/CTS"), ("xany", "XON/XOFF/XANY")];

fn command_line(p: &Profile) -> String {
    if p.command.is_empty() && p.args.is_empty() { return String::new(); }
    join_args(&std::iter::once(p.command.clone()).chain(p.args.iter().cloned()).collect::<Vec<_>>())
}

fn kind(p: &Profile) -> &'static str {
    if p.layout.is_some() { "layout" } else if p.ssh.is_some() { "ssh" } else if p.serial.is_some() { "serial" } else if p.socket.is_some() { "socket" } else { "local" }
}

impl ProfileEditor {
    pub fn profile(p: Profile, original: Option<String>, config: &Config, schemes: &[String], ports: &[String]) -> Self {
        let kind = kind(&p);
        Self::new(p, original, None, kind, config, schemes, ports)
    }

    pub fn defaults(kind: &str, d: &ProfileDefaults, config: &Config, schemes: &[String], ports: &[String]) -> Self {
        let kind = match kind { "ssh" => "ssh", "serial" => "serial", "socket" | "network" => "socket", _ => "local" };
        let p = Profile {
            env: d.env.clone(), cwd: d.cwd.clone(), color: d.color.clone(), scheme: d.scheme.clone(),
            keep_tab_title: d.keep_tab_title, clear_after_connect: d.clear_after_connect,
            stream: d.stream.clone(), on_exit: d.on_exit.unwrap_or_default(), ssh_options: d.ssh_options.clone(),
            ssh: d.ssh.clone(), socket: d.socket.clone(), admin: d.admin, ..Profile::default()
        };
        Self::new(p, None, Some(d.clone()), kind, config, schemes, ports)
    }

    fn new(p: Profile, original: Option<String>, defaults: Option<ProfileDefaults>, kind: &'static str, config: &Config, schemes: &[String], ports: &[String]) -> Self {
        let is_defaults = defaults.is_some();
        let mut fields = Vec::new();
        if !is_defaults {
            fields.push(Field::Name.text(&p.name));
            let mut groups: Choices = config.groups.iter().map(|g| (g.name.clone(), group_path(&g.name, config))).collect();
            groups.sort_by(|a,b| a.1.cmp(&b.1));
            if let Some(name) = &p.group {
                if !groups.iter().any(|(v, _)| v == name) { groups.push((name.clone(), name.clone())); }
            }
            groups.insert(0, (String::new(), t("Ungrouped").into()));
            fields.push(Field::Group.choice(optional(p.group.as_ref()), groups, false));
            fields.push(Field::Icon.choice(optional(p.icon.as_ref()), Vec::new(), false));
        }
        let presets = crate::app::TAB_COLORS.iter().map(|&(label, color)| (color, label)).collect::<Vec<_>>();
        fields.push(Field::Color.choice(optional(p.color.as_ref()), options(&presets, false), true));
        let mut schemes = names(schemes);
        schemes.insert(0, (String::new(), t("Window scheme").into()));
        let scheme = Field::Scheme.choice(optional(p.scheme.as_ref()), schemes, true);
        fields.push(Field::Exit.choice(enum_text((p.on_exit != OnExit::Inherit).then_some(p.on_exit)), options(EXITS, true), false));
        if !is_defaults { fields.push(Field::Hidden.toggle(Some(p.hidden), false)); }
        fields.push(Field::KeepTitle.toggle(p.keep_tab_title, true));
        fields.push(Field::ClearAfterConnect.toggle(p.clear_after_connect, true));
        let mut tabs = vec![EditorTab::new("General", fields)];
        match kind {
            "local" => tabs.push(command_tab(&p, is_defaults)),
            "ssh" => tabs.extend([ssh::tab(&p, is_defaults), forwards::tab(&p), ssh::advanced_tab(&p), algorithms::tab()]),
            "serial" => tabs.push(serial_tab(&p, defaults.as_ref(), ports)),
            "socket" => tabs.push(EditorTab::new("Network", vec![Field::Address.text(optional(p.socket.as_ref()))])),
            _ => {}
        }
        if matches!(kind, "ssh" | "serial" | "socket") { tabs.push(stream_tab(p.stream.as_ref())); }
        tabs.push(EditorTab::new("Colors", vec![scheme]));
        let algorithm_options = p.ssh_options.as_ref().and_then(|s| s.algorithms.clone()).unwrap_or_default();
        let mut editor = Self { original, error: None, tab: 0, focus: 0, tabs, source: p, defaults, kind, groups: config.groups.clone(), global: config.profile_defaults.clone(), jump_choices: Vec::new(), proxy_arguments: BTreeMap::new(), algorithm_catalog: Default::default(), algorithm_options };
        editor.set_jump_profiles(&config.profiles);
        editor
    }

    pub(crate) fn global_environment(env: &BTreeMap<String, String>) -> Self {
        Self {
            original: None, error: None, tab: 0, focus: 0, tabs: vec![EditorTab::new("Environment variables", env_fields(env).collect())],
            source: Profile::default(), defaults: None, kind: "environment", groups: Vec::new(), global: BTreeMap::new(),
            jump_choices: Vec::new(), proxy_arguments: BTreeMap::new(), algorithm_catalog: Default::default(), algorithm_options: Default::default(),
        }
    }

    pub(crate) fn is_global_environment(&self) -> bool {
        self.kind == "environment"
    }

    pub(crate) fn colors_tab(&self) -> bool {
        self.tabs[self.tab].fields.first().is_some_and(|field| field.id == Field::Scheme)
    }

    pub(crate) fn preview_scheme(&self, config: &Config) -> Option<String> {
        if let Some(name) = self.opt(Field::Scheme) {
            return Some(name);
        }
        if self.defaults.is_some() {
            return None;
        }
        let mut profile = self.source.clone();
        profile.group = self.opt(Field::Group);
        profile.scheme = None;
        crate::profile::effective(&profile, config).scheme
    }

    pub fn title(&self) -> String {
        if self.is_global_environment() {
            t("Environment variables").into()
        } else if self.defaults.is_some() {
            let kind = match self.kind { "ssh" => "SSH", "serial" => t("Serial"), "socket" => t("Network"), _ => t("Local") };
            tf("{type} defaults", &[("type", kind)])
        } else if self.original.is_some() {
            t("Edit profile").into()
        } else {
            t("New profile").into()
        }
    }

    pub fn select_tab(&mut self, index: usize) {
        if index < self.tabs.len() { self.tab = index; self.focus = 0; }
    }

    pub(crate) fn focuses_icon(&self) -> bool {
        self.tabs[self.tab].fields[self.focus].is_icon()
    }

    pub(crate) fn set_icon(&mut self, icon: String) {
        if let Some(field) = self.tabs[0].fields.iter_mut().find(|field| field.is_icon()) {
            field.value = icon;
        }
    }

    pub fn activate(&mut self, index: usize) {
        if let Some(field) = self.tabs[self.tab].fields.get_mut(index) {
            self.focus = index;
            if field.id == Field::AddForward { self.add_forward(); }
            else if !field.editable { self.cycle_field(false); }
        }
    }

    pub fn insert(&mut self, text: &str) {
        let field = &mut self.tabs[self.tab].fields[self.focus];
        if field.editable {
            field.value.extend(text.chars().filter(|c| !c.is_control()));
            self.error = None;
        }
    }

    pub fn key(&mut self, key: Key, mods: Mods) {
        let count = self.tabs.len();
        if key == Key::Tab && mods.ctrl {
            return self.select_tab((self.tab + if mods.shift { count - 1 } else { 1 }) % count);
        }
        let fields = &mut self.tabs[self.tab].fields;
        let len = fields.len();
        let field = &mut fields[self.focus];
        match key {
            Key::Left | Key::Right => self.cycle_field(key == Key::Left),
            Key::Enter | Key::Space if field.id == Field::AddForward => self.add_forward(),
            Key::Space if !field.editable => self.cycle_field(false),
            Key::Tab => self.focus = (self.focus + if mods.shift { len - 1 } else { 1 }) % len,
            Key::Up => self.focus = self.focus.saturating_sub(1),
            Key::Down => self.focus = (self.focus + 1).min(len - 1),
            Key::Enter if matches!(field.id, Field::Env | Field::Identity) => {
                let row = field.id.text("");
                self.focus += 1;
                fields.insert(self.focus, row);
            }
            Key::Enter if field.id.is_step() => {
                let start = step_start(fields, self.focus);
                self.focus = start + 4;
                fields.splice(self.focus..self.focus, step_fields(&LoginStep::default()));
            }
            Key::Delete if field.id.is_step() => {
                let start = step_start(fields, self.focus);
                fields.drain(start..start + 4);
                if fields.iter().all(|f| f.id != Field::Expect) { fields.extend(step_fields(&LoginStep::default())); }
                self.focus = start.min(fields.len() - 1);
            }
            Key::Delete if field.id.is_forward() => self.remove_forward(),
            Key::Delete if matches!(field.id, Field::Env | Field::Identity) => {
                let id = field.id;
                if fields.iter().filter(|f| f.id == id).count() == 1 {
                    fields[self.focus].value.clear();
                } else {
                    fields.remove(self.focus);
                    self.focus = self.focus.min(fields.len() - 1);
                }
            }
            Key::Enter if !field.choices.is_empty() => self.cycle_field(false),
            Key::Enter if field.id == Field::AlgorithmCheck => self.cycle_field(false),
            Key::Enter => self.focus = (self.focus + 1) % len,
            Key::Backspace if field.editable => {
                if mods.ctrl { field.value.clear(); } else { field.value.pop(); }
            }
            _ => {}
        }
        self.error = None;
    }

    fn fields(&self) -> impl Iterator<Item=&EditorField> { self.tabs.iter().flat_map(|tab| &tab.fields) }

    fn value(&self, id: Field) -> &str {
        self.fields().find(|f| f.id == id).map_or("", |f| f.value.trim())
    }

    fn opt(&self, id: Field) -> Option<String> {
        let value = self.value(id);
        (!value.is_empty()).then(|| value.into())
    }

    fn number<T: std::str::FromStr>(&self, id: Field) -> Result<Option<T>, String> {
        self.opt(id).map(|v| v.parse().map_err(|_| tf("Enter a valid number for {field}", &[("field", id.label())]))).transpose()
    }

    fn flag(&self, id: Field) -> Option<bool> { self.value(id).parse().ok() }
}

fn env_fields(env: &BTreeMap<String, String>) -> impl Iterator<Item = EditorField> + '_ {
    env.iter().map(|(k, v)| Field::Env.text(format!("{k}={v}"))).chain(std::iter::once(Field::Env.text("")))
}

fn command_tab(p: &Profile, defaults: bool) -> EditorTab {
    let mut fields = Vec::new();
    if !defaults { fields.push(Field::Command.text(command_line(p))); }
    fields.push(Field::Cwd.text(optional(p.cwd.as_ref())));
    if cfg!(windows) { fields.push(Field::Admin.toggle(p.admin.or(p.elevated.then_some(true)), true)); }
    fields.extend(env_fields(&p.env));
    EditorTab::new("Command", fields)
}

fn step_fields(step: &LoginStep) -> [EditorField; 4] {
    [Field::Expect.text(&step.expect), Field::Send.text(&step.send), Field::Regex.toggle(Some(step.regex), false), Field::Optional.toggle(Some(step.optional), false)]
}

fn step_start(fields: &[EditorField], index: usize) -> usize {
    fields[..=index].iter().rposition(|field| field.id == Field::Expect).expect("login step")
}

fn stream_tab(stream: Option<&Stream>) -> EditorTab {
    let s = stream.cloned().unwrap_or_default();
    let mut fields = vec![
        Field::Input.choice(enum_text(s.input), options(INPUTS, true), false),
        Field::Output.choice(enum_text(s.output), options(OUTPUTS, true), false),
        Field::InLines.choice(enum_text(s.newlines_in), options(NEWLINES, true), false),
        Field::OutLines.choice(enum_text(s.newlines_out), options(NEWLINES, true), false),
        Field::Backspace.choice(enum_text(s.backspace), options(BACKSPACES, true), false),
    ];
    fields.extend(s.login.unwrap_or_default().iter().flat_map(step_fields));
    fields.extend(step_fields(&LoginStep::default()));
    EditorTab::new("Stream", fields)
}

fn serial_tab(p: &Profile, defaults: Option<&ProfileDefaults>, ports: &[String]) -> EditorTab {
    let source = p.serial.as_deref().unwrap_or_default();
    let s = if let Some(defaults) = defaults {
        defaults.serial.clone().unwrap_or_default()
    } else {
        SerialDefaults::from_spec(source).unwrap_or_else(|_| SerialDefaults { port: Some(source.into()), ..SerialDefaults::default() })
    };
    let mut speeds = options(&[("?", "Ask when connecting")], true);
    speeds.extend(serial::BAUDS.iter().map(|baud| (baud.to_string(), baud.to_string())));
    let mut devices = options(&[], true);
    devices.extend(names(ports));
    let mut parities = options(PARITIES, true);
    if cfg!(windows) {
        parities.extend(options(WINDOWS_PARITIES, false));
    }
    let digits = |values: &[&str]| {
        let mut choices = options(&[], true);
        choices.extend(values.iter().map(|v| ((*v).into(), (*v).into())));
        choices
    };
    EditorTab::new("Serial", vec![
        Field::Device.choice(optional(s.port), devices, true),
        Field::Speed.choice(optional(s.speed), speeds, true),
        Field::Bits.choice(optional(s.data_bits), digits(&["5", "6", "7", "8"]), false),
        Field::Parity.choice(enum_text(s.parity), parities, false),
        Field::Stop.choice(optional(s.stop_bits), digits(&["1", "1.5", "2"]), false),
        Field::Flow.choice(enum_text(s.flow), options(FLOWS, true), false),
        Field::Slow.toggle(s.slow, true),
    ])
}

impl ProfileEditor {
    fn serial_options(&self) -> Result<SerialDefaults, String> {
        let invalid = || t("Enter a valid serial port and speed").to_owned();
        let options = SerialDefaults {
            port: self.opt(Field::Device), speed: self.opt(Field::Speed), data_bits: self.number(Field::Bits)?,
            parity: enum_value(self.value(Field::Parity)), stop_bits: self.opt(Field::Stop).map(|text| text.parse().map_err(|_| invalid())).transpose()?,
            flow: enum_value(self.value(Field::Flow)), slow: self.flag(Field::Slow),
        };
        if options.port.as_ref().is_some_and(|p| p.contains(',')) || options.speed.as_ref().is_some_and(|s| s != "?" && s.parse::<u32>().map_or(true, |n| !(50..=4_000_000).contains(&n))) {
            return Err(invalid());
        }
        let validated = SerialDefaults::from_spec(&options.spec()).map_err(|_| invalid())?;
        if validated.port != options.port || validated.speed != options.speed { return Err(invalid()); }
        Ok(options)
    }

    pub(crate) fn environment(&self) -> Result<BTreeMap<String, String>, String> {
        let mut env = BTreeMap::new();
        for field in self.fields().filter(|f| f.id == Field::Env && !f.value.trim().is_empty()) {
            let (name, value) = field.value.split_once('=').filter(|(name, _)| !name.trim().is_empty())
                .ok_or_else(|| t("Enter each environment variable as key=value").to_owned())?;
            if (cfg!(windows) && env.keys().any(|key: &String| key.to_uppercase() == name.trim().to_uppercase()))
                || env.insert(name.trim().into(), value.into()).is_some() {
                return Err(tf("Duplicate environment variable: {name}", &[("name", name.trim())]));
            }
        }
        Ok(env)
    }

    fn ssh_options(&self) -> Result<Option<SshOptions>, String> {
        let old = self.source.ssh_options.as_ref();
        let files: Vec<_> = self.fields().filter(|f| f.id == Field::Identity).map(|f| f.value.trim())
            .filter(|v| !v.is_empty()).map(str::to_owned).collect();
        let identity_files = (!files.is_empty() || old.and_then(|o| o.identity_files.as_ref()).is_some_and(Vec::is_empty)).then_some(files);
        let port = self.number(Field::Port)?;
        if port == Some(0) { return Err(t("SSH port must be between 1 and 65535").into()); }
        let options = SshOptions {
            user: self.ssh_user(), port, identity_files, keepalive_ms: self.number(Field::Keepalive)?,
            proxy: self.ssh_proxy()?, auth: enum_value(self.value(Field::Auth)),
            forwards: self.ssh_forwards()?,
            forward_agent: self.flag(Field::ForwardAgent), reuse_session: self.flag(Field::ReuseSession),
            keepalive_count: self.number(Field::Count)?, ready_timeout_ms: self.number(Field::Timeout)?,
            skip_banner: self.flag(Field::Banner), remember_cwd: self.flag(Field::Remember), x11: self.flag(Field::X11),
            term: self.opt(Field::Term), algorithms: self.ssh_algorithm_options(),
        };
        Ok((options != SshOptions::default()).then_some(options))
    }

    fn stream(&self) -> Option<Stream> {
        let mut steps = Vec::new();
        let mut cells = self.fields().filter(|f| f.id.is_step());
        while let (Some(expect), Some(send), Some(regex), Some(optional)) = (cells.next(), cells.next(), cells.next(), cells.next()) {
            let step = LoginStep { expect: expect.value.clone(), send: send.value.clone(), regex: regex.value == "true", optional: optional.value == "true" };
            if step != LoginStep::default() { steps.push(step); }
        }
        let keep_empty = self.source.stream.as_ref().and_then(|s| s.login.as_ref()).is_some_and(Vec::is_empty);
        let stream = Stream {
            input: enum_value(self.value(Field::Input)), output: enum_value(self.value(Field::Output)),
            newlines_in: enum_value(self.value(Field::InLines)), newlines_out: enum_value(self.value(Field::OutLines)),
            backspace: enum_value(self.value(Field::Backspace)), login: (!steps.is_empty() || keep_empty).then_some(steps),
        };
        (stream != Stream::default()).then_some(stream)
    }

    fn attributes(&self) -> Result<ProfileDefaults, String> {
        let mut d = self.defaults.clone().unwrap_or_else(|| ProfileDefaults {
            cwd: self.source.cwd.clone(), env: self.source.env.clone(), ssh_options: self.source.ssh_options.clone(),
            stream: self.source.stream.clone(), admin: self.source.admin, ..ProfileDefaults::default()
        });
        let scheme = self.value(Field::Scheme);
        d.scheme = if scheme.is_empty() { None } else {
            Some(self.fields().find(|f| f.id == Field::Scheme).and_then(|f| f.choices.iter().find(|(k, _)| k.eq_ignore_ascii_case(scheme)))
                .map(|(k, _)| k.clone()).ok_or_else(|| t("There is no color scheme with this name").to_owned())?)
        };
        d.color = self.opt(Field::Color);
        if d.color != self.source.color && d.color.as_ref().is_some_and(|c| crate::settings::color_value(c).is_none()) {
            return Err(t("Enter a color as #RRGGBB").into());
        }
        d.keep_tab_title = self.flag(Field::KeepTitle);
        d.clear_after_connect = self.flag(Field::ClearAfterConnect);
        d.on_exit = enum_value(self.value(Field::Exit));
        match self.kind {
            "local" => {
                d.cwd = self.opt(Field::Cwd);
                d.env = self.environment()?;
                if cfg!(windows) { d.admin = self.flag(Field::Admin); }
            }
            "ssh" => {
                d.ssh = self.opt(Field::Destination);
                if d.ssh.as_ref().is_some_and(|v| crate::ssh::parse_target(v).is_none_or(|t| t.port == Some(0))) {
                    return Err(t("Enter a valid SSH destination").into());
                }
                d.ssh_options = self.ssh_options()?;
            }
            "serial" => {
                let serial = self.serial_options()?;
                d.serial = (serial != SerialDefaults::default()).then_some(serial);
            }
            "socket" => {
                d.socket = self.opt(Field::Address).map(|v| aster_proto::address::parse(&v).map(|a| a.display())
                    .map_err(|_| t("The network address should look like telnet://host:23 or tcp://host:port").to_owned())).transpose()?;
            }
            _ => {}
        }
        if matches!(self.kind, "ssh" | "serial" | "socket") { d.stream = self.stream(); }
        Ok(d)
    }

    pub fn defaults_result(&self) -> Result<ProfileDefaults, String> { self.attributes() }

    pub fn profile_result(&self) -> Result<Profile, String> {
        let name = self.opt(Field::Name).unwrap_or_default();
        let d = self.attributes()?;
        let mut p = Profile {
            name, group: self.opt(Field::Group), color: d.color, scheme: d.scheme, on_exit: d.on_exit.unwrap_or_default(),
            icon: self.opt(Field::Icon), keep_tab_title: d.keep_tab_title, clear_after_connect: d.clear_after_connect,
            hidden: self.flag(Field::Hidden) == Some(true), builtin: false, elevated: false, admin: d.admin,
            cwd: d.cwd, env: d.env, stream: d.stream, ssh_options: d.ssh_options, ..self.source.clone()
        };
        match self.kind {
            "local" => {
                let command = self.value(Field::Command);
                if command != command_line(&self.source) {
                    let mut words = split_args(command).into_iter();
                    p.command = words.next().unwrap_or_default();
                    p.args = words.collect();
                }
            }
            "ssh" => p.ssh = Some(d.ssh.unwrap_or_default()),
            "serial" => {
                let spec = d.serial.unwrap_or_default();
                let source = self.source.serial.as_deref().unwrap_or_default();
                if SerialDefaults::from_spec(source).ok().as_ref() != Some(&spec) { p.serial = Some(spec.spec()); }
            }
            "socket" => p.socket = Some(d.socket.unwrap_or_default()),
            _ => {}
        }
        let config = Config { groups: self.groups.clone(), profile_defaults: self.global.clone(), ..Config::default() };
        let resolved = crate::profile::effective(&p, &config);
        let required = match self.kind {
            "local" => !p.command.trim().is_empty(),
            "ssh" => resolved.ssh.as_deref().is_some_and(|s| !s.trim().is_empty()),
            "serial" => resolved.serial.as_deref().is_some_and(|s| serial::parse(s).is_ok()),
            "socket" => resolved.socket.as_deref().is_some_and(|s| aster_proto::address::parse(s).is_ok()),
            _ => true,
        };
        if !required { return Err(t("Give a command to run, an SSH destination, a serial port or a network address").into()); }
        if p.name.is_empty() {
            p.name = match self.kind {
                "ssh" => resolved.ssh.as_deref().and_then(crate::ssh::parse_target).map(|mut target| {
                    if let Some(options) = &resolved.ssh_options {
                        target.user = options.user.clone().or(target.user);
                        target.port = options.port.or(target.port);
                    }
                    target.display()
                }).unwrap_or_default(),
                "serial" => resolved.serial.as_deref().and_then(|spec| serial::parse(spec).ok()).map(|settings| settings.port).unwrap_or_default(),
                "socket" => resolved.socket.as_deref().and_then(|address| aster_proto::address::parse(address).ok()).map(|address| address.authority()).unwrap_or_default(),
                "layout" => t("Layout").into(),
                _ => command_line(&resolved),
            };
        }
        if self.kind == "ssh" {
            let previous = self.source.ssh.as_deref().and_then(crate::ssh::parse_target);
            if p.command.is_empty() || previous.is_some_and(|t| t.args() == self.source.args) {
                if p.command.is_empty() { p.command = "ssh".into(); }
                p.args = p.ssh.as_deref().and_then(crate::ssh::parse_target).map(|target| target.args()).unwrap_or_default();
            }
        }
        Ok(p)
    }
}
