use super::*;
use aster_core::profile::{InputMode, LoginStep};
use aster_platform::Pty;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(5);

struct Nothing;

impl Pty for Nothing {
    fn resize(&self, _cols: u16, _rows: u16) {}

    fn kill(&self) {}

    fn pid(&self) -> u32 {
        0
    }
}

struct Tap(Sender<Vec<u8>>);

impl Write for Tap {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let _ = self.0.send(buf.to_vec());
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct Feed(Receiver<Vec<u8>>);

impl Read for Feed {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        match self.0.recv() {
            Ok(bytes) => {
                out[..bytes.len()].copy_from_slice(&bytes);
                Ok(bytes.len())
            }
            Err(_) => Ok(0),
        }
    }
}

struct Fake {
    spawned: Spawned,
    output: Sender<Vec<u8>>,
    keys: Receiver<Vec<u8>>,
    raw: Receiver<Vec<u8>>,
}

fn fake() -> Fake {
    let (output, from_session) = unbounded();
    let (keys_tx, keys) = unbounded();
    let (raw_tx, raw) = unbounded();
    let spawned = Spawned {
        pty: Box::new(Nothing),
        reader: Box::new(Feed(from_session)),
        writer: Box::new(Tap(keys_tx)),
        raw: Some(Box::new(Tap(raw_tx))),
        wait: Box::new(|| Some(0)),
    };
    Fake { spawned, output, keys, raw }
}

fn line_mode() -> Stream {
    Stream { input: InputMode::Line, login: vec![LoginStep { expect: "login:".into(), send: r"admin\r".into(), ..LoginStep::default() }], ..Stream::default() }
}

#[test]
fn plain_profiles_are_not_wrapped() {
    let fake = fake();
    let mut spawned = wrap(fake.spawned, Some(&Stream::default()));
    spawned.writer.write_all(b"x").unwrap();
    assert_eq!(fake.keys.recv_timeout(WAIT).unwrap(), b"x");
}

#[test]
fn keys_are_edited_while_transfers_and_login_replies_go_raw() {
    let fake = fake();
    let mut spawned = wrap(fake.spawned, Some(&line_mode()));
    spawned.writer.write_all(b"ab").unwrap();
    let mut shown = [0u8; 2];
    spawned.reader.read_exact(&mut shown).unwrap();
    assert_eq!(&shown, b"ab");
    assert!(fake.keys.try_recv().is_err());
    spawned.writer.write_all(b"\r").unwrap();
    assert_eq!(fake.keys.recv_timeout(WAIT).unwrap(), b"ab\r");
    spawned.raw.as_mut().unwrap().write_all(b"\x18B0\r").unwrap();
    assert_eq!(fake.raw.recv_timeout(WAIT).unwrap(), b"\x18B0\r");
    fake.output.send(b"login:".to_vec()).unwrap();
    assert_eq!(fake.raw.recv_timeout(WAIT).unwrap(), b"admin\r");
    assert!(fake.keys.try_recv().is_err());
}

#[test]
fn output_ends_when_the_session_does() {
    let fake = fake();
    let mut spawned = wrap(fake.spawned, Some(&line_mode()));
    fake.output.send(b"bye".to_vec()).unwrap();
    drop(fake.output);
    let mut text = Vec::new();
    spawned.reader.read_to_end(&mut text).unwrap();
    assert_eq!(text, b"bye");
    assert_eq!((spawned.wait)(), Some(0));
}
