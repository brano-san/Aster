use super::*;
use aster_zmodem::frame::{hex_header, Header, CANFC32, CANFDX, CANOVIO, ZFIN, ZRINIT, ZRQINIT};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aster-zmodem-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn plain_output_passes_through_untouched() {
    let mut t = Transfer::new(temp_dir("plain"));
    let out = t.input(b"ls -la\r\n");
    assert_eq!(out.terminal, b"ls -la\r\n");
    assert!(out.write.is_empty() && out.note.is_none() && !t.active());
}

#[test]
fn files_sent_by_the_remote_side_land_in_the_downloads_folder() {
    let downloads = temp_dir("receive");
    let body: Vec<u8> = (0..50_000u32).map(|i| (i % 253) as u8).collect();
    let mut remote = Sender::new(vec![
        SendFile { name: "dir/report.bin".into(), modified: 0, source: Box::new(body.clone()) },
        SendFile { name: "empty.txt".into(), modified: 0, source: Box::new(Vec::new()) },
    ]);
    let mut t = Transfer::new(downloads.clone());
    let start = [b"sending\r\n".to_vec(), hex_header(Header::new(ZRQINIT, [0; 4]))].concat();
    let out = t.input(&start);
    let mut shown = out.terminal.clone();
    assert!(text(&shown).starts_with("sending"));
    assert_eq!(out.note, Some(Note::Incoming));
    let mut out = t.accept();
    assert_eq!(out.note, Some(Note::Running));
    let mut to_remote = std::mem::take(&mut out.write);
    for _ in 0..100_000 {
        for w in to_remote.drain(..) {
            remote.feed(&w);
        }
        while let Some(b) = remote.poll() {
            let o = t.input(&b);
            shown.extend(o.terminal);
            to_remote.extend(o.write);
        }
        if !t.active() && remote.finished() {
            break;
        }
    }
    assert!(!t.active() && remote.finished(), "{}", text(&shown));
    assert_eq!(std::fs::read(downloads.join("report.bin")).unwrap(), body);
    assert_eq!(std::fs::read(downloads.join("empty.txt")).unwrap(), b"");
    let log = text(&shown);
    assert!(log.contains("Saved report.bin") && log.contains("Transfer complete"), "{log}");
}

#[test]
fn a_waiting_remote_side_receives_the_chosen_files() {
    let dir = temp_dir("send");
    let path = dir.join("upload.dat");
    let body: Vec<u8> = (0..70_000u32).map(|i| (i * 7 % 256) as u8).collect();
    std::fs::write(&path, &body).unwrap();
    let mut t = Transfer::new(dir.clone());
    let offer = [b"waiting\r\n".to_vec(), hex_header(Header::new(ZRINIT, [0, 0, 0, CANFDX | CANOVIO | CANFC32]))].concat();
    let out = t.input(&offer);
    assert_eq!(out.note, Some(Note::AwaitingFiles));
    assert!(t.waiting());
    assert!(!t.send_files(&[dir.join("missing")]).terminal.is_empty(), "a missing file is reported");
    assert!(t.waiting(), "nothing valid was chosen, so it keeps waiting");

    let mut remote = Receiver::new();
    remote.feed(&hex_header(Header::new(ZRQINIT, [0; 4])));
    let mut out = t.send_files(std::slice::from_ref(&path));
    assert_eq!(out.note, Some(Note::Running));
    let mut got = Vec::new();
    let mut to_remote = std::mem::take(&mut out.write);
    let mut done = false;
    for _ in 0..100_000 {
        while let Some(b) = t.poll() {
            to_remote.push(b);
        }
        for w in std::mem::take(&mut to_remote) {
            for event in remote.feed(&w) {
                match event {
                    RxEvent::Send(reply) => to_remote.extend(t.input(&reply).write),
                    RxEvent::Data(d) => got.extend(d),
                    RxEvent::Done => done = true,
                    _ => {}
                }
            }
        }
        if done && !t.active() {
            break;
        }
    }
    assert!(done && !t.active());
    assert_eq!(got, body);
}

#[test]
fn cancelling_a_waiting_transfer_tells_the_remote_side() {
    let mut t = Transfer::new(temp_dir("cancel"));
    t.input(&hex_header(Header::new(ZRINIT, [0, 0, 0, CANFC32])));
    let out = t.cancel();
    assert!(out.write[0].starts_with(&[0x18; 5]));
    assert_eq!(out.note, Some(Note::Ended));
    assert!(!t.active());
    assert_eq!(t.input(b"$ ").terminal, b"$ ", "the shell is back afterwards");
}

