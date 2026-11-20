use super::*;

fn console() -> (Console, crossbeam_channel::Receiver<Vec<u8>>, tokio::sync::mpsc::UnboundedSender<Input>) {
    let (out, shown) = crossbeam_channel::unbounded();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (Console::new(out, rx, (80, 24)), shown, tx)
}

fn shown(rx: &crossbeam_channel::Receiver<Vec<u8>>) -> String {
    rx.try_iter().map(|b| String::from_utf8_lossy(&b).into_owned()).collect()
}

fn block<F: std::future::Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f)
}

#[test]
fn a_secret_is_read_without_echo() {
    let (mut c, out, tx) = console();
    tx.send(Input::Data(b"hun".to_vec())).unwrap();
    tx.send(Input::Data(b"ter2\x7f2\r".to_vec())).unwrap();
    assert_eq!(block(c.ask("Password: ", false)).as_deref(), Some("hunter2"));
    assert_eq!(shown(&out), "Password: \r\n");
}

#[test]
fn a_visible_answer_is_echoed_and_resizes_are_remembered() {
    let (mut c, out, tx) = console();
    tx.send(Input::Resize(120, 40)).unwrap();
    tx.send(Input::Data(b"yes\r".to_vec())).unwrap();
    assert_eq!(block(c.ask("Continue? ", true)).as_deref(), Some("yes"));
    assert_eq!(shown(&out), "Continue? yes\r\n");
    assert_eq!(c.size, (120, 40));
}

#[test]
fn ctrl_c_or_closing_the_pane_cancels_the_question() {
    let (mut c, _, tx) = console();
    tx.send(Input::Data(b"ab\x03".to_vec())).unwrap();
    assert_eq!(block(c.ask("? ", true)), None);
    tx.send(Input::Close).unwrap();
    assert_eq!(block(c.ask("? ", true)), None);
    assert!(c.closed);
}

#[test]
fn printed_text_uses_terminal_line_endings() {
    assert_eq!(crlf("a\nb\r\nc"), "a\r\nb\r\nc");
}

#[test]
fn service_lines_reset_each_line_and_escape_terminal_controls() {
    let (c, out, _) = console();
    c.note("one\r\ntwo\n");
    c.warn("warning");
    c.error("bad\x1b[2J");
    assert_eq!(shown(&out), "\x1b[2mone\x1b[0m\r\n\x1b[2mtwo\x1b[0m\r\n\x1b[33mwarning\x1b[0m\r\n\x1b[31mbad[2J\x1b[0m\r\n");
}
