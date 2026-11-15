use super::*;
use aster_zmodem::frame::{hex_header, Header, CANFC32, CANFDX, CANOVIO, ZRINIT, ZRQINIT};

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
    let start = [b"sz started\r\n".to_vec(), hex_header(Header::new(ZRQINIT, [0; 4]))].concat();
    let mut out = t.input(&start);
    let mut shown = out.terminal.clone();
    assert!(text(&shown).starts_with("sz started"));
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
    let offer = [b"rz waiting\r\n".to_vec(), hex_header(Header::new(ZRINIT, [0, 0, 0, CANFDX | CANOVIO | CANFC32]))].concat();
    let out = t.input(&offer);
    assert_eq!(out.note, Some(Note::Offered));
    assert!(t.waiting());
    assert!(!t.send_files(&[dir.join("missing")]).terminal.is_empty(), "a missing file is reported");
    assert!(t.waiting(), "nothing valid was chosen, so it keeps waiting");

    let mut remote = Receiver::new();
    remote.feed(&hex_header(Header::new(ZRQINIT, [0; 4])));
    let mut out = t.send_files(std::slice::from_ref(&path));
    assert_eq!(out.note, Some(Note::Ended));
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