const PROMPT: &[u8] = b"\r\nuser@host:~$ ";
const COMPLETE: &str = "\x1b[1m[Transfer complete]\x1b[0m\r\n";

fn remote_sender(body: &[u8]) -> Sender {
    Sender::new(vec![SendFile { name: "big.bin".into(), modified: 0, source: Box::new(body.to_vec()) }])
}

fn start() -> Vec<u8> {
    hex_header(Header::new(ZRQINIT, [0; 4]))
}

fn exchange(t: &mut Transfer, remote: &mut Sender, mut to_remote: Vec<Vec<u8>>, mut until: impl FnMut(&Transfer, &[u8]) -> bool, mut chunk: impl FnMut(Vec<u8>) -> Vec<u8>) -> Vec<u8> {
    let mut shown = Vec::new();
    for _ in 0..100_000 {
        for w in to_remote.drain(..) {
            remote.feed(&w);
        }
        let mut batch = Vec::new();
        for b in std::iter::from_fn(|| remote.poll()).take(8) {
            batch.extend(b);
        }
        if !batch.is_empty() {
            let o = t.input(&chunk(batch));
            shown.extend(o.terminal);
            to_remote.extend(o.write);
        }
        if until(t, &shown) {
            break;
        }
    }
    shown
}

#[test]
fn nothing_is_received_until_the_files_are_accepted() {
    let downloads = temp_dir("accept");
    let body: Vec<u8> = (0..20_000u32).map(|i| (i % 251) as u8).collect();
    let mut remote = remote_sender(&body);
    let mut t = Transfer::new(downloads.clone());
    let out = t.input(&[b"before".to_vec(), start()].concat());
    assert!(text(&out.terminal).starts_with("before"), "output before the marker is shown");
    assert!(out.write.is_empty(), "the remote side gets no answer yet");
    assert_eq!(out.note, Some(Note::Incoming));
    assert!(t.asking());
    let again = t.input(&start());
    assert!(again.write.is_empty() && again.terminal.is_empty() && again.note.is_none());
    assert!(std::fs::read_dir(&downloads).unwrap().next().is_none(), "no file before the answer");
    let out = t.accept();
    assert_eq!(out.note, Some(Note::Running));
    assert!(!out.write.is_empty(), "the buffered request is answered on accept");
    exchange(&mut t, &mut remote, out.write, |t, _| !t.active(), |b| b);
    assert!(!t.active());
    assert_eq!(std::fs::read(downloads.join("big.bin")).unwrap(), body);
}

#[test]
fn refusing_incoming_files_aborts_them_and_returns_to_the_shell() {
    let downloads = temp_dir("refuse");
    let mut t = Transfer::new(downloads.clone());
    t.input(&start());
    let out = t.cancel();
    assert_eq!(out.write, vec![[vec![0x18; 8], vec![0x08; 8]].concat()]);
    assert_eq!(out.note, Some(Note::Ended));
    assert!(text(&out.terminal).contains("Refused"), "{}", text(&out.terminal));
    assert!(!t.active());
    assert!(t.accept().write.is_empty(), "nothing is left to accept");
    assert_eq!(t.input(PROMPT).terminal, PROMPT);
    assert!(std::fs::read_dir(&downloads).unwrap().next().is_none());
}

#[test]
fn cancelling_while_receiving_deletes_the_partial_file() {
    let downloads = temp_dir("cancel-receive");
    let body: Vec<u8> = (0..400_000u32).map(|i| (i % 249) as u8).collect();
    let mut remote = remote_sender(&body);
    let mut t = Transfer::new(downloads.clone());
    t.input(&start());
    let out = t.accept();
    let partial = downloads.join("big.bin");
    let check = partial.clone();
    exchange(&mut t, &mut remote, out.write, |_, _| std::fs::metadata(&check).is_ok_and(|m| m.len() > 0), |b| b);
    assert!(t.active() && std::fs::metadata(&partial).unwrap().len() < body.len() as u64);
    let out = t.cancel();
    assert!(out.write[0].starts_with(&[0x18; 8]));
    assert_eq!(out.note, Some(Note::Ended));
    assert!(!t.active());
    assert!(!partial.exists(), "the partial download is removed");
    let events = remote.feed(&out.write[0]);
    assert!(events.contains(&TxEvent::Aborted) && remote.finished());
}

