use super::*;

fn step(expect: &str, send: &str) -> Script {
    Script { expect: expect.to_owned(), send: send.to_owned(), ..Script::default() }
}

#[test]
fn plain_options_pass_bytes_through() {
    let options = StreamOptions::default();
    assert!(options.is_plain());
    let mut outgoing = Outgoing::new(&options);
    let sent = outgoing.feed(b"ls\r\x7f\x1b[A");
    assert_eq!(sent.send, b"ls\r\x7f\x1b[A");
    assert!(sent.echo.is_empty());
    let mut incoming = Incoming::new(&options).0;
    assert!(incoming.start().is_empty());
    assert_eq!(incoming.feed(b"a\nb").display, b"a\nb");
}

#[test]
fn line_mode_echoes_and_sends_whole_lines_with_converted_newlines() {
    let options = StreamOptions { input: InputMode::Readline, newlines_out: Newlines::CrLf, ..StreamOptions::default() };
    assert!(!options.is_plain());
    let mut outgoing = Outgoing::new(&options);
    let typed = outgoing.feed(b"shw\x7fow");
    assert!(typed.send.is_empty());
    assert_eq!(typed.echo, b"shw\x08 \x08ow");
    let entered = outgoing.feed(b"\r");
    assert_eq!(entered.send, b"show\r\n");
    assert_eq!(entered.echo, b"\r\n");
}

#[test]
fn backspace_is_mapped_before_sending() {
    let options = StreamOptions { backspace: Backspace::CtrlH, ..StreamOptions::default() };
    assert_eq!(Outgoing::new(&options).feed(b"a\x7f").send, b"a\x08");
}

#[test]
fn hex_input_sends_parsed_bytes() {
    let options = StreamOptions { input: InputMode::ReadlineHex, ..StreamOptions::default() };
    let mut outgoing = Outgoing::new(&options);
    outgoing.feed(b"de ad");
    assert_eq!(outgoing.feed(b"\r").send, [0xde, 0xad]);
}

#[test]
fn incoming_newlines_and_hex_view() {
    let options = StreamOptions { newlines_in: Newlines::ImplicitCr, ..StreamOptions::default() };
    assert_eq!(Incoming::new(&options).0.feed(b"a\nb").display, b"a\r\nb");
    let options = StreamOptions { hex_output: true, ..StreamOptions::default() };
    let shown = Incoming::new(&options).0.feed(b"AB").display;
    assert!(String::from_utf8(shown).unwrap().starts_with("00000000  41 42"));
}

#[test]
fn login_script_replies_to_raw_output_and_then_stops() {
    let options = StreamOptions {
        hex_output: true,
        login: vec![step("", r"\r"), step("login:", r"admin\r"), step("Password:", r"secret\r")],
        ..StreamOptions::default()
    };
    let mut incoming = Incoming::new(&options).0;
    assert_eq!(incoming.start(), b"\r");
    assert_eq!(incoming.feed(b"\x1b[1mlog").reply, b"");
    assert_eq!(incoming.feed(b"in:").reply, b"admin\r");
    assert_eq!(incoming.feed(b"Password:").reply, b"secret\r");
    assert!(incoming.feed(b"Password:").reply.is_empty());
}

#[test]
fn a_bad_login_pattern_is_reported() {
    let options = StreamOptions { login: vec![Script { expect: "(".to_owned(), regex: true, ..Script::default() }], ..StreamOptions::default() };
    let (mut incoming, error) = Incoming::new(&options);
    assert!(error.is_some_and(|e| e.starts_with("Script 0")));
    assert_eq!(incoming.feed(b"(").display, b"(");
    assert!(incoming.start().is_empty());
}
