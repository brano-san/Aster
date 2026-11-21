#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Parity {
    #[default]
    None,
    Odd,
    Even,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flow {
    #[default]
    None,
    Software,
    Hardware,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub port: String,
    pub baud: u32,
    pub ask_baud: bool,
    pub data_bits: u8,
    pub parity: Parity,
    pub stop_bits: u8,
    pub flow: Flow,
    pub slow: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            port: String::new(),
            baud: 115_200,
            ask_baud: false,
            data_bits: 8,
            parity: Parity::None,
            stop_bits: 1,
            flow: Flow::None,
            slow: false,
        }
    }
}

pub const BAUDS: &[u32] = &[
    110, 300, 600, 1200, 2400, 4800, 9600, 14400, 19200, 38400, 57600, 115_200, 230_400, 460_800,
    921_600, 1_000_000, 1_500_000,
];

const MIN_BAUD: u32 = 50;
const MAX_BAUD: u32 = 4_000_000;

pub fn parse(spec: &str) -> Result<Settings, String> {
    let mut parts = spec.split(',').map(str::trim);
    let head = parts.next().unwrap_or_default();
    let mut settings = Settings::default();
    let (port, baud) = split_port(head);
    if port.is_empty() {
        return Err("The serial port name is empty.".to_owned());
    }
    settings.port = port.to_owned();
    if baud == Some("?") {
        settings.ask_baud = true;
    } else if let Some(baud) = baud {
        settings.baud = parse_baud(baud)?;
    }
    let mut frame_seen = false;
    let mut flow_seen = false;
    let mut send_seen = false;
    for part in parts {
        if send_seen {
            return Err("The slow flag must appear once at the end of the serial port specification.".to_owned());
        }
        if part.eq_ignore_ascii_case("slow") || part.eq_ignore_ascii_case("fast") {
            settings.slow = part.eq_ignore_ascii_case("slow");
            send_seen = true;
            continue;
        }
        if flow_seen {
            return Err(format!(
                "Unexpected extra part \"{part}\" after the flow control."
            ));
        }
        if !frame_seen {
            if let Some(flow) = parse_flow(part) {
                settings.flow = flow;
                flow_seen = true;
                continue;
            }
            let frame = parse_frame(part)?;
            settings.data_bits = frame.data_bits.unwrap_or(settings.data_bits);
            settings.parity = frame.parity.unwrap_or(settings.parity);
            settings.stop_bits = frame.stop_bits.unwrap_or(settings.stop_bits);
            frame_seen = true;
            continue;
        }
        settings.flow = parse_flow(part).ok_or_else(|| {
            format!("Unknown flow control \"{part}\": expected none, xonxoff, software, rtscts or hardware.")
        })?;
        flow_seen = true;
    }
    Ok(settings)
}

fn parity_code(parity: Parity) -> char {
    match parity {
        Parity::None => 'N',
        Parity::Odd => 'O',
        Parity::Even => 'E',
    }
}

pub fn format(settings: &Settings) -> String {
    let parity = parity_code(settings.parity);
    let flow = match settings.flow {
        Flow::None => "",
        Flow::Software => ",xonxoff",
        Flow::Hardware => ",rtscts",
    };
    let baud = if settings.ask_baud { "?".to_owned() } else { settings.baud.to_string() };
    let slow = if settings.slow { ",slow" } else { "" };
    format!("{}:{baud},{}{parity}{}{flow}{slow}", settings.port, settings.data_bits, settings.stop_bits)
}

fn split_port(head: &str) -> (&str, Option<&str>) {
    if let Some((port, baud)) = head.rsplit_once(':') {
        let baud = baud.trim();
        if baud == "?" || (!baud.is_empty() && baud.bytes().all(|b| b.is_ascii_digit())) {
            return (port.trim(), Some(baud));
        }
    }
    (head, None)
}

fn parse_baud(text: &str) -> Result<u32, String> {
    let error = || {
        format!("Invalid baud rate \"{text}\": expected a number from {MIN_BAUD} to {MAX_BAUD}.")
    };
    let baud: u32 = text.parse().map_err(|_| error())?;
    if (MIN_BAUD..=MAX_BAUD).contains(&baud) {
        Ok(baud)
    } else {
        Err(error())
    }
}

