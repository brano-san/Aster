use crate::{PlatformError, Pty, SpawnRequest, Spawned};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};

pub const HELLO: u8 = 0;
pub const SPAWN: u8 = 1;
pub const READY: u8 = 2;
pub const DATA: u8 = 3;
pub const RESIZE: u8 = 4;
pub const KILL: u8 = 5;
pub const EXIT: u8 = 6;
pub const ERROR: u8 = 7;

const MAX_FRAME: usize = 16 * 1024 * 1024;

pub type Reader = Box<dyn Read + Send>;
pub type Writer = Box<dyn Write + Send>;

pub fn write_frame(w: &mut dyn Write, kind: u8, payload: &[u8]) -> io::Result<()> {
    let mut frame = Vec::with_capacity(5 + payload.len());
    frame.push(kind);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(payload);
    w.write_all(&frame)?;
    w.flush()
}

pub fn read_frame(r: &mut dyn Read) -> io::Result<Option<(u8, Vec<u8>)>> {
    let mut head = [0u8; 5];
    let mut got = 0;
    while got < head.len() {
        let n = r.read(&mut head[got..])?;
        if n == 0 {
            return if got == 0 { Ok(None) } else { Err(io::ErrorKind::UnexpectedEof.into()) };
        }
        got += n;
    }
    let len = u32::from_le_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    Ok(Some((head[0], payload)))
}

fn put(out: &mut Vec<u8>, text: &str) {
    out.extend_from_slice(&(text.len() as u32).to_le_bytes());
    out.extend_from_slice(text.as_bytes());
}

fn take(input: &mut &[u8]) -> Option<String> {
    let len = u32::from_le_bytes(input.get(..4)?.try_into().ok()?) as usize;
    let text = input.get(4..4 + len)?;
    let value = String::from_utf8(text.to_vec()).ok()?;
    *input = &input[4 + len..];
    Some(value)
}

fn put_env<'a>(out: &mut Vec<u8>, values: impl ExactSizeIterator<Item = (&'a String, &'a String)>) {
    out.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for (key, value) in values {
        put(out, key);
        put(out, value);
    }
}

pub fn encode_request(req: &SpawnRequest) -> Vec<u8> {
    let mut out = Vec::new();
    put(&mut out, &req.command);
    out.extend_from_slice(&(req.args.len() as u32).to_le_bytes());
    for a in &req.args {
        put(&mut out, a);
    }
    put(&mut out, &req.cwd.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default());
    put_env(&mut out, req.env.iter().map(|(key, value)| (key, value)));
    out.extend_from_slice(&req.cols.to_le_bytes());
    out.extend_from_slice(&req.rows.to_le_bytes());
    if let Some(environment) = &req.local_environment {
        out.push(u8::from(environment.refresh) | (u8::from(environment.set_comspec) << 1));
        put_env(&mut out, environment.global.iter());
        put_env(&mut out, environment.profile.iter());
    }
    out
}

pub fn decode_request(mut input: &[u8]) -> Option<SpawnRequest> {
    let input = &mut input;
    let count = |input: &mut &[u8]| -> Option<usize> {
        let n = u32::from_le_bytes(input.get(..4)?.try_into().ok()?) as usize;
        *input = &input[4..];
        Some(n)
    };
    let command = take(input)?;
    let args = (0..count(input)?).map(|_| take(input)).collect::<Option<Vec<_>>>()?;
    let cwd = take(input)?;
    let env = (0..count(input)?).map(|_| Some((take(input)?, take(input)?))).collect::<Option<Vec<_>>>()?;
    let cols = u16::from_le_bytes(input.get(..2)?.try_into().ok()?);
    let rows = u16::from_le_bytes(input.get(2..4)?.try_into().ok()?);
    let mut extra = input.get(4..)?;
    let local_environment = match extra.split_first() {
        None => None,
        Some((&flags, rest)) if flags <= 3 => {
            extra = rest;
            let global = (0..count(&mut extra)?).map(|_| Some((take(&mut extra)?, take(&mut extra)?))).collect::<Option<_>>()?;
            let profile = (0..count(&mut extra)?).map(|_| Some((take(&mut extra)?, take(&mut extra)?))).collect::<Option<_>>()?;
            if !extra.is_empty() { return None; }
            Some(Box::new(aster_core::environment::LocalEnvironment { global, profile, refresh: flags & 1 != 0, set_comspec: flags & 2 != 0 }))
        }
        _ => return None,
    };
    Some(SpawnRequest { command, args, cwd: (!cwd.is_empty()).then(|| PathBuf::from(cwd)), env, local_environment, cols, rows })
}

type Shared = Arc<Mutex<Writer>>;

fn send(writer: &Shared, kind: u8, payload: &[u8]) -> io::Result<()> {
    let mut w = writer.lock().map_err(|_| io::Error::other("writer poisoned"))?;
    write_frame(&mut **w, kind, payload)
}

struct Output {
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    at: usize,
}

