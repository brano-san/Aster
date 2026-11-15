use super::*;

fn connection() -> Telnet {
    Telnet::new("xterm-256color", 80, 24)
}

#[test]
fn plain_data_is_one_event() {
    assert_eq!(
        connection().receive(b"hello\r\n"),
        vec![TelnetEvent::Data(b"hello\r\n".to_vec())]
    );
}

#[test]
fn escaped_iac_stays_in_data() {
    assert_eq!(
        connection().receive(&[65, 255, 255, 66]),
        vec![TelnetEvent::Data(vec![65, 255, 66])]
    );
}

#[test]
fn echo_enable_and_disable() {
    let mut telnet = connection();
    assert_eq!(
        telnet.receive(&[255, 251, 1]),
        vec![
            TelnetEvent::Send(vec![255, 253, 1]),
            TelnetEvent::RemoteEcho(true)
        ]
    );
    assert_eq!(
        telnet.receive(&[255, 252, 1]),
        vec![
            TelnetEvent::Send(vec![255, 254, 1]),
            TelnetEvent::RemoteEcho(false)
        ]
    );
}

#[test]
fn repeated_echo_negotiation_is_silent() {
    let mut telnet = connection();
    telnet.receive(&[255, 251, 1]);
    assert!(telnet.receive(&[255, 251, 1]).is_empty());
    telnet.receive(&[255, 252, 1]);
    assert!(telnet.receive(&[255, 252, 1]).is_empty());
    assert_eq!(telnet.receive(&[255, 251, 1]).len(), 2);
}

#[test]
fn sga_negotiates_each_direction_independently() {
    let mut telnet = connection();
    assert_eq!(
        telnet.receive(&[255, 251, 3, 255, 253, 3]),
        vec![
            TelnetEvent::Send(vec![255, 253, 3]),
            TelnetEvent::Send(vec![255, 251, 3])
        ]
    );
    assert!(telnet.receive(&[255, 251, 3, 255, 253, 3]).is_empty());
    assert_eq!(
        telnet.receive(&[255, 252, 3, 255, 254, 3]),
        vec![
            TelnetEvent::Send(vec![255, 254, 3]),
            TelnetEvent::Send(vec![255, 252, 3])
        ]
    );
}

#[test]
fn terminal_type_negotiation_and_request() {
    let mut telnet = connection();
    assert_eq!(
        telnet.receive(&[255, 253, 24]),
        vec![TelnetEvent::Send(vec![255, 251, 24])]
    );
    let mut response = vec![255, 250, 24, 0];
    response.extend_from_slice(b"xterm-256color");
    response.extend_from_slice(&[255, 240]);
    assert_eq!(
        telnet.receive(&[255, 250, 24, 1, 255, 240]),
        vec![TelnetEvent::Send(response)]
    );
}

#[test]
fn naws_negotiation_sends_dimensions() {
    let mut telnet = connection();
    assert_eq!(
        telnet.receive(&[255, 253, 31]),
        vec![
            TelnetEvent::Send(vec![255, 251, 31]),
            TelnetEvent::Send(vec![255, 250, 31, 0, 80, 0, 24, 255, 240])
        ]
    );
    assert!(telnet.receive(&[255, 253, 31]).is_empty());
}

#[test]
fn naws_escapes_every_iac_dimension_byte() {
    let events = Telnet::new("term", 255, 65535).receive(&[255, 253, 31]);
    assert_eq!(
        events[1],
        TelnetEvent::Send(vec![
            255, 250, 31, 0, 255, 255, 255, 255, 255, 255, 255, 240
        ])
    );
}

#[test]
fn resize_before_negotiation_remembers_dimensions() {
    let mut telnet = connection();
    assert_eq!(telnet.resize(100, 40), None);
    assert_eq!(
        telnet.receive(&[255, 253, 31])[1],
        TelnetEvent::Send(vec![255, 250, 31, 0, 100, 0, 40, 255, 240])
    );
}

#[test]
fn resize_after_negotiation_returns_packet() {
    let mut telnet = connection();
    telnet.receive(&[255, 253, 31]);
    assert_eq!(
        telnet.resize(511, 256),
        Some(vec![255, 250, 31, 1, 255, 255, 1, 0, 255, 240])
    );
}

