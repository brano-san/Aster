use super::*;

#[test]
fn extended_frames_and_flow_round_trip() {
    for (input, expected) in [
        ("COM3:?,8m1,slow", "COM3:?,8M1,slow"),
        ("COM3:9600,8s1,XAny", "COM3:9600,8S1,xany"),
        ("COM3:9600,8n1.5", "COM3:9600,8N1.5"),
    ] {
        let settings = parse(input).unwrap();
        assert_eq!(format(&settings), expected);
        assert_eq!(parse(expected), Ok(settings));
    }
    assert!(parse("COM3,8N1.2").is_err());
}

#[test]
fn partial_frames_inherit_fractional_stop_bits_and_xany() {
    let defaults = Defaults::from_spec("COM3:9600,5M1.5,xany").unwrap();
    assert_eq!(defaults.resolve(",?S?").as_deref(), Some("COM3:9600,5S1.5,xany"));
}
