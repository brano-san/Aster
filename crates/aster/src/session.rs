use crate::zmodem::{Note, Outcome, Transfer};
use aster_core::app::Shared;
use aster_core::panes::PaneId;
use aster_platform::{Pty, Spawned, WindowId};
use crossbeam_channel::{unbounded, Sender};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const PUMP_QUEUE: usize = 64;

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
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Ctx {
    fn deliver(&self, out: Outcome) {
        if !out.terminal.is_empty() {
            self.terminal.lock().feed(&out.terminal);
        }
        for bytes in out.write {
            let _ = self.writer.send(Out::Transfer(bytes));
        }
        if let Some(note) = out.note {
            let _ = self.notes.send((self.window, self.pane, note));
        }
        if out.pump {
            let _ = self.pump.send(());
        }
        (self.wake)();
    }
}

pub struct Start {
    pub window: WindowId,
    pub pane: PaneId,
    pub downloads: PathBuf,
    pub exits: Sender<Exit>,
    pub notes: Sender<TransferNote>,
}

impl Session {
    pub fn start(start: Start, spawned: Spawned, terminal: Shared, wake: impl Fn() + Clone + Send + Sync + 'static) -> Session {
        let Start { window, pane, downloads, exits, notes } = start;
        let Spawned { pty, mut reader, mut writer, mut raw, wait } = spawned;
        let (tx, rx) = unbounded::<Out>();
        let (pump_tx, pump_rx) = unbounded::<()>();
        let ctx = Ctx { window, pane, terminal, transfer: Arc::new(Mutex::new(Transfer::new(downloads))), writer: tx.clone(), pump: pump_tx, notes, wake: Arc::new(wake.clone()) };
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
                                Err(_) => Outcome { terminal: buf[..n].to_vec(), ..Outcome::default() },
                            };
                            reading.deliver(out);
                        }
                    }
                }
            })
            .expect("pty reader thread");
        let pumping = ctx.clone();
        std::thread::Builder::new()
            .name(format!("aster-zmodem-{pane}"))
            .spawn(move || {
                for () in pump_rx {
                    loop {
                        let next = pumping.transfer.lock().ok().and_then(|mut t| t.poll());
                        let Some(bytes) = next else { break };
                        while pumping.writer.len() > PUMP_QUEUE {
                            std::thread::sleep(Duration::from_millis(2));
                        }
                        if pumping.writer.send(Out::Transfer(bytes)).is_err() {
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
        self.ctx.deliver(out);
    }

    pub fn accept_transfer(&self) {
        let out = self.ctx.transfer.lock().map(|mut t| t.accept()).unwrap_or_default();
        self.ctx.deliver(out);
    }

    pub fn cancel_transfer(&self) {
        let out = self.ctx.transfer.lock().map(|mut t| t.cancel()).unwrap_or_default();
        self.ctx.deliver(out);
    }
}
