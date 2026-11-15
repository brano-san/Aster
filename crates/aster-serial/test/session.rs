use super::*;
use crossbeam_channel::RecvTimeoutError;
use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Instant;

const WAIT: Duration = Duration::from_secs(5);

#[derive(Default)]
struct State {
    to_host: VecDeque<u8>,
    from_host: Vec<u8>,
    bauds: Vec<u32>,
    breaks: usize,
    timeouts: usize,
    read_failure: bool,
    write_failure: bool,
    clone_failure: bool,
}

#[derive(Default)]
struct Line {
    state: Mutex<State>,
    changed: Condvar,
}

#[derive(Clone, Default)]
struct Device(Arc<Line>);

impl Device {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap()
    }

    fn update(&self, change: impl FnOnce(&mut State)) {
        change(&mut self.lock());
        self.0.changed.notify_all();
    }

    fn send(&self, bytes: &[u8]) {
        self.update(|state| state.to_host.extend(bytes));
    }

    fn wait_for<T>(&self, mut probe: impl FnMut(&State) -> Option<T>) -> T {
        let deadline = Instant::now() + WAIT;
        let mut state = self.lock();
        loop {
            if let Some(value) = probe(&state) {
                return value;
            }
            let left = deadline
                .checked_duration_since(Instant::now())
                .expect("the device state never reached the expected value");
            state = self.0.changed.wait_timeout(state, left).unwrap().0;
        }
    }

    fn port(&self) -> Box<dyn Port> {
        Box::new(TestPort(self.clone()))
    }
}

struct TestPort(Device);

impl Read for TestPort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let line = &self.0 .0;
        let state = line.state.lock().unwrap();
        let (mut state, _) = line
            .changed
            .wait_timeout_while(state, Duration::from_millis(10), |s| {
                s.to_host.is_empty() && !s.read_failure
            })
            .unwrap();
        if state.read_failure {
            return Err(io::Error::new(ErrorKind::BrokenPipe, "device removed"));
        }
        if state.to_host.is_empty() {
            state.timeouts += 1;
            drop(state);
            line.changed.notify_all();
            return Err(ErrorKind::TimedOut.into());
        }
        let n = buf.len().min(state.to_host.len()).min(3);
        for (slot, byte) in buf.iter_mut().zip(state.to_host.drain(..n)) {
            *slot = byte;
        }
        Ok(n)
    }
}

impl Write for TestPort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.0.lock().write_failure {
            return Err(io::Error::new(ErrorKind::BrokenPipe, "device removed"));
        }
        self.0
            .update(|state| state.from_host.extend_from_slice(buf));
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Port for TestPort {
    fn set_baud(&mut self, baud: u32) -> io::Result<()> {
        self.0.update(|state| state.bauds.push(baud));
        Ok(())
    }

    fn send_break(&mut self) -> io::Result<()> {
        self.0.update(|state| state.breaks += 1);
        Ok(())
    }

    fn try_clone_port(&self) -> io::Result<Box<dyn Port>> {
        if self.0.lock().clone_failure {
            return Err(io::Error::other("cannot clone"));
        }
        Ok(self.0.port())
    }
}

fn collect(connection: &Connection, len: usize) -> Vec<u8> {
    let mut received = Vec::new();
    while received.len() < len {
        received.extend(
            connection
                .output
                .recv_timeout(WAIT)
                .expect("output did not arrive"),
        );
    }
    received
}

fn assert_ended(connection: &Connection, code: u32) {
    assert_eq!(connection.exit.recv_timeout(WAIT), Ok(code));
    loop {
        match connection.output.recv_timeout(WAIT) {
            Ok(_) => continue,
            Err(error) => {
                assert_eq!(error, RecvTimeoutError::Disconnected);
                break;
            }
        }
    }
}

#[test]
fn bytes_sent_by_the_device_arrive_on_output_in_order() {
    let device = Device::default();
    let connection = start(device.port());
    let message: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
    device.send(&message[..400]);
    device.send(&message[400..]);
    assert_eq!(collect(&connection, message.len()), message);
}

