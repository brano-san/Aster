use std::cmp::Ordering;
pub use aster_proto::serial::{format, parse, Flow, Parity, Settings, StopBits, BAUDS};

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
