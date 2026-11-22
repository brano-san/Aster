use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc::UnboundedSender;
use crate::socks::{Handshake, Step};

pub use aster_proto::ssh_forward::{parse, Forward};

pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}

pub(crate) async fn pump(a: &mut (impl AsyncRead + AsyncWrite + Unpin), b: &mut (impl AsyncRead + AsyncWrite + Unpin)) -> std::io::Result<()> {
    tokio::io::copy_bidirectional(a, b).await.map(|_| ())
}
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
            let _ = pump(&mut socket, &mut stream).await;
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
