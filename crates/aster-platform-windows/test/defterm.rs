use super::*;
use std::io::Read;

#[test]
fn the_handoff_proxy_registers_in_the_current_process() {
    use windows::Win32::System::Com::{CoGetClassObject, CoGetPSClsid};
    let _apartment = Apartment::enter().unwrap();
    let _registration = register_proxy().unwrap();
    unsafe {
        assert_eq!(CoGetPSClsid(&ITerminalHandoff3::IID).unwrap(), PROXY_CLSID);
        let _factory: IUnknown = CoGetClassObject(&PROXY_CLSID, CLSCTX_INPROC_SERVER, None).unwrap();
    }
}

#[test]
fn the_broker_transfers_streams_to_an_authenticated_pipe_peer() {
    let (signal_read, signal_write) = local_pipe().unwrap();
    let current = unsafe { GetCurrentProcess() };
    let real_process = duplicate_from(current, current.0 as usize).unwrap();
    let (mut attached, mut input, mut output) = attached_peer(&signal_write, raw(&real_process));
    drop(signal_write);
    assert_eq!(attached.pty.pid(), std::process::id());
    attached.writer.write_all(b"in").unwrap();
    let mut read = [0; 2];
    input.read_exact(&mut read).unwrap();
    assert_eq!(&read, b"in");
    output.write_all(b"out").unwrap();
    let mut read = [0; 3];
    attached.reader.read_exact(&mut read).unwrap();
    assert_eq!(&read, b"out");
    attached.pty.force_resize(80, 24);
    let mut signal = File::from(signal_read);
    let mut packet = [0; 12];
    signal.read_exact(&mut packet).unwrap();
    assert_eq!(packet, [8, 0, 79, 0, 24, 0, 8, 0, 80, 0, 24, 0]);
    attached.pty.kill();
    assert_eq!(signal.read(&mut read).unwrap(), 0);
}

fn attached_peer(signal: &OwnedHandle, client: HANDLE) -> (Spawned, File, File) {
    let (send, recv) = std::sync::mpsc::sync_channel(1);
    let relayed = relay(raw(signal), client, |token| {
        let token = token.to_owned();
        std::thread::Builder::new().spawn(move || { let _ = send.send(attach_from_pipe(&token)); }).map(|_| ())
    });
    let attached = recv.recv_timeout(TRANSFER_TIMEOUT).unwrap().unwrap();
    let (host_input, host_output) = relayed.unwrap();
    (attached, File::from(host_input), File::from(host_output))
}

#[test]
fn a_delegated_console_keeps_its_streams_after_the_initial_client_exits() {
    let system = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap());
    let mut client = Command::new(system.join("System32").join("cmd.exe"))
        .args(["/D", "/C", "exit", "/B", "23"])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .creation_flags(DETACHED_PROCESS.0).spawn().unwrap();
    assert_eq!(client.wait().unwrap().code(), Some(23));
    let (signal_read, signal_write) = local_pipe().unwrap();
    let (attached, _input, mut output) = attached_peer(&signal_write, HANDLE(client.as_raw_handle()));
    drop(signal_write);
    let Spawned { pty, mut reader, wait, .. } = attached;
    assert_eq!(pty.pid(), client.id());
    let (started_tx, started) = std::sync::mpsc::sync_channel(1);
    let (done_tx, done) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        done_tx.send(wait()).unwrap();
    });
    started.recv_timeout(TRANSFER_TIMEOUT).unwrap();
    assert_eq!(done.recv_timeout(Duration::from_millis(200)), Err(std::sync::mpsc::RecvTimeoutError::Timeout));
    pty.resize(90, 32);
    let mut signal = File::from(signal_read);
    let mut packet = [0; 6];
    signal.read_exact(&mut packet).unwrap();
    assert_eq!(packet, [8, 0, 90, 0, 32, 0]);
    output.write_all(b"late output").unwrap();
    drop(output);
    let mut collected = Vec::new();
    reader.read_to_end(&mut collected).unwrap();
    assert_eq!(collected, b"late output");
    assert_eq!(done.recv_timeout(TRANSFER_TIMEOUT).unwrap(), Some(23));
}

#[test]
fn a_handoff_offer_rejects_foreign_or_empty_handles() {
    let offer = encode_offer([4, 8, 12, 16]);
    assert_eq!(decode_offer(offer).unwrap(), [4, 8, 12, 16]);
    let mut foreign = offer;
    foreign[0] = 0;
    assert_eq!(decode_offer(foreign).unwrap_err().kind(), io::ErrorKind::InvalidData);
    for invalid in [0, usize::MAX] {
        assert_eq!(decode_offer(encode_offer([4, 8, invalid, 16])).unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}

#[test]
fn a_resize_message_encodes_three_bounded_little_endian_words() {
    assert_eq!(resize_packet(312, 43), [8, 0, 56, 1, 43, 0]);
    assert_eq!(resize_packet(u16::MAX, 0), [8, 0, 255, 127, 1, 0]);
}