#[test]
fn data_input_reaches_the_device() {
    let device = Device::default();
    let connection = start(device.port());
    connection
        .input
        .send(Input::Data(b"AT\r".to_vec()))
        .unwrap();
    connection
        .input
        .send(Input::Data(b"ATI\r".to_vec()))
        .unwrap();
    let written = device.wait_for(|s| (s.from_host.len() >= 7).then(|| s.from_host.clone()));
    assert_eq!(written, b"AT\rATI\r");
}

#[test]
fn baud_changes_and_breaks_are_applied_to_the_port() {
    let device = Device::default();
    let connection = start(device.port());
    connection.input.send(Input::SetBaud(9600)).unwrap();
    connection.input.send(Input::Break).unwrap();
    connection.input.send(Input::SetBaud(57600)).unwrap();
    let (bauds, breaks) =
        device.wait_for(|s| (s.bauds.len() == 2).then(|| (s.bauds.clone(), s.breaks)));
    assert_eq!(bauds, [9600, 57600]);
    assert_eq!(breaks, 1);
}

#[test]
fn close_ends_the_session_with_code_zero_and_closes_output() {
    let device = Device::default();
    let connection = start(device.port());
    device.send(b"hello");
    assert_eq!(collect(&connection, 5), b"hello");
    connection.input.send(Input::Close).unwrap();
    assert_ended(&connection, 0);
}

#[test]
fn dropping_every_input_sender_ends_the_session_like_close() {
    let device = Device::default();
    let Connection {
        output,
        input,
        exit,
    } = start(device.port());
    drop(input);
    assert_eq!(exit.recv_timeout(WAIT), Ok(0));
    assert_eq!(
        output.recv_timeout(WAIT),
        Err(RecvTimeoutError::Disconnected)
    );
}

#[test]
fn a_fatal_read_error_ends_the_session_with_code_one() {
    let device = Device::default();
    let connection = start(device.port());
    device.send(b"last");
    assert_eq!(collect(&connection, 4), b"last");
    device.update(|state| state.read_failure = true);
    assert_ended(&connection, 1);
}

#[test]
fn a_fatal_write_error_ends_the_session_with_code_one() {
    let device = Device::default();
    let connection = start(device.port());
    device.update(|state| state.write_failure = true);
    connection.input.send(Input::Data(b"x".to_vec())).unwrap();
    assert_ended(&connection, 1);
}

#[test]
fn read_timeouts_do_not_end_the_session() {
    let device = Device::default();
    let connection = start(device.port());
    device.wait_for(|s| (s.timeouts >= 5).then_some(()));
    assert_eq!(connection.exit.try_recv(), Err(TryRecvError::Empty));
    device.send(b"still here");
    assert_eq!(collect(&connection, 10), b"still here");
    connection.input.send(Input::Data(b"ok".to_vec())).unwrap();
    device.wait_for(|s| (s.from_host == b"ok").then_some(()));
    connection.input.send(Input::Close).unwrap();
    assert_ended(&connection, 0);
}

#[test]
fn a_port_that_cannot_be_shared_between_threads_ends_at_once_with_code_one() {
    let device = Device::default();
    device.update(|state| state.clone_failure = true);
    let connection = start(device.port());
    assert_ended(&connection, 1);
}

#[test]
fn opening_a_missing_port_reports_its_name() {
    let settings = Settings {
        port: "aster-test-missing-port".to_owned(),
        ..Settings::default()
    };
    let error = open(&settings).err().expect("a missing port must not open");
    assert!(error.contains("aster-test-missing-port"), "{error}");
}

#[test]
fn opening_with_unsupported_frame_values_is_refused_before_touching_the_port() {
    let error = open(&Settings {
        port: "P".to_owned(),
        data_bits: 9,
        ..Settings::default()
    })
    .err()
    .unwrap();
    assert!(error.contains("data bits"), "{error}");
    let error = open(&Settings {
        port: "P".to_owned(),
        stop_bits: 3,
        ..Settings::default()
    })
    .err()
    .unwrap();
    assert!(error.contains("stop bits"), "{error}");
}
