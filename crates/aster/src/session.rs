use crate::zmodem::{Note, Outcome, Transfer};
use aster_core::app::{Shared, Wake};
use aster_core::panes::PaneId;
use aster_platform::{Pty, Spawned, WindowId};
use crossbeam_channel::{bounded, unbounded, Sender};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PUMP_QUEUE: usize = 64;
const FEED_SLICE: usize = 8 * 1024;
const RAW_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Default)]
pub struct RawOutput(Arc<Mutex<VecDeque<u8>>>);

impl RawOutput {
    fn append(&self, bytes: &[u8]) {
        let bytes = &bytes[bytes.len().saturating_sub(RAW_LIMIT)..];
        let mut ring = self.0.lock().unwrap();
        let discard = (ring.len() + bytes.len()).saturating_sub(RAW_LIMIT);
        ring.drain(..discard);
        ring.extend(bytes);
    }

    fn snapshot(&self) -> Vec<u8> {
        let ring = self.0.lock().unwrap();
        let (head, tail) = ring.as_slices();
        [head, tail].concat()
    }

    pub fn save(&self, pane: PaneId, downloads: impl FnOnce() -> Option<PathBuf> + Send + 'static, terminal: Shared, wake: Wake) {
        let bytes = self.snapshot();
        let report = move |message: String| {
            let clean: String = message.chars().filter(|c| !c.is_control()).collect();
            terminal.lock().feed(format!("\r\n\x1b[2m[{clean}]\x1b[0m\r\n").as_bytes());
            wake();
        };
        let notify = report.clone();
        let worker = std::thread::Builder::new().name(format!("aster-save-output-{pane}")).spawn(move || {
            let result = downloads().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Downloads folder unavailable"))
                .and_then(|dir| write_output(&dir, pane, &bytes));
            notify(match result {
                Ok(path) => format!("saved to {}", path.display()),
                Err(e) => format!("cannot save output: {e}"),
            });
        });
        if let Err(e) = worker {
            report(format!("cannot save output: {e}"));
        }
    }
}

fn write_output(dir: &Path, pane: PaneId, bytes: &[u8]) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut time = SystemTime::now().duration_since(UNIX_EPOCH).map_err(io::Error::other)?.as_nanos();
    loop {
        let path = dir.join(format!("aster-output-{pane}-{time}.bin"));
        let mut file = match std::fs::File::create_new(&path) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                time += 1;
                continue;
            }
            result => result?,
        };
        if let Err(e) = file.write_all(bytes) {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
        return Ok(path);
    }
}

#[derive(Default)]
struct Pacer {
    next: Option<Instant>,
}

impl Pacer {
    fn delay(&self, now: Instant) -> Duration {
        self.next.map_or(Duration::ZERO, |next| next.saturating_duration_since(now))
    }

    fn woke(&mut self, now: Instant) {
        self.next = Some(now + Duration::from_millis(8));
    }

    fn should_yield(now: Instant, drawn: Instant) -> bool {
        now.saturating_duration_since(drawn) >= Duration::from_millis(100)
    }
}

#[derive(Clone)]
pub struct OutputFlow {
    pub wake: Wake,
    drawn: Arc<Mutex<Instant>>,
}

impl OutputFlow {
    pub fn new(wake: impl Fn() + Send + 'static) -> Self {
        let (tx, rx) = bounded(1);
        std::thread::Builder::new().name("aster-output-wake".into()).spawn(move || {
            let mut pacer = Pacer::default();
            while rx.recv().is_ok() {
                let delay = pacer.delay(Instant::now());
                if !delay.is_zero() {
                    std::thread::sleep(delay);
                    let _ = rx.try_recv();
                }
                pacer.woke(Instant::now());
                wake();
            }
        }).expect("output wake thread");
        Self { wake: Arc::new(move || { let _ = tx.try_send(()); }), drawn: Arc::new(Mutex::new(Instant::now())) }
    }

    pub fn frame_drawn(&self) {
        *self.drawn.lock().unwrap() = Instant::now();
    }
}

pub struct Session {
    pub pty: Box<dyn Pty>,
    writer: Sender<Out>,
    ctx: Ctx,
}

enum Out {
    Keys(Vec<u8>),
    Transfer(Vec<u8>),
}

pub type Exit = (WindowId, PaneId, Option<u32>);
pub type TransferNote = (WindowId, PaneId, Note);

#[derive(Clone)]
struct Ctx {
    window: WindowId,
    pane: PaneId,
    terminal: Shared,
    transfer: Arc<Mutex<Transfer>>,
    writer: Sender<Out>,
    pump: Sender<()>,
    notes: Sender<TransferNote>,
    flow: OutputFlow,
    raw_output: RawOutput,
}

