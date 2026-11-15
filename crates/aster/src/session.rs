use aster_core::app::Shared;
use aster_core::panes::PaneId;
use aster_platform::{Pty, Spawned, WindowId};
use crossbeam_channel::{unbounded, Sender};
use std::io::{Read, Write};

pub struct Session {
    pub pty: Box<dyn Pty>,
    writer: Sender<Vec<u8>>,
}

pub type Exit = (WindowId, PaneId, Option<u32>);

impl Session {
    pub fn start(window: WindowId, pane: PaneId, spawned: Spawned, terminal: Shared, wake: impl Fn() + Clone + Send + 'static, exits: Sender<Exit>) -> Session {
        let Spawned { pty, mut reader, mut writer, wait } = spawned;
        let read_wake = wake.clone();
        std::thread::Builder::new()
            .name(format!("aster-pty-read-{pane}"))
            .spawn(move || {
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            terminal.lock().feed(&buf[..n]);
                            read_wake();
                        }
                    }
                }
            })
            .expect("pty reader thread");
        let (tx, rx) = unbounded::<Vec<u8>>();
        std::thread::Builder::new()
            .name(format!("aster-pty-write-{pane}"))
            .spawn(move || {
                for bytes in rx {
                    if writer.write_all(&bytes).and_then(|_| writer.flush()).is_err() {
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
        Session { pty, writer: tx }
    }

    pub fn write(&self, bytes: Vec<u8>) {
        let _ = self.writer.send(bytes);
    }
}