impl Read for Output {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        while self.at >= self.buf.len() {
            match self.rx.recv() {
                Ok(bytes) => {
                    self.buf = bytes;
                    self.at = 0;
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.buf.len() - self.at);
        out[..n].copy_from_slice(&self.buf[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
}

struct Keys(Shared);

impl Write for Keys {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        send(&self.0, DATA, buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Remote {
    writer: Shared,
    pid: u32,
}

impl Pty for Remote {
    fn resize(&self, cols: u16, rows: u16) {
        let mut payload = cols.to_le_bytes().to_vec();
        payload.extend_from_slice(&rows.to_le_bytes());
        let _ = send(&self.writer, RESIZE, &payload);
    }

    fn kill(&self) {
        let _ = send(&self.writer, KILL, &[]);
    }

    fn pid(&self) -> u32 {
        self.pid
    }
}

pub fn client(mut reader: Reader, writer: Writer, token: &str, req: &SpawnRequest) -> Result<Spawned, PlatformError> {
    let fail = |e: io::Error| PlatformError::new(-1, format!("the terminal host is not responding: {e}"));
    let writer: Shared = Arc::new(Mutex::new(writer));
    send(&writer, HELLO, token.as_bytes()).map_err(fail)?;
    send(&writer, SPAWN, &encode_request(req)).map_err(fail)?;
    let pid = match read_frame(&mut *reader).map_err(fail)? {
        Some((READY, payload)) => payload.get(..4).map_or(0, |p| u32::from_le_bytes(p.try_into().unwrap_or_default())),
        Some((ERROR, payload)) => return Err(PlatformError::new(-1, String::from_utf8_lossy(&payload).into_owned())),
        _ => return Err(PlatformError::new(-1, "the terminal host closed the connection")),
    };
    let (out_tx, out_rx) = channel();
    let (exit_tx, exit_rx) = channel();
    std::thread::Builder::new()
        .name("aster-host-client".into())
        .spawn(move || {
            let mut code = None;
            while let Ok(Some((kind, payload))) = read_frame(&mut *reader) {
                match kind {
                    DATA => {
                        if out_tx.send(payload).is_err() {
                            break;
                        }
                    }
                    EXIT => {
                        code = payload.get(..4).map(|p| u32::from_le_bytes(p.try_into().unwrap_or_default())).filter(|c| *c != u32::MAX);
                        break;
                    }
                    _ => {}
                }
            }
            drop(out_tx);
            let _ = exit_tx.send(code);
        })
        .map_err(|e| PlatformError::new(-1, e.to_string()))?;
    Ok(Spawned {
        pty: Box::new(Remote { writer: writer.clone(), pid }),
        reader: Box::new(Output { rx: out_rx, buf: Vec::new(), at: 0 }),
        writer: Box::new(Keys(writer)),
        raw: None,
        wait: Box::new(move || exit_rx.recv().ok().flatten()),
    })
}

pub fn serve(mut reader: Reader, writer: Writer, token: &str, spawn: &dyn Fn(&SpawnRequest) -> Result<Spawned, PlatformError>) -> io::Result<()> {
    let writer: Shared = Arc::new(Mutex::new(writer));
    match read_frame(&mut *reader)? {
        Some((HELLO, given)) if given == token.as_bytes() => {}
        _ => return Err(io::Error::new(io::ErrorKind::PermissionDenied, "wrong host token")),
    }
    let req = match read_frame(&mut *reader)? {
        Some((SPAWN, payload)) => decode_request(&payload).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad spawn request"))?,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "expected a spawn request")),
    };
    let Spawned { pty, reader: mut child_out, writer: mut child_in, wait, .. } = match spawn(&req) {
        Ok(s) => s,
        Err(e) => {
            send(&writer, ERROR, e.message.as_bytes())?;
            return Ok(());
        }
    };
    send(&writer, READY, &pty.pid().to_le_bytes())?;
    let pump = writer.clone();
    let (drained, drained_rx) = channel::<()>();
    std::thread::Builder::new().name("aster-host-output".into()).spawn(move || {
        let _drained = drained;
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            match child_out.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if send(&pump, DATA, &buf[..n]).is_err() {
                        break;
                    }
                }
            }
        }
    })?;
    let ender = writer.clone();
    std::thread::Builder::new().name("aster-host-wait".into()).spawn(move || {
        let code = wait();
        let _ = drained_rx.recv_timeout(std::time::Duration::from_secs(2));
        let _ = send(&ender, EXIT, &code.unwrap_or(u32::MAX).to_le_bytes());
    })?;
    loop {
        match read_frame(&mut *reader) {
            Ok(Some((DATA, payload))) => {
                if child_in.write_all(&payload).and_then(|_| child_in.flush()).is_err() {
                    break;
                }
            }
            Ok(Some((RESIZE, payload))) if payload.len() == 4 => {
                pty.resize(u16::from_le_bytes([payload[0], payload[1]]), u16::from_le_bytes([payload[2], payload[3]]));
            }
            Ok(Some((KILL, _))) => pty.kill(),
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                pty.kill();
                break;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "../test/host.rs"]
mod tests;
