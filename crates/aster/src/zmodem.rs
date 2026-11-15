use aster_zmodem::frame::{detect, Detected};
use aster_zmodem::receive::{Receiver, RxEvent};
use aster_zmodem::send::{SendFile, Sender, Source, TxEvent};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, UNIX_EPOCH};

const MAX_WAITING: usize = 64 * 1024;
const STATUS_EVERY: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    Offered,
    Ended,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    pub terminal: Vec<u8>,
    pub write: Vec<Vec<u8>>,
    pub note: Option<Note>,
    pub pump: bool,
}

impl Outcome {
    fn say(&mut self, text: &str) {
        self.terminal.extend_from_slice(format!("\r\n\x1b[1m[{text}]\x1b[0m\r\n").as_bytes());
    }

    fn status(&mut self, text: &str) {
        self.terminal.extend_from_slice(format!("\r\x1b[K\x1b[2m[{text}]\x1b[0m").as_bytes());
    }
}

struct Incoming {
    file: std::fs::File,
    path: PathBuf,
    name: String,
    written: u64,
    size: Option<u64>,
}

enum Mode {
    Idle,
    Receiving { rx: Receiver, file: Option<Incoming>, shown: Instant },
    Offered { waiting: Vec<u8> },
    Sending { tx: Box<Sender>, names: Vec<String>, sizes: Vec<u64>, shown: Instant },
}

pub struct Transfer {
    mode: Mode,
    downloads: PathBuf,
}

struct LocalFile(std::fs::File, u64);

impl Source for LocalFile {
    fn len(&self) -> u64 {
        self.1
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> usize {
        #[cfg(windows)]
        let read = std::os::windows::fs::FileExt::seek_read(&self.0, buf, offset);
        #[cfg(unix)]
        let read = std::os::unix::fs::FileExt::read_at(&self.0, buf, offset);
        read.unwrap_or(0)
    }
}

fn human(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / 1024.0),
        b => format!("{b} B"),
    }
}

impl Transfer {
    pub fn new(downloads: PathBuf) -> Self {
        Self { mode: Mode::Idle, downloads }
    }

    #[cfg(test)]
    pub fn waiting(&self) -> bool {
        matches!(self.mode, Mode::Offered { .. })
    }

    #[cfg(test)]
    pub fn active(&self) -> bool {
        !matches!(self.mode, Mode::Idle)
    }

    pub fn input(&mut self, bytes: &[u8]) -> Outcome {
        let mut out = Outcome::default();
        match &mut self.mode {
            Mode::Idle => match detect(bytes) {
                None => out.terminal.extend_from_slice(bytes),
                Some((at, kind)) => {
                    out.terminal.extend_from_slice(&bytes[..at]);
                    match kind {
                        Detected::Receive => {
                            out.say("Receiving files");
                            self.mode = Mode::Receiving { rx: Receiver::new(), file: None, shown: Instant::now() };
                            self.receive(&bytes[at..], &mut out);
                        }
                        Detected::Send => {
                            out.say("The remote side is waiting for files: drop them onto the window or type a path and press Enter; Esc cancels");
                            self.mode = Mode::Offered { waiting: bytes[at..].to_vec() };
                            out.note = Some(Note::Offered);
                        }
                    }
                }
            },
            Mode::Receiving { .. } => self.receive(bytes, &mut out),
            Mode::Offered { waiting } => {
                waiting.extend_from_slice(bytes);
                if waiting.len() > MAX_WAITING {
                    let text = std::mem::take(waiting);
                    self.mode = Mode::Idle;
                    out.terminal.extend_from_slice(&text);
                    out.note = Some(Note::Ended);
                }
            }
            Mode::Sending { .. } => self.send(bytes, &mut out),
        }
        out
    }

