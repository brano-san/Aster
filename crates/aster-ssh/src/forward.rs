use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::UnboundedSender;
use crate::socks::{Handshake, Step};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Forward {
    Local { bind: String, port: u16, host: String, host_port: u16 },
    Remote { bind: String, port: u16, host: String, host_port: u16 },
    Dynamic { bind: String, port: u16 },
}

fn fields(value: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new(); let mut field = String::new(); let mut bracket = false; let mut closed = false;
    for c in value.chars() {
        match c {
            '[' if field.is_empty() && !bracket && !closed => bracket = true,
            ']' if bracket => { bracket = false; closed = true; }
            ':' if !bracket => { fields.push(std::mem::take(&mut field)); closed = false; }
            '[' | ']' => return Err("invalid address brackets".into()),
            _ if closed => return Err("unexpected text after address".into()),
            _ => field.push(c),
        }
    }
    if bracket { return Err("unclosed address bracket".into()); }
    fields.push(field);
    if fields.iter().any(String::is_empty) { return Err("missing address or port".into()); }
    Ok(fields)
}

fn port(value: &str, zero: bool) -> Result<u16, String> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) { return Err("port must be a decimal number".into()); }
    let port = value.parse::<u16>().map_err(|_| "port is outside 0..65535".to_owned())?;
    if port == 0 && !zero { return Err("destination port must be greater than zero".into()); }
    Ok(port)
}

pub fn parse(kind: &str, spec: &str) -> Result<Forward, String> {
    let words: Vec<_> = spec.split_whitespace().collect();
    let kind = kind.to_ascii_lowercase();
    if !matches!(kind.as_str(), "local" | "remote" | "dynamic") { return Err("unknown forward kind".into()); }
    let pieces = match words.as_slice() {
        [one] => fields(one)?,
        [left, right] if kind != "dynamic" => {
            let mut a = fields(left)?; let b = fields(right)?;
            if !(1..=2).contains(&a.len()) || b.len() != 2 { return Err("expected bind port and destination host:port".into()); }
            a.extend(b); a
        }
        _ => return Err("invalid forward specification".into()),
    };
    let needed = if kind == "dynamic" { 1 } else { 3 };
    if pieces.len() != needed && pieces.len() != needed + 1 { return Err("missing or extra forward fields".into()); }
    let offset = pieces.len() - needed;
    let bind = if offset == 0 { "127.0.0.1" } else if pieces[0] == "*" { "0.0.0.0" } else { &pieces[0] }.to_owned();
    let port = port(&pieces[offset], true)?;
    if kind == "dynamic" { return Ok(Forward::Dynamic { bind, port }); }
    let host = pieces[offset + 1].clone(); let host_port = self::port(&pieces[offset + 2], false)?;
    Ok(if kind == "local" { Forward::Local { bind, port, host, host_port } } else { Forward::Remote { bind, port, host, host_port } })
}

pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
pub type Opened = Pin<Box<dyn Future<Output = Result<Box<dyn Stream>, String>> + Send>>;
pub trait Opener: Send + Sync + 'static { fn open(&self, host: String, port: u16) -> Opened; }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Listening { forward: Forward, port: u16 },
    Failed { forward: Forward, error: String },
    Opened { forward: Forward, host: String, port: u16 },
    Refused { forward: Forward, host: String, port: u16, error: String },
}

pub struct Running { listener: Arc<Mutex<Option<TcpListener>>>, wake: Arc<tokio::sync::Notify> }
impl Running {
    pub fn stop(&self) { self.listener.lock().unwrap().take(); self.wake.notify_one(); }
}
impl Drop for Running { fn drop(&mut self) { self.stop(); } }

pub async fn start(forward: Forward, opener: Arc<dyn Opener>, events: UnboundedSender<Event>) -> Running {
    let running = Running { listener: Arc::new(Mutex::new(None)), wake: Arc::new(tokio::sync::Notify::new()) };
    let (bind, port) = match &forward {
        Forward::Local { bind, port, .. } | Forward::Dynamic { bind, port } => (bind, *port),
        Forward::Remote { .. } => { let _ = events.send(Event::Failed { forward, error: "remote forwards are started by the session".into() }); return running; }
    };
    let listener = match TcpListener::bind((bind.as_str(), port)).await {
        Ok(listener) => listener,
        Err(error) => { let _ = events.send(Event::Failed { forward, error: error.to_string() }); return running; }
    };
    let port = match listener.local_addr() {
        Ok(address) => address.port(),
        Err(error) => { let _ = events.send(Event::Failed { forward, error: error.to_string() }); return running; }
    };
    *running.listener.lock().unwrap() = Some(listener);
    let _ = events.send(Event::Listening { forward: forward.clone(), port });
    let shared = running.listener.clone(); let wake = running.wake.clone();
    tokio::spawn(async move {
        loop {
            let accept = std::future::poll_fn(|cx| {
                let guard = shared.lock().unwrap();
                match guard.as_ref() {
                    Some(listener) => listener.poll_accept(cx).map(Some),
                    None => std::task::Poll::Ready(None),
                }
            });
            let accepted = tokio::select! { biased; _ = wake.notified() => break, accepted = accept => accepted };
            match accepted {
                Some(Ok((socket, _))) => {
                    let forward = forward.clone(); let opener = opener.clone(); let events = events.clone();
                    tokio::spawn(async move { handle(socket, forward, opener, events).await; });
                }
                Some(Err(error)) => { let _ = events.send(Event::Failed { forward: forward.clone(), error: error.to_string() }); break; }
                None => break,
            }
        }
        shared.lock().unwrap().take();
    });
    running
}

async fn negotiate(socket: &mut TcpStream) -> std::io::Result<Option<(Handshake, String, u16)>> {
    let mut handshake = Handshake::new(); let mut bytes = [0; 2048];
    loop {
        let n = socket.read(&mut bytes).await?;
        if n == 0 { return Ok(None); }
        for step in handshake.feed(&bytes[..n]) {
            match step {
                Step::Reply(reply) => socket.write_all(&reply).await?,
                Step::Fail(reply) => { socket.write_all(&reply).await?; return Ok(None); }
                Step::Connect { host, port } => return Ok(Some((handshake, host, port))),
                Step::More => {}
            }
        }
    }
}

async fn handle(mut socket: TcpStream, forward: Forward, opener: Arc<dyn Opener>, events: UnboundedSender<Event>) {
    let (mut handshake, host, port) = match &forward {
        Forward::Local { host, host_port, .. } => (None, host.clone(), *host_port),
        Forward::Dynamic { .. } => {
            match tokio::time::timeout(std::time::Duration::from_secs(10), negotiate(&mut socket)).await {
                Ok(Ok(Some((handshake, host, port)))) => (Some(handshake), host, port),
                _ => return,
            }
        }
        Forward::Remote { .. } => return,
    };
    match opener.open(host.clone(), port).await {
        Ok(mut stream) => {
            let _ = events.send(Event::Opened { forward, host, port });
            if let Some(handshake) = &mut handshake {
                if socket.write_all(&handshake.success()).await.is_err() || stream.write_all(&handshake.rest()).await.is_err() { return; }
            }
            let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
        }
        Err(error) => {
            let _ = events.send(Event::Refused { forward, host, port, error });
            if let Some(handshake) = handshake { let _ = socket.write_all(&handshake.failure()).await; }
        }
    }
}

#[cfg(test)]
#[path = "../test/forward.rs"]
mod tests;