#[test]
fn cancelling_while_sending_tells_the_remote_side() {
    let dir = temp_dir("cancel-send");
    let path = dir.join("upload.dat");
    std::fs::write(&path, vec![7u8; 300_000]).unwrap();
    let mut t = Transfer::new(dir.clone());
    t.input(&hex_header(Header::new(ZRINIT, [0, 0, 0, CANFDX | CANOVIO | CANFC32])));
    let mut remote = Receiver::new();
    remote.feed(&start());
    let mut to_remote = t.send_files(std::slice::from_ref(&path)).write;
    let mut got = 0;
    for _ in 0..1000 {
        while let Some(b) = t.poll() {
            to_remote.push(b);
        }
        for w in std::mem::take(&mut to_remote) {
            for event in remote.feed(&w) {
                match event {
                    RxEvent::Send(reply) => to_remote.extend(t.input(&reply).write),
                    RxEvent::Data(d) => got += d.len(),
                    _ => {}
                }
            }
        }
        if got > 0 {
            break;
        }
    }
    assert!(got > 0 && t.active());
    let out = t.cancel();
    assert_eq!(out.note, Some(Note::Ended));
    assert!(!t.active() && t.poll().is_none());
    assert!(remote.feed(&out.write[0]).contains(&RxEvent::Aborted));
    assert_eq!(t.input(PROMPT).terminal, PROMPT);
}

#[test]
fn shell_output_in_the_same_chunk_as_the_final_bytes_reaches_the_terminal() {
    let body = vec![1u8; 5000];
    let mut remote = remote_sender(&body);
    let mut t = Transfer::new(temp_dir("tail-same"));
    t.input(&start());
    let out = t.accept();
    let fin = hex_header(Header::new(ZFIN, [0; 4]));
    let shown = exchange(&mut t, &mut remote, out.write, |t, _| !t.active(), |mut b| {
        if b == fin {
            b.extend(b"OO");
            b.extend(PROMPT);
        }
        b
    });
    let log = text(&shown);
    assert!(log.ends_with(&format!("{COMPLETE}{}", text(PROMPT))), "{log:?}");
}

#[test]
fn shell_output_after_the_final_bytes_in_later_chunks_is_exact() {
    let mut remote = remote_sender(b"abc");
    let mut t = Transfer::new(temp_dir("tail-later"));
    t.input(&start());
    let out = t.accept();
    let shown = exchange(&mut t, &mut remote, out.write, |t, _| !t.active(), |b| b);
    assert!(text(&shown).ends_with(COMPLETE));
    assert!(t.input(b"O").terminal.is_empty());
    assert!(t.input(b"O").terminal.is_empty());
    assert_eq!(t.input(PROMPT).terminal, PROMPT);
    assert_eq!(t.input(b"OK").terminal, b"OK", "only the two final bytes are dropped");
}

#[test]
fn shell_output_after_a_finished_upload_reaches_the_terminal() {
    let dir = temp_dir("tail-send");
    let path = dir.join("small.txt");
    std::fs::write(&path, b"hello").unwrap();
    let mut t = Transfer::new(dir.clone());
    t.input(&hex_header(Header::new(ZRINIT, [0, 0, 0, CANFDX | CANOVIO | CANFC32])));
    let mut remote = Receiver::new();
    remote.feed(&start());
    let mut to_remote = t.send_files(std::slice::from_ref(&path)).write;
    let mut shown = Vec::new();
    for _ in 0..1000 {
        while let Some(b) = t.poll() {
            to_remote.push(b);
        }
        for w in std::mem::take(&mut to_remote) {
            for event in remote.feed(&w) {
                if let RxEvent::Send(mut reply) = event {
                    if remote.finished() {
                        reply.extend(PROMPT);
                    }
                    let o = t.input(&reply);
                    shown.extend(o.terminal);
                    to_remote.extend(o.write);
                }
            }
        }
        if !t.active() {
            break;
        }
    }
    assert!(!t.active());
    let log = text(&shown);
    assert!(log.ends_with(&format!("{COMPLETE}{}", text(PROMPT))), "{log:?}");
}

#[test]
fn output_after_a_remote_cancel_reaches_the_terminal_without_the_cancel_bytes() {
    let downloads = temp_dir("remote-cancel");
    let mut remote = remote_sender(&vec![3u8; 200_000]);
    let mut t = Transfer::new(downloads.clone());
    t.input(&start());
    let out = t.accept();
    let partial = downloads.join("big.bin");
    let check = partial.clone();
    exchange(&mut t, &mut remote, out.write, |_, _| std::fs::metadata(&check).is_ok_and(|m| m.len() > 0), |b| b);
    let mut bytes = remote.abort();
    bytes.extend(PROMPT);
    let out = t.input(&bytes);
    assert_eq!(out.note, Some(Note::Ended));
    assert!(!t.active() && !partial.exists());
    assert!(text(&out.terminal).ends_with(&text(PROMPT)));
    assert!(!out.terminal.contains(&0x08) && !out.terminal.contains(&0x18));
}