    fn receive(&mut self, bytes: &[u8], out: &mut Outcome) {
        let Mode::Receiving { rx, file, shown } = &mut self.mode else { return };
        let mut ended = false;
        for event in rx.feed(bytes) {
            match event {
                RxEvent::Send(b) => out.write.push(b),
                RxEvent::FileStart { name, size } => {
                    let _ = std::fs::create_dir_all(&self.downloads);
                    let path = aster_ssh::sftp::unique(&self.downloads, &aster_ssh::sftp::local_name(&name));
                    match std::fs::File::create(&path) {
                        Ok(f) => {
                            out.say(&format!("Receiving {name}{}", size.map(|s| format!(" ({})", human(s))).unwrap_or_default()));
                            *file = Some(Incoming { file: f, path, name, written: 0, size });
                        }
                        Err(e) => {
                            out.say(&format!("Cannot save {name}: {e}"));
                            out.write.push(rx.abort());
                            ended = true;
                            break;
                        }
                    }
                }
                RxEvent::Data(data) => {
                    if let Some(f) = file.as_mut() {
                        if f.file.write_all(&data).is_err() {
                            out.say(&format!("Cannot write {}", f.path.display()));
                            out.write.push(rx.abort());
                            let _ = std::fs::remove_file(&f.path);
                            *file = None;
                            ended = true;
                            break;
                        }
                        f.written += data.len() as u64;
                        if shown.elapsed() >= STATUS_EVERY {
                            *shown = Instant::now();
                            let part = f.size.filter(|s| *s > 0).map(|s| format!(" {}%", f.written * 100 / s)).unwrap_or_default();
                            out.status(&format!("{} {}{part}", f.name, human(f.written)));
                        }
                    }
                }
                RxEvent::FileEnd => {
                    if let Some(f) = file.take() {
                        drop(f.file);
                        out.say(&format!("Saved {} to {}", f.name, f.path.display()));
                    }
                }
                RxEvent::Done => {
                    out.say("Transfer complete");
                    ended = true;
                }
                RxEvent::Aborted => {
                    if let Some(f) = file.take() {
                        drop(f.file);
                        let _ = std::fs::remove_file(&f.path);
                    }
                    out.say("Transfer cancelled");
                    ended = true;
                }
            }
        }
        if ended {
            self.mode = Mode::Idle;
            out.note = Some(Note::Ended);
        }
    }

    fn send(&mut self, bytes: &[u8], out: &mut Outcome) {
        let Mode::Sending { tx, names, sizes, shown } = &mut self.mode else { return };
        let mut ended = false;
        for event in tx.feed(bytes) {
            match event {
                TxEvent::Progress { file, sent } => {
                    if shown.elapsed() >= STATUS_EVERY {
                        *shown = Instant::now();
                        let total = sizes.get(file).copied().unwrap_or(0).max(1);
                        out.status(&format!("{} {}%", names.get(file).cloned().unwrap_or_default(), sent * 100 / total));
                    }
                }
                TxEvent::FileDone(i) => out.say(&format!("Sent {}", names.get(i).cloned().unwrap_or_default())),
                TxEvent::Skipped(i) => out.say(&format!("The remote side skipped {}", names.get(i).cloned().unwrap_or_default())),
                TxEvent::Done => {
                    out.say("Transfer complete");
                    ended = true;
                }
                TxEvent::Aborted => {
                    out.say("Transfer cancelled");
                    ended = true;
                }
            }
        }
        out.pump = !ended;
        if ended {
            if let Some(last) = tx.poll() {
                out.write.push(last);
            }
            self.mode = Mode::Idle;
            out.note = Some(Note::Ended);
        }
    }

    pub fn send_files(&mut self, paths: &[PathBuf]) -> Outcome {
        let mut out = Outcome::default();
        let Mode::Offered { waiting } = &mut self.mode else { return out };
        let waiting = std::mem::take(waiting);
        let mut files = Vec::new();
        let (mut names, mut sizes) = (Vec::new(), Vec::new());
        for path in paths {
            match open(path) {
                Ok(file) => {
                    names.push(file.name.clone());
                    sizes.push(file.source.len());
                    files.push(file);
                }
                Err(e) => out.say(&e),
            }
        }
        if files.is_empty() {
            self.mode = Mode::Offered { waiting };
            return out;
        }
        out.say(&format!("Sending {}", names.join(", ")));
        self.mode = Mode::Sending { tx: Box::new(Sender::new(files)), names, sizes, shown: Instant::now() };
        out.note = Some(Note::Ended);
        self.send(&waiting, &mut out);
        out.note = out.note.or(Some(Note::Ended));
        out
    }

    pub fn cancel(&mut self) -> Outcome {
        let mut out = Outcome::default();
        let abort = match &mut self.mode {
            Mode::Idle => return out,
            Mode::Receiving { rx, file, .. } => {
                if let Some(f) = file.take() {
                    drop(f.file);
                    let _ = std::fs::remove_file(&f.path);
                }
                rx.abort()
            }
            Mode::Sending { tx, .. } => tx.abort(),
            Mode::Offered { .. } => {
                let mut b = vec![0x18; 8];
                b.extend([0x08; 8]);
                b
            }
        };
        out.write.push(abort);
        out.say("Transfer cancelled");
        out.note = Some(Note::Ended);
        self.mode = Mode::Idle;
        out
    }

    pub fn poll(&mut self) -> Option<Vec<u8>> {
        match &mut self.mode {
            Mode::Sending { tx, .. } => tx.poll(),
            _ => None,
        }
    }
}

fn open(path: &Path) -> Result<SendFile, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file; only files can be sent", path.display()));
    }
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
    let modified = meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    Ok(SendFile { name, modified, source: Box::new(LocalFile(file, meta.len())) })
}

#[cfg(test)]
#[path = "../test/zmodem.rs"]
mod tests;
