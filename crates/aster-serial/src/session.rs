use crate::settings::{Flow, Parity, Settings};
use crossbeam_channel::{bounded, select, unbounded, Receiver, Sender, TryRecvError};
use std::io::{self, ErrorKind, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const READ_TIMEOUT: Duration = Duration::from_millis(50);
const BREAK_HOLD: Duration = Duration::from_millis(250);
const CHUNK: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Data(Vec<u8>),
    Break,
    SetBaud(u32),
    Close,
}

pub struct Connection {
    pub output: Receiver<Vec<u8>>,
    pub input: Sender<Input>,
    pub exit: Receiver<u32>,
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

pub fn open(settings: &Settings) -> Result<Connection, String> {
    let name = &settings.port;
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
        1 => serialport::StopBits::One,
        2 => serialport::StopBits::Two,
        other => {
            return Err(format!(
                "Cannot open {name}: unsupported stop bits {other}."
            ))
        }
    };
    let parity = match settings.parity {
        Parity::None => serialport::Parity::None,
        Parity::Odd => serialport::Parity::Odd,
        Parity::Even => serialport::Parity::Even,
    };
    let flow = match settings.flow {
        Flow::None => serialport::FlowControl::None,
        Flow::Software => serialport::FlowControl::Software,
        Flow::Hardware => serialport::FlowControl::Hardware,
    };
    let port = serialport::new(name.as_str(), settings.baud)
        .data_bits(data_bits)
        .stop_bits(stop_bits)
        .parity(parity)
        .flow_control(flow)
        .timeout(READ_TIMEOUT)
        .open()
        .map_err(|error| format!("Cannot open {name}: {error}"))?;
    Ok(start(Box::new(SerialDevice(port))))
}

pub fn start(mut port: Box<dyn Port>) -> Connection {
    let (output_tx, output) = unbounded();
    let (input, input_rx) = unbounded();
    let (exit_tx, exit) = bounded(1);
    let connection = Connection {
        output,
        input,
        exit,
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
            let code = write_loop(&mut *port, &input_rx, &alive_rx);
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

fn write_loop(port: &mut dyn Port, input: &Receiver<Input>, alive: &Receiver<()>) -> u32 {
    loop {
        select! {
            recv(input) -> message => match message {
                Ok(Input::Data(data)) => {
                    if write_data(port, &data, alive).is_err() {
                        return 1;
                    }
                }
                Ok(Input::Break) => {
                    let _ = port.send_break();
                }
                Ok(Input::SetBaud(baud)) => {
                    let _ = port.set_baud(baud);
                }
                Ok(Input::Close) | Err(_) => return 0,
            },
            recv(alive) -> _ => return 1,
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
