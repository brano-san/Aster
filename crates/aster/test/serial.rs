use super::*;
use std::time::Duration;

const WAIT: Duration = Duration::from_secs(5);

fn failed_attempt(serial: &mut Serial) -> Option<u32> {
    let (notice, received) = bounded(1);
    let mut spawned = serial.connect_with(|_| Err("port unavailable".into()), move |s| notice.send(s).unwrap());
    let wait = std::thread::spawn(spawned.wait);
    let mut output = String::new();
    spawned.reader.read_to_string(&mut output).unwrap();
    assert!(output.is_empty());
    assert_eq!(received.recv_timeout(WAIT).unwrap(), "port unavailable");
    wait.join().unwrap()
}

#[test]
fn initial_open_failure_uses_the_normal_exit_path() {
    let mut serial = Serial::new("COM3:19200", None).unwrap();
    let code = failed_attempt(&mut serial);
    assert_eq!(code, Some(1));
    assert!(!serial.disconnected(code));
}

#[test]
fn unplug_retries_the_same_port_and_settings_until_it_opens_again() {
    let mut serial = Serial::new("COM3:19200,7E2,rtscts,slow", None).unwrap();
    for rate in [19200, 38400] {
        let expected = settings::parse(&format!("COM3:{rate},7E2,rtscts,slow")).unwrap();
        let (device, output) = unbounded();
        let (input, from_host) = unbounded();
        let (unplug, exit) = bounded(1);
        let (feedback, control) = unbounded();
        let (notice, received) = unbounded();
        let mut spawned = serial.connect_with(move |settings| {
            assert_eq!(settings, &expected);
            Ok(session::Connection { output, input, exit, control })
        }, move |s| notice.send(s).unwrap());
        assert_eq!(serial.keys(b"\x1b"), KeysAction::Write);
        let wait = std::thread::spawn(spawned.wait);
        spawned.writer.write_all(b"hello").unwrap();
        assert_eq!(from_host.recv_timeout(WAIT).unwrap(), Input::Data(b"hello".to_vec()));
        serial.control(SerialControl::Baud(38400));
        serial.control(SerialControl::Break);
        assert_eq!(from_host.recv_timeout(WAIT).unwrap(), Input::SetBaud(38400));
        assert_eq!(from_host.recv_timeout(WAIT).unwrap(), Input::Break);
        feedback.send(DriverControl::Baud(38400)).unwrap();
        serial.control(SerialControl::Baud(65432));
        assert_eq!(from_host.recv_timeout(WAIT).unwrap(), Input::SetBaud(65432));
        feedback.send(DriverControl::BaudFailed(65432, "unsupported rate".into())).unwrap();
        feedback.send(DriverControl::BreakFailed("line unavailable".into())).unwrap();
        drop(feedback);
        device.send(b"final bytes".to_vec()).unwrap();
        drop(device);
        unplug.send(1).unwrap();
        let mut output = Vec::new();
        spawned.reader.read_to_end(&mut output).unwrap();
        assert_eq!(output, b"final bytes");
        assert!(serial.disconnected(wait.join().unwrap()));
        assert!(received.recv_timeout(WAIT).unwrap().contains("unsupported rate"));
        assert!(received.recv_timeout(WAIT).unwrap().contains("line unavailable"));
        assert_eq!(serial.keys(b"\x1b[A"), KeysAction::Ignore);
        assert_eq!(serial.keys(b"hello\r"), KeysAction::Ignore);
        assert_eq!(serial.keys(b"\r"), KeysAction::Reconnect);
        let code = failed_attempt(&mut serial);
        assert!(serial.disconnected(code));
        assert_eq!(serial.keys(b"\x1b"), KeysAction::Close);
        assert_eq!(serial.keys(b"\n"), KeysAction::Reconnect);
    }
}

#[test]
fn line_mode_finishes_cleanly_when_the_user_closes_an_open_port() {
    let stream = Stream { input: aster_core::profile::InputMode::Line, ..Stream::default() };
    let mut serial = Serial::new("COM4", Some(stream)).unwrap();
    let (device, output) = unbounded();
    let (input, from_host) = unbounded();
    let (closed, exit) = bounded(1);
    let (_feedback, control) = unbounded();
    let mut spawned = serial.connect_with(move |_| Ok(session::Connection { output, input, exit, control }), |s| panic!("{s}"));
    let wait = std::thread::spawn(spawned.wait);
    spawned.writer.write_all(b"ab").unwrap();
    assert!(from_host.try_recv().is_err());
    spawned.writer.write_all(b"\r").unwrap();
    assert_eq!(from_host.recv_timeout(WAIT).unwrap(), Input::Data(b"ab\r".to_vec()));
    spawned.pty.kill();
    assert_eq!(from_host.recv_timeout(WAIT).unwrap(), Input::Close);
    device.send(b"last response".to_vec()).unwrap();
    drop(device);
    closed.send(0).unwrap();
    let mut output = String::new();
    spawned.reader.read_to_string(&mut output).unwrap();
    assert!(output.contains("last response"));
    let code = wait.join().unwrap();
    assert_eq!(code, Some(0));
    assert!(!serial.disconnected(code));
}
