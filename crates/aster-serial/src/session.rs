use crate::settings::{Flow, Parity, Settings, StopBits};
use crossbeam_channel::{after, bounded, never, select, unbounded, Receiver, Sender, TryRecvError};
use std::collections::VecDeque;
use std::io::{self, ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const READ_TIMEOUT: Duration = Duration::from_millis(50);
const BREAK_HOLD: Duration = Duration::from_millis(250);
const SLOW_BYTE_DELAY: Duration = Duration::from_millis(2);
const CHUNK: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Data(Vec<u8>),
    Break,
    SetBaud(u32),
    Close,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DriverControl {
    Baud(u32),
    BaudFailed(u32, String),
    BreakFailed(String),
}

pub struct Connection {
    pub output: Receiver<Vec<u8>>,
    pub input: Sender<Input>,
    pub exit: Receiver<u32>,
    pub control: Receiver<DriverControl>,
}

pub trait Port: Read + Write + Send {
    fn set_baud(&mut self, baud: u32) -> io::Result<()>;
    fn send_break(&mut self) -> io::Result<()>;
    fn try_clone_port(&self) -> io::Result<Box<dyn Port>>;
}

struct SerialDevice(Box<dyn serialport::SerialPort>);

impl Read for SerialDevice {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for SerialDevice {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

impl Port for SerialDevice {
    fn set_baud(&mut self, baud: u32) -> io::Result<()> {
        Ok(self.0.set_baud_rate(baud)?)
    }

    fn send_break(&mut self) -> io::Result<()> {
        self.0.set_break()?;
        thread::sleep(BREAK_HOLD);
        Ok(self.0.clear_break()?)
    }

    fn try_clone_port(&self) -> io::Result<Box<dyn Port>> {
        Ok(Box::new(SerialDevice(self.0.try_clone()?)))
    }
}

#[cfg(windows)]
type DeviceHandle = std::os::windows::io::RawHandle;
#[cfg(unix)]
type DeviceHandle = std::os::fd::RawFd;

pub fn open(settings: &Settings, configure: impl FnOnce(DeviceHandle, &Settings) -> io::Result<()>) -> Result<Connection, String> {
    let name = &settings.port;
    if settings.ask_baud {
        return Err("Select a baud rate before opening the serial port.".to_owned());
    }
    if cfg!(windows) && settings.flow == Flow::Xany {
        return Err("XANY flow control is unavailable on this platform.".into());
    }
    if !cfg!(windows) && matches!(settings.parity, Parity::Mark | Parity::Space) {
        return Err("Mark and space parity are unavailable on this platform.".into());
    }
    if !cfg!(windows) && settings.stop_bits == StopBits::OneAndHalf {
        return Err("1.5 stop bits are unavailable on this platform.".into());
    }
    let data_bits = match settings.data_bits {
        5 => serialport::DataBits::Five,
        6 => serialport::DataBits::Six,
        7 => serialport::DataBits::Seven,
        8 => serialport::DataBits::Eight,
        other => {
            return Err(format!(
                "Cannot open {name}: unsupported data bits {other}."
            ))
        }
    };
    let stop_bits = match settings.stop_bits {
        StopBits::One | StopBits::OneAndHalf => serialport::StopBits::One,
        StopBits::Two => serialport::StopBits::Two,
    };
    let parity = match settings.parity {
        Parity::None => serialport::Parity::None,
        Parity::Odd | Parity::Mark | Parity::Space => serialport::Parity::Odd,
        Parity::Even => serialport::Parity::Even,
    };
    let flow = match settings.flow {
        Flow::None => serialport::FlowControl::None,
        Flow::Software | Flow::Xany => serialport::FlowControl::Software,
        Flow::Hardware => serialport::FlowControl::Hardware,
    };
    let port = serialport::new(name.as_str(), settings.baud)
        .data_bits(data_bits)
        .stop_bits(stop_bits)
        .parity(parity)
        .flow_control(flow)
        .timeout(READ_TIMEOUT)
        .open_native()
        .map_err(|error| format!("Cannot open {name}: {error}"))?;
    #[cfg(windows)]
    let handle = std::os::windows::io::AsRawHandle::as_raw_handle(&port);
    #[cfg(unix)]
    let handle = std::os::fd::AsRawFd::as_raw_fd(&port);
    configure(handle, settings).map_err(|error| format!("Cannot open {name}: {error}"))?;
    Ok(start_with_mode(Box::new(SerialDevice(Box::new(port))), settings.slow))
}

pub fn start(port: Box<dyn Port>) -> Connection {
    start_with_mode(port, false)
}

fn start_with_mode(mut port: Box<dyn Port>, slow: bool) -> Connection {
    let (output_tx, output) = unbounded();
    let (input, input_rx) = unbounded();
    let (control_tx, control) = unbounded();
    let (exit_tx, exit) = bounded(1);
    let connection = Connection {
        output,
        input,
        exit,
        control,
    };
    let reader_port = match port.try_clone_port() {
        Ok(reader_port) => reader_port,
        Err(_) => {
            drop(output_tx);
            let _ = exit_tx.send(1);
            return connection;
        }
    };
    let stop = Arc::new(AtomicBool::new(false));
    let (alive_tx, alive_rx) = bounded::<()>(0);
    let reader_stop = stop.clone();
    let reader = thread::Builder::new()
        .name("serial-reader".to_owned())
        .spawn(move || read_loop(reader_port, output_tx, reader_stop, alive_tx));
    let reader = match reader {
        Ok(reader) => reader,
        Err(_) => {
            let _ = exit_tx.send(1);
            return connection;
        }
    };
    let failed_stop = stop.clone();
    let failed_exit = exit_tx.clone();
    let writer = thread::Builder::new()
        .name("serial-writer".to_owned())
        .spawn(move || {
            let code = write_loop(&mut *port, &input_rx, &alive_rx, control_tx, slow);
            stop.store(true, Ordering::SeqCst);
            let _ = reader.join();
            let _ = exit_tx.send(code);
        });
    if writer.is_err() {
        failed_stop.store(true, Ordering::SeqCst);
        let _ = failed_exit.send(1);
    }
    connection
}

fn read_loop(
    mut port: Box<dyn Port>,
    output: Sender<Vec<u8>>,
    stop: Arc<AtomicBool>,
    alive: Sender<()>,
) {
    let mut buf = vec![0u8; CHUNK];
    while !stop.load(Ordering::SeqCst) {
        match port.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if output.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
            Err(error) if is_transient(&error) => {}
            Err(_) => break,
        }
    }
    drop(alive);
}

fn write_loop(port: &mut dyn Port, input: &Receiver<Input>, alive: &Receiver<()>, control: Sender<DriverControl>, slow: bool) -> u32 {
    let mut pending = VecDeque::new();
    let idle = never();
    let mut timer = idle.clone();
    loop {
        select! {
            recv(input) -> message => match message {
                Ok(Input::Data(data)) if slow => {
                    pending.extend(data);
                    if !pending.is_empty() && timer.same_channel(&idle) {
                        timer = after(Duration::ZERO);
                    }
                }
                Ok(Input::Data(data)) => {
                    if write_data(port, &data, alive).is_err() {
                        return 1;
                    }
                }
                Ok(Input::Break) => {
                    if let Err(error) = port.send_break() {
                        let _ = control.send(DriverControl::BreakFailed(error.to_string()));
                    }
                }
                Ok(Input::SetBaud(baud)) => {
                    let event = match port.set_baud(baud) {
                        Ok(()) => DriverControl::Baud(baud),
                        Err(error) => DriverControl::BaudFailed(baud, error.to_string()),
                    };
                    let _ = control.send(event);
                }
                Ok(Input::Close) | Err(_) => return 0,
            },
            recv(alive) -> _ => return 1,
            recv(timer) -> _ => {
                if let Some(byte) = pending.pop_front() {
                    if write_data(port, &[byte], alive).is_err() {
                        return 1;
                    }
                    timer = after(SLOW_BYTE_DELAY);
                } else {
                    timer = idle.clone();
                }
            }
        }
    }
}

fn write_data(port: &mut dyn Port, mut data: &[u8], alive: &Receiver<()>) -> io::Result<()> {
    while !data.is_empty() {
        match port.write(data) {
            Ok(0) => return Err(ErrorKind::WriteZero.into()),
            Ok(n) => data = &data[n..],
            Err(error) if is_transient(&error) => {
                if matches!(alive.try_recv(), Err(TryRecvError::Disconnected)) {
                    return Err(error);
                }
            }
            Err(error) => return Err(error),
        }
    }
    match port.flush() {
        Err(error) if !is_transient(&error) => Err(error),
        _ => Ok(()),
    }
}

fn is_transient(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted
    )
}

#[cfg(test)]
#[path = "../test/session.rs"]
mod tests;
