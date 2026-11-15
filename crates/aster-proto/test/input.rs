use super::*;

#[test]
fn backspace_default() {
    assert_eq!(map_backspace(b"a\x7fb", Backspace::default()), b"a\x7fb");
}

#[test]
fn backspace_ctrl_h() {
    assert_eq!(map_backspace(b"\x7fa\x7f", Backspace::CtrlH), b"\x08a\x08");
}

#[test]
fn backspace_ctrl_question() {
    assert_eq!(map_backspace(b"a\x7fb", Backspace::CtrlQuestion), b"a\x7fb");
}

#[test]
fn backspace_delete() {
    assert_eq!(map_backspace(b"a\x7fb", Backspace::Delete), b"a\x1b[3~b");
}

#[test]
fn normal_preserves_all_bytes() {
    let bytes: Vec<u8> = (0..=255).collect();
    let result = InputProcessor::new(InputMode::Normal).feed(&bytes);
    assert_eq!(result.send, bytes);
    assert!(result.echo.is_empty());
}

#[test]
fn local_echo_filters_controls_and_sequences() {
    let keys = b"a\r\x7f\x08\x01\x1b[A\x1bOPz";
    let result = InputProcessor::new(InputMode::LocalEcho).feed(keys);
    assert_eq!(result.send, keys);
    assert_eq!(result.echo, b"a\r\n\x08 \x08\x08 \x08z");
}

#[test]
fn readline_waits_for_enter() {
    let mut input = InputProcessor::new(InputMode::Readline);
    assert_eq!(
        input.feed(b"word"),
        Processed {
            send: vec![],
            echo: b"word".to_vec()
        }
    );
    assert_eq!(
        input.feed(b"\r"),
        Processed {
            send: b"word\r".to_vec(),
            echo: b"\r\n".to_vec()
        }
    );
    assert_eq!(input.feed(b"\n").send, b"\r");
}

#[test]
fn backspace_removes_unicode_character() {
    let mut input = InputProcessor::new(InputMode::Readline);
    input.feed("при".as_bytes());
    assert_eq!(input.feed(b"\x7f").echo, b"\x08 \x08");
    assert_eq!(input.feed(b"\r").send, "пр\r".as_bytes());
}

#[test]
fn empty_backspace_is_silent() {
    assert_eq!(
        InputProcessor::new(InputMode::Readline).feed(b"\x08\x7f"),
        Processed::default()
    );
}

#[test]
fn interrupt_clears_line() {
    let mut input = InputProcessor::new(InputMode::Readline);
    input.feed(b"word");
    assert_eq!(
        input.feed(b"\x03"),
        Processed {
            send: vec![3],
            echo: b"^C\r\n".to_vec()
        }
    );
    assert_eq!(input.feed(b"\r").send, b"\r");
}

#[test]
fn kill_line_erases_characters() {
    let mut input = InputProcessor::new(InputMode::Readline);
    input.feed("я🙂".as_bytes());
    assert_eq!(input.feed(b"\x15").echo, b"\x08 \x08\x08 \x08");
    assert_eq!(input.feed(b"\r").send, b"\r");
}

#[test]
fn split_escape_sequences_are_ignored() {
    for mode in [InputMode::Readline, InputMode::LocalEcho] {
        let mut input = InputProcessor::new(mode);
        assert_eq!(input.feed(b"a\x1b").echo, b"a");
        assert!(input.feed(b"[1;").echo.is_empty());
        assert_eq!(input.feed(b"5Dz\x1bOb").echo, b"z");
    }
}

#[test]
fn split_utf8_is_preserved() {
    let mut input = InputProcessor::new(InputMode::Readline);
    let bytes = "я🙂".as_bytes();
    let mut echo = Vec::new();
    for byte in bytes {
        echo.extend(input.feed(&[*byte]).echo);
    }
    assert_eq!(echo, bytes);
    assert_eq!(input.feed(b"\r").send, "я🙂\r".as_bytes());
}

#[test]
fn invalid_utf8_is_replaced() {
    assert_eq!(
        InputProcessor::new(InputMode::LocalEcho)
            .feed(b"\xffa")
            .echo,
        "�a".as_bytes()
    );
}

#[test]
fn readline_ignores_other_controls() {
    let mut input = InputProcessor::new(InputMode::Readline);
    assert_eq!(input.feed(b"a\x00\t\x1bxz\r").send, b"az\r");
}

#[test]
fn hex_accepts_prefixes_and_multiple_bytes() {
    assert_eq!(
        InputProcessor::new(InputMode::ReadlineHex)
            .feed(b"0x0a 1B ff 0X0203\r")
            .send,
        [10, 27, 255, 2, 3]
    );
}

#[test]
fn hex_rejects_odd_token() {
    let result = InputProcessor::new(InputMode::ReadlineHex).feed(b"aa f\r");
    assert!(result.send.is_empty());
    assert_eq!(result.echo, b"aa f\r\n[invalid hex]\r\n");
}

#[test]
fn hex_rejects_non_hex_and_bare_prefix() {
    for text in [b"gg\r".as_slice(), b"0x\r", "éé\r".as_bytes()] {
        let result = InputProcessor::new(InputMode::ReadlineHex).feed(text);
        assert!(result.send.is_empty());
        assert!(result.echo.ends_with(b"[invalid hex]\r\n"));
    }
}

#[test]
fn hex_empty_and_error_clear_buffer() {
    let mut input = InputProcessor::new(InputMode::ReadlineHex);
    assert_eq!(
        input.feed(b"\r"),
        Processed {
            send: vec![],
            echo: b"\r\n".to_vec()
        }
    );
    input.feed(b"g\r");
    assert_eq!(input.feed(b"ff\r").send, [255]);
}

#[test]
fn multiple_lines_accumulate_send() {
    assert_eq!(
        InputProcessor::new(InputMode::Readline)
            .feed(b"a\rb\n")
            .send,
        b"a\rb\r"
    );
}