#[test]
fn disabling_naws_stops_resize_packets() {
    let mut telnet = connection();
    telnet.receive(&[255, 253, 31]);
    assert_eq!(
        telnet.receive(&[255, 254, 31]),
        vec![TelnetEvent::Send(vec![255, 252, 31])]
    );
    assert_eq!(telnet.resize(90, 30), None);
    assert_eq!(
        telnet.receive(&[255, 253, 31])[1],
        TelnetEvent::Send(vec![255, 250, 31, 0, 90, 0, 30, 255, 240])
    );
}

#[test]
fn unknown_options_are_refused_without_repeated_replies() {
    let mut telnet = connection();
    assert_eq!(
        telnet.receive(&[255, 251, 99, 255, 253, 99]),
        vec![
            TelnetEvent::Send(vec![255, 254, 99]),
            TelnetEvent::Send(vec![255, 252, 99])
        ]
    );
    assert!(telnet.receive(&[255, 251, 99, 255, 253, 99]).is_empty());
    assert!(telnet.receive(&[255, 252, 99, 255, 254, 99]).is_empty());
    assert_eq!(
        telnet.receive(&[255, 251, 99]),
        vec![TelnetEvent::Send(vec![255, 254, 99])]
    );
}

#[test]
fn disabling_inactive_options_is_silent() {
    let mut telnet = connection();
    assert!(telnet
        .receive(&[255, 252, 1, 255, 254, 3, 255, 254, 24, 255, 254, 31, 255, 252, 99])
        .is_empty());
}

#[test]
fn negotiation_split_across_calls() {
    let mut telnet = connection();
    assert!(telnet.receive(&[255]).is_empty());
    assert_eq!(
        telnet.receive(&[251, 1]),
        vec![
            TelnetEvent::Send(vec![255, 253, 1]),
            TelnetEvent::RemoteEcho(true)
        ]
    );
}

#[test]
fn data_and_commands_keep_event_order() {
    assert_eq!(
        connection().receive(&[65, 255, 251, 1, 66]),
        vec![
            TelnetEvent::Data(vec![65]),
            TelnetEvent::Send(vec![255, 253, 1]),
            TelnetEvent::RemoteEcho(true),
            TelnetEvent::Data(vec![66])
        ]
    );
}

#[test]
fn outgoing_iac_is_doubled() {
    assert_eq!(
        Telnet::escape_outgoing(&[0, 255, 255, 1]),
        [0, 255, 255, 255, 255, 1]
    );
    assert_eq!(Telnet::escape_outgoing(b"abc"), b"abc");
    assert!(Telnet::escape_outgoing(b"").is_empty());
}

#[test]
fn ignored_commands_and_subnegotiations_do_not_leak_data() {
    let mut telnet = connection();
    assert!(telnet
        .receive(&[255, 241, 255, 249, 255, 250, 99, 65, 255, 255, 66, 255, 240])
        .is_empty());
    assert_eq!(
        telnet.receive(b"ok"),
        vec![TelnetEvent::Data(b"ok".to_vec())]
    );
}

#[test]
fn subnegotiation_survives_every_split_position() {
    let bytes = [255, 253, 24, 255, 250, 24, 1, 255, 240];
    let expected = connection().receive(&bytes);
    for split in 0..=bytes.len() {
        let mut telnet = connection();
        let mut events = telnet.receive(&bytes[..split]);
        events.extend(telnet.receive(&bytes[split..]));
        assert_eq!(events, expected, "{split}");
    }
}

#[test]
fn escaped_iac_can_cross_calls() {
    let mut telnet = connection();
    assert_eq!(
        telnet.receive(&[65, 255]),
        vec![TelnetEvent::Data(vec![65])]
    );
    assert_eq!(
        telnet.receive(&[255, 66]),
        vec![TelnetEvent::Data(vec![255, 66])]
    );
}

#[test]
fn empty_receive_is_silent() {
    assert!(connection().receive(b"").is_empty());
}

#[test]
fn non_ascii_terminal_name_uses_replacement() {
    let mut telnet = Telnet::new("aя🙂", 0, 0);
    assert_eq!(
        telnet.receive(&[255, 250, 24, 1, 255, 240]),
        vec![TelnetEvent::Send(vec![
            255, 250, 24, 0, b'a', b'?', b'?', 255, 240
        ])]
    );
}

#[test]
fn malformed_terminal_type_request_is_ignored() {
    assert!(connection()
        .receive(&[255, 250, 24, 1, 2, 255, 240])
        .is_empty());
    assert!(connection()
        .receive(&[255, 250, 24, 0, 255, 240])
        .is_empty());
}
