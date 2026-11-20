use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Parity {
    #[default]
    None,
    Odd,
    Even,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
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
    for part in parts {
        if settings.slow {
            return Err("The slow flag must appear once at the end of the serial port specification.".to_owned());
        }
        if part.eq_ignore_ascii_case("slow") {
            settings.slow = true;
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
            let (data_bits, parity, stop_bits) = parse_frame(part)?;
            settings.data_bits = data_bits;
            settings.parity = parity;
            settings.stop_bits = stop_bits;
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

pub fn format(settings: &Settings) -> String {
    let parity = match settings.parity {
        Parity::None => 'N',
        Parity::Odd => 'O',
        Parity::Even => 'E',
    };
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

fn parse_frame(text: &str) -> Result<(u8, Parity, u8), String> {
    let error = || {
        format!("Invalid frame \"{text}\": expected data bits 5-8, parity N, O or E and stop bits 1 or 2, like 8N1.")
    };
    let bytes = text.as_bytes();
    if bytes.len() != 3 {
        return Err(error());
    }
    let data_bits = match bytes[0] {
        b @ b'5'..=b'8' => b - b'0',
        _ => return Err(error()),
    };
    let parity = match bytes[1].to_ascii_uppercase() {
        b'N' => Parity::None,
        b'O' => Parity::Odd,
        b'E' => Parity::Even,
        _ => return Err(error()),
    };
    let stop_bits = match bytes[2] {
        b'1' => 1,
        b'2' => 2,
        _ => return Err(error()),
    };
    Ok((data_bits, parity, stop_bits))
}

fn parse_flow(text: &str) -> Option<Flow> {
    match text.to_ascii_lowercase().as_str() {
        "none" => Some(Flow::None),
        "xonxoff" | "software" => Some(Flow::Software),
        "rtscts" | "hardware" => Some(Flow::Hardware),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortInfo {
    pub name: String,
    pub description: String,
}

pub fn ports() -> Vec<PortInfo> {
    let mut ports: Vec<PortInfo> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .map(|info| PortInfo {
            description: describe(&info.port_type),
            name: info.port_name,
        })
        .collect();
    ports.sort_by(|a, b| natural_cmp(&a.name, &b.name));
    ports
}

fn describe(port_type: &serialport::SerialPortType) -> String {
    match port_type {
        serialport::SerialPortType::UsbPort(usb) => [&usb.product, &usb.manufacturer]
            .into_iter()
            .flatten()
            .map(|text| text.trim())
            .find(|text| !text.is_empty())
            .unwrap_or("USB serial")
            .to_owned(),
        serialport::SerialPortType::BluetoothPort => "Bluetooth".to_owned(),
        serialport::SerialPortType::PciPort => "PCI".to_owned(),
        serialport::SerialPortType::Unknown => String::new(),
    }
}

fn natural_cmp(a: &str, b: &str) -> Ordering {
    natural_cmp_folded(a.as_bytes(), b.as_bytes()).then_with(|| a.cmp(b))
}

fn natural_cmp_folded(mut a: &[u8], mut b: &[u8]) -> Ordering {
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (da, ra) = split_digits(a);
                let (db, rb) = split_digits(b);
                let order = compare_numbers(da, db);
                if order != Ordering::Equal {
                    return order;
                }
                a = ra;
                b = rb;
            }
            (Some(x), Some(y)) => {
                let order = x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase());
                if order != Ordering::Equal {
                    return order;
                }
                a = &a[1..];
                b = &b[1..];
            }
        }
    }
}

fn split_digits(text: &[u8]) -> (&[u8], &[u8]) {
    let end = text
        .iter()
        .position(|b| !b.is_ascii_digit())
        .unwrap_or(text.len());
    text.split_at(end)
}

fn compare_numbers(a: &[u8], b: &[u8]) -> Ordering {
    let a_trimmed = trim_zeros(a);
    let b_trimmed = trim_zeros(b);
    a_trimmed
        .len()
        .cmp(&b_trimmed.len())
        .then_with(|| a_trimmed.cmp(b_trimmed))
}

fn trim_zeros(digits: &[u8]) -> &[u8] {
    let start = digits
        .iter()
        .position(|&b| b != b'0')
        .unwrap_or(digits.len());
    &digits[start..]
}

#[cfg(test)]
#[path = "../test/settings.rs"]
mod tests;