fn parse_frame(text: &str) -> Result<Defaults, String> {
    let error = || {
        format!("Invalid frame \"{text}\": expected data bits 5-8, parity N, O or E and stop bits 1 or 2, like 8N1.")
    };
    let bytes = text.as_bytes();
    if bytes.len() != 3 {
        return Err(error());
    }
    let data_bits = match bytes[0] {
        b'?' => None,
        b @ b'5'..=b'8' => Some(b - b'0'),
        _ => return Err(error()),
    };
    let parity = match bytes[1].to_ascii_uppercase() {
        b'?' => None,
        b'N' => Some(Parity::None),
        b'O' => Some(Parity::Odd),
        b'E' => Some(Parity::Even),
        _ => return Err(error()),
    };
    let stop_bits = match bytes[2] {
        b'?' => None,
        b'1' => Some(1),
        b'2' => Some(2),
        _ => return Err(error()),
    };
    Ok(Defaults { data_bits, parity, stop_bits, ..Defaults::default() })
}

fn parse_flow(text: &str) -> Option<Flow> {
    match text.to_ascii_lowercase().as_str() {
        "none" => Some(Flow::None),
        "xonxoff" | "software" => Some(Flow::Software),
        "rtscts" | "hardware" => Some(Flow::Hardware),
        _ => None,
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Defaults {
    pub port: Option<String>,
    pub speed: Option<String>,
    pub data_bits: Option<u8>,
    pub parity: Option<Parity>,
    pub stop_bits: Option<u8>,
    pub flow: Option<Flow>,
    pub slow: Option<bool>,
}

impl Defaults {
    pub fn inherit(&mut self, other: &Self) {
        if self.port.is_none() { self.port.clone_from(&other.port); }
        if self.speed.is_none() { self.speed.clone_from(&other.speed); }
        self.data_bits = self.data_bits.or(other.data_bits);
        self.parity = self.parity.or(other.parity);
        self.stop_bits = self.stop_bits.or(other.stop_bits);
        self.flow = self.flow.or(other.flow);
        self.slow = self.slow.or(other.slow);
    }

    pub fn resolve(&self, spec: &str) -> Option<String> {
        if self == &Self::default() { return None; }
        let mut options = Self::from_spec(spec).ok()?;
        options.inherit(self);
        parse(&options.spec()).ok().map(|settings| format(&settings))
    }

    pub fn from_spec(spec: &str) -> Result<Self, String> {
        let mut parts = spec.split(',').map(str::trim);
        let (port, speed) = split_port(parts.next().unwrap_or_default());
        let mut output = Self { port: (!port.is_empty()).then(|| port.into()), speed: speed.map(str::to_owned), ..Self::default() };
        let validation = if port.is_empty() { format!("port{spec}") } else { spec.into() };
        parse(&validation)?;
        for part in parts {
            if let Ok(frame) = parse_frame(part) {
                output.inherit(&frame);
            } else if let Some(flow) = parse_flow(part) {
                output.flow = Some(flow);
            } else {
                output.slow = Some(part.eq_ignore_ascii_case("slow"));
            }
        }
        Ok(output)
    }

    pub fn spec(&self) -> String {
        let mut spec = self.port.clone().unwrap_or_default();
        if let Some(speed) = &self.speed { spec.push(':'); spec.push_str(speed); }
        if self.data_bits.is_some() || self.parity.is_some() || self.stop_bits.is_some() {
            let digit = |bits: Option<u8>| bits.map_or_else(|| "?".into(), |n| n.to_string());
            spec.push(',');
            spec.push_str(&digit(self.data_bits));
            spec.push(self.parity.map_or('?', parity_code));
            spec.push_str(&digit(self.stop_bits));
        }
        if let Some(flow) = self.flow {
            spec.push_str(match flow { Flow::None => ",none", Flow::Software => ",xonxoff", Flow::Hardware => ",rtscts" });
        }
        if let Some(slow) = self.slow { spec.push_str(if slow { ",slow" } else { ",fast" }); }
        spec
    }
}
