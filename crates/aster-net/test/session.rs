use super::*;
use std::net::TcpListener;
use std::time::Instant;

const WAIT: Duration = Duration::from_secs(5);

fn options() -> Options {
    Options { term: "xterm-256color".to_owned(), cols: 80, rows: 24, echo: true }
}

fn serve(protocol: Protocol) -> (Connection, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let connection = open(Address { protocol, host: "127.0.0.1".to_owned(), port }, options());
    let (server, _) = listener.accept().unwrap();
    server.set_read_timeout(Some(WAIT)).unwrap();
    (connection, server)
}

fn read_exact(server: &mut TcpStream, len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    server.read_exact(&mut buf).unwrap();
    buf
}

fn output_until(connection: &Connection, expected: &[u8]) -> Vec<u8> {
    let deadline = Instant::now() + WAIT;
    let mut seen = Vec::new();
    while !seen.windows(expected.len()).any(|w| w == expected) {
        let left = deadline.checked_duration_since(Instant::now()).unwrap_or_else(|| panic!("never saw {:?} in {:?}", String::from_utf8_lossy(expected), String::from_utf8_lossy(&seen)));
        match connection.output.recv_timeout(left) {
            Ok(bytes) => seen.extend(bytes),
            Err(_) => panic!("output ended before {:?}, got {:?}", String::from_utf8_lossy(expected), String::from_utf8_lossy(&seen)),
        }
    }
    seen
}

#[test]
fn telnet_negotiates_window_size_and_terminal_type() {
    let (connection, mut server) = serve(Protocol::Telnet);
    server.write_all(&[255, 253, 31]).unwrap();
    assert_eq!(read_exact(&mut server, 3), [255, 251, 31]);
    assert_eq!(read_exact(&mut server, 9), [255, 250, 31, 0, 80, 0, 24, 255, 240]);
    connection.input.send(Input::Resize(100, 30)).unwrap();
    assert_eq!(read_exact(&mut server, 9), [255, 250, 31, 0, 100, 0, 30, 255, 240]);
    server.write_all(&[255, 253, 24, 255, 250, 24, 1, 255, 240]).unwrap();
    assert_eq!(read_exact(&mut server, 3), [255, 251, 24]);
    let mut reply = vec![255, 250, 24, 0];
    reply.extend_from_slice(b"xterm-256color");
    reply.extend_from_slice(&[255, 240]);
    assert_eq!(read_exact(&mut server, reply.len()), reply);
}

#[test]
fn telnet_echoes_locally_until_the_server_echoes() {
    let (connection, mut server) = serve(Protocol::Telnet);
    connection.input.send(Input::Data(b"ab\r".to_vec())).unwrap();
    assert_eq!(read_exact(&mut server, 4), b"ab\r\n");
    output_until(&connection, b"ab\r\n");
    server.write_all(&[255, 251, 1]).unwrap();
    assert_eq!(read_exact(&mut server, 3), [255, 253, 1]);
    connection.input.send(Input::Data(vec![b'c', 255])).unwrap();
    assert_eq!(read_exact(&mut server, 3), [b'c', 255, 255]);
    server.write_all(b"remote").unwrap();
    let seen = output_until(&connection, b"remote");
    assert!(!seen.contains(&b'c'));
}

#[test]
fn telnet_data_reaches_the_terminal_without_commands() {
    let (connection, mut server) = serve(Protocol::Telnet);
    server.write_all(&[b'h', 255, 251, 3, b'i', 255, 255]).unwrap();
    assert_eq!(read_exact(&mut server, 3), [255, 253, 3]);
    let seen = output_until(&connection, &[b'i', 255]);
    assert_eq!(seen, [b'h', b'i', 255]);
}

#[test]
fn raw_tcp_echoes_and_sends_crlf() {
    let (connection, mut server) = serve(Protocol::Raw);
    server.write_all(b"hello\nworld\r\n").unwrap();
    output_until(&connection, b"hello\r\nworld\r\n");
    connection.input.send(Input::Data(b"x\r".to_vec())).unwrap();
    assert_eq!(read_exact(&mut server, 3), b"x\r\n");
    output_until(&connection, b"x\r\n");
    connection.input.send(Input::Data(vec![255])).unwrap();
    assert_eq!(read_exact(&mut server, 1), [255]);
}

#[test]
fn remote_close_ends_the_session_cleanly() {
    let (connection, server) = serve(Protocol::Telnet);
    drop(server);
    assert_eq!(connection.exit.recv_timeout(WAIT), Ok(0));
}

#[test]
fn closing_the_session_closes_the_socket() {
    let (connection, mut server) = serve(Protocol::Raw);
    connection.input.send(Input::Close).unwrap();
    assert_eq!(connection.exit.recv_timeout(WAIT), Ok(0));
    let mut buf = [0u8; 1];
    assert!(matches!(server.read(&mut buf), Ok(0) | Err(_)));
}

#[test]
fn refused_connection_reports_and_fails() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let connection = open(Address { protocol: Protocol::Telnet, host: "127.0.0.1".to_owned(), port }, options());
    assert_eq!(connection.exit.recv_timeout(Duration::from_secs(15)), Ok(1));
    let text: Vec<u8> = connection.output.try_iter().flatten().collect();
    assert!(String::from_utf8_lossy(&text).starts_with(&format!("Cannot connect to 127.0.0.1:{port}: ")));
}

#[test]
fn echo_can_be_left_to_the_stream_layer() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let connection = open(Address { protocol: Protocol::Raw, host: "127.0.0.1".to_owned(), port }, Options { echo: false, ..options() });
    let (mut server, _) = listener.accept().unwrap();
    server.set_read_timeout(Some(WAIT)).unwrap();
    connection.input.send(Input::Data(b"q\r".to_vec())).unwrap();
    assert_eq!(read_exact(&mut server, 3), b"q\r\n");
    server.write_all(b"done").unwrap();
    assert_eq!(output_until(&connection, b"done"), b"done");
}
