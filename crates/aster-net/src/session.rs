use aster_proto::address::{Address, Protocol};
use aster_proto::input::{InputMode, InputProcessor};
use aster_proto::newline::{NewlineConverter, Newlines};
use aster_proto::telnet::{Telnet, TelnetEvent};
use crossbeam_channel::{bounded, select, unbounded, Receiver, Sender};
use std::io::{self, ErrorKind, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CHUNK: usize = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Data(Vec<u8>),
    Raw(Vec<u8>),
    Resize(u16, u16),
    Close,
}

pub struct Connection {
    pub output: Receiver<Vec<u8>>,
    pub input: Sender<Input>,
    pub exit: Receiver<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub term: String,
    pub cols: u16,
    pub rows: u16,
    pub echo: bool,
}

struct Shared {
    protocol: Protocol,
    telnet: Mutex<Telnet>,
    remote_echo: AtomicBool,
    echo: bool,
    output: Sender<Vec<u8>>,
}

impl Shared {
    fn print(&self, text: String) {
        let _ = self.output.send(text.into_bytes());
    }
}

pub fn open(address: Address, options: Options) -> Connection {
    let (output_tx, output) = unbounded();
    let (input, input_rx) = unbounded();
    let (exit_tx, exit) = bounded(1);
    let fallback = exit_tx.clone();
    let spawned = thread::Builder::new().name("net-session".to_owned()).spawn(move || {
        let shared = Arc::new(Shared {
            protocol: address.protocol,
            telnet: Mutex::new(Telnet::new(&options.term, options.cols, options.rows)),
            remote_echo: AtomicBool::new(false),
            echo: options.echo,
            output: output_tx,
        });
        let code = match connect(&address) {
            Ok(stream) => run(stream, &shared, &input_rx),
            Err(error) => {
                shared.print(format!("Cannot connect to {}: {error}\r\n", address.authority()));
                1
            }
        };
        drop(shared);
        let _ = exit_tx.send(code);
    });
    if spawned.is_err() {
        let _ = fallback.send(1);
    }
    Connection { output, input, exit }
}

fn connect(address: &Address) -> io::Result<TcpStream> {
    let mut last = io::Error::new(ErrorKind::NotFound, "the host name did not resolve");
    for target in (address.host.as_str(), address.port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&target, CONNECT_TIMEOUT) {
            Ok(stream) => {
                let _ = stream.set_nodelay(true);
                return Ok(stream);
            }
            Err(error) => last = error,
        }
    }
    Err(last)
}

fn run(stream: TcpStream, shared: &Arc<Shared>, input: &Receiver<Input>) -> u32 {
    let mut reader = match stream.try_clone() {
        Ok(reader) => reader,
        Err(error) => {
            shared.print(format!("Cannot use the connection: {error}\r\n"));
            return 1;
        }
    };
    let (wire_tx, wire) = unbounded::<Vec<u8>>();
    let (alive_tx, alive) = bounded::<()>(0);
    let reading = shared.clone();
    let reader = thread::Builder::new().name("net-reader".to_owned()).spawn(move || {
        let code = read_loop(&mut reader, &reading, &wire_tx);
        drop(alive_tx);
        code
    });
    let Ok(reader) = reader else { return 1 };
    let mut writer = stream;
    let written = write_loop(&mut writer, shared, input, &wire, &alive);
    let _ = writer.shutdown(Shutdown::Both);
    let read = reader.join().unwrap_or(1);
    written.unwrap_or(read)
}

fn read_loop(stream: &mut TcpStream, shared: &Shared, wire: &Sender<Vec<u8>>) -> u32 {
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = match stream.read(&mut buf) {
            Ok(0) => return 0,
            Ok(n) => n,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) if matches!(error.kind(), ErrorKind::ConnectionAborted | ErrorKind::NotConnected) => return 0,
            Err(error) => {
                shared.print(format!("\r\nConnection lost: {error}\r\n"));
                return 1;
            }
        };
        let bytes = &buf[..n];
        if shared.protocol == Protocol::Raw {
            let _ = shared.output.send(bytes.to_vec());
            continue;
        }
        let events = match shared.telnet.lock() {
            Ok(mut telnet) => telnet.receive(bytes),
            Err(_) => return 1,
        };
        for event in &events {
            if let TelnetEvent::RemoteEcho(on) = event {
                shared.remote_echo.store(*on, Ordering::SeqCst);
            }
        }
        for event in events {
            match event {
                TelnetEvent::Data(data) => {
                    let _ = shared.output.send(data);
                }
                TelnetEvent::Send(reply) => {
                    let _ = wire.send(reply);
                }
                TelnetEvent::RemoteEcho(_) => {}
            }
        }
    }
}

fn write_loop(stream: &mut TcpStream, shared: &Shared, input: &Receiver<Input>, wire: &Receiver<Vec<u8>>, alive: &Receiver<()>) -> Option<u32> {
    let mut echo = InputProcessor::new(InputMode::LocalEcho);
    let mut enter = NewlineConverter::new(Newlines::CrLf);
    let telnet = shared.protocol == Protocol::Telnet;
    let send = |stream: &mut TcpStream, bytes: &[u8]| {
        let sent = stream.write_all(bytes).and_then(|()| stream.flush());
        if let Err(error) = &sent {
            shared.print(format!("\r\nConnection lost: {error}\r\n"));
        }
        sent.is_ok()
    };
    loop {
        select! {
            recv(input) -> message => match message {
                Ok(Input::Data(keys)) => {
                    if shared.echo && !shared.remote_echo.load(Ordering::SeqCst) {
                        let shown = echo.feed(&keys).echo;
                        if !shown.is_empty() {
                            let _ = shared.output.send(shown);
                        }
                    }
                    let mut bytes = enter.convert(&keys);
                    if telnet {
                        bytes = Telnet::escape_outgoing(&bytes);
                    }
                    if !send(stream, &bytes) {
                        return Some(1);
                    }
                }
                Ok(Input::Raw(bytes)) => {
                    let bytes = if telnet { Telnet::escape_outgoing(&bytes) } else { bytes };
                    if !send(stream, &bytes) {
                        return Some(1);
                    }
                }
                Ok(Input::Resize(cols, rows)) => {
                    let packet = shared.telnet.lock().ok().and_then(|mut t| t.resize(cols, rows));
                    if let Some(packet) = packet.filter(|_| telnet) {
                        if !send(stream, &packet) {
                            return Some(1);
                        }
                    }
                }
                Ok(Input::Close) | Err(_) => return Some(0),
            },
            recv(wire) -> reply => match reply {
                Ok(reply) => {
                    if !send(stream, &reply) {
                        return Some(1);
                    }
                }
                Err(_) => return None,
            },
            recv(alive) -> _ => return None,
        }
    }
}

#[cfg(test)]
#[path = "../test/session.rs"]
mod tests;