impl Ctx {
    fn deliver(&self, out: Outcome, reader: bool) {
        let raw = out.raw;
        for (bytes, record) in [(&out.terminal[..raw.start], false), (&out.terminal[raw.clone()], true), (&out.terminal[raw.end..], false)] {
            for slice in bytes.chunks(FEED_SLICE) {
                {
                    let mut terminal = self.terminal.lock();
                    terminal.feed(slice);
                    if record {
                        self.raw_output.append(slice);
                    }
                }
                (self.flow.wake)();
                if reader {
                    let drawn = *self.flow.drawn.lock().unwrap();
                    if Pacer::should_yield(Instant::now(), drawn) {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
            }
        }
        for bytes in out.write {
            let _ = self.writer.send(Out::Transfer(bytes));
        }
        if let Some(note) = out.note {
            let _ = self.notes.send((self.window, self.pane, note));
            (self.flow.wake)();
        }
        if out.pump {
            let _ = self.pump.send(());
        }
    }
}

pub struct Start {
    pub flow: OutputFlow,
    pub raw_output: RawOutput,
    pub window: WindowId,
    pub pane: PaneId,
    pub downloads: PathBuf,
    pub exits: Sender<Exit>,
    pub notes: Sender<TransferNote>,
}

impl Session {
    pub fn start(start: Start, spawned: Spawned, terminal: Shared) -> Session {
        let Start { window, pane, downloads, exits, notes, flow, raw_output } = start;
        let wake = flow.wake.clone();
        let Spawned { pty, mut reader, mut writer, mut raw, wait } = spawned;
        let (tx, rx) = unbounded::<Out>();
        let (pump_tx, pump_rx) = unbounded::<()>();
        let ctx = Ctx { window, pane, terminal, transfer: Arc::new(Mutex::new(Transfer::new(downloads))), writer: tx.clone(), pump: pump_tx, notes, flow, raw_output };
        let reading = ctx.clone();
        std::thread::Builder::new()
            .name(format!("aster-pty-read-{pane}"))
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let out = match reading.transfer.lock() {
                                Ok(mut t) => t.input(&buf[..n]),
                                Err(_) => Outcome { terminal: buf[..n].to_vec(), raw: 0..n, ..Outcome::default() },
                            };
                            reading.deliver(out, true);
                        }
                    }
                }
            })
            .expect("pty reader thread");
        let transfer = ctx.transfer.clone();
        let transfer_writer = tx.clone();
        std::thread::Builder::new()
            .name(format!("aster-zmodem-{pane}"))
            .spawn(move || {
                for () in pump_rx {
                    loop {
                        let next = transfer.lock().ok().and_then(|mut t| t.poll());
                        let Some(bytes) = next else { break };
                        while transfer_writer.len() > PUMP_QUEUE {
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        if transfer_writer.send(Out::Transfer(bytes)).is_err() {
                            return;
                        }
                    }
                }
            })
            .expect("transfer pump thread");
        std::thread::Builder::new()
            .name(format!("aster-pty-write-{pane}"))
            .spawn(move || {
                for out in rx {
                    let (target, bytes): (&mut Box<dyn Write + Send>, Vec<u8>) = match out {
                        Out::Keys(bytes) => (&mut writer, bytes),
                        Out::Transfer(bytes) => (raw.as_mut().unwrap_or(&mut writer), bytes),
                    };
                    if target.write_all(&bytes).and_then(|_| target.flush()).is_err() {
                        break;
                    }
                }
            })
            .expect("pty writer thread");
        std::thread::Builder::new()
            .name(format!("aster-pty-wait-{pane}"))
            .spawn(move || {
                let code = wait();
                let _ = exits.send((window, pane, code));
                wake();
            })
            .expect("pty wait thread");
        Session { pty, writer: tx, ctx }
    }

    pub fn write(&self, bytes: Vec<u8>) {
        let _ = self.writer.send(Out::Keys(bytes));
    }

    pub fn send_files(&self, files: &[PathBuf]) {
        let out = self.ctx.transfer.lock().map(|mut t| t.send_files(files)).unwrap_or_default();
        self.ctx.deliver(out, false);
    }

    pub fn accept_transfer(&self) {
        let out = self.ctx.transfer.lock().map(|mut t| t.accept()).unwrap_or_default();
        self.ctx.deliver(out, false);
    }

    pub fn cancel_transfer(&self) {
        let out = self.ctx.transfer.lock().map(|mut t| t.cancel()).unwrap_or_default();
        self.ctx.deliver(out, false);
    }
}

#[cfg(test)]
#[path = "../test/session.rs"]
mod tests;
