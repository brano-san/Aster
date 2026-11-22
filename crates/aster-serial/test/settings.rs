use super::*;

fn port(name: &str) -> Settings {
    Settings {
        port: name.to_owned(),
        ..Settings::default()
    }
}

#[test]
fn defaults_are_115200_baud_8n1_without_flow_control() {
    let s = Settings::default();
    assert_eq!(
        (
            s.port.as_str(),
            s.baud,
            s.data_bits,
            s.parity,
            s.stop_bits,
            s.flow
        ),
        ("", 115_200, 8, Parity::None, StopBits::One, Flow::None)
    );
}

#[test]
fn a_bare_port_takes_every_default() {
    assert_eq!(parse("COM3"), Ok(port("COM3")));
}

#[test]
fn a_port_with_a_baud_rate_keeps_the_default_frame() {
    assert_eq!(
        parse("COM3:9600"),
        Ok(Settings {
            baud: 9600,
            ..port("COM3")
        })
    );
}

#[test]
fn ask_baud_and_slow_round_trip_without_losing_port_options() {
    let settings = parse(" dev:port : ? , 7e2 , HARDWARE , SlOw ").unwrap();
    assert_eq!(settings, Settings {
        ask_baud: true, slow: true, data_bits: 7, parity: Parity::Even,
        stop_bits: StopBits::Two, flow: Flow::Hardware, ..port("dev:port")
    });
    assert_eq!(format(&settings), "dev:port:?,7E2,rtscts,slow");
    assert_eq!(parse(&format(&settings)), Ok(settings));
    assert_eq!(format(&parse("COM3:?").unwrap()), "COM3:?,8N1");
    assert_eq!(format(&parse("COM3,slow").unwrap()), "COM3:115200,8N1,slow");
    assert_eq!(format(&parse("COM3:9600,xonxoff,slow").unwrap()), "COM3:9600,8N1,xonxoff,slow");
}

#[test]
fn ask_baud_and_slow_still_reject_invalid_specs() {
    for spec in [":?,slow", "COM3:?,7K2", "COM3:0,slow", "COM3:?,8N1,xonany,slow", "COM3,slow,slow", "COM3,slow,unknown"] {
        assert!(parse(spec).is_err(), "{spec}");
    }
}

#[test]
fn a_spec_with_parity_two_stop_bits_and_hardware_flow_is_parsed() {
    let expected = Settings {
        baud: 115_200,
        data_bits: 7,
        parity: Parity::Even,
        stop_bits: StopBits::Two,
        flow: Flow::Hardware,
        ..port("/dev/ttyUSB0")
    };
    assert_eq!(parse("/dev/ttyUSB0:115200,7E2,rtscts"), Ok(expected));
}

#[test]
fn a_device_path_port_with_software_flow_is_parsed() {
    let expected = Settings {
        baud: 57600,
        flow: Flow::Software,
        ..port(r"\\.\COM10")
    };
    assert_eq!(parse(r"\\.\COM10:57600,8n1,xonxoff"), Ok(expected));
}

#[test]
fn frames_and_flow_names_ignore_case_and_surrounding_whitespace() {
    let expected = Settings {
        baud: 9600,
        data_bits: 5,
        parity: Parity::Odd,
        stop_bits: StopBits::One,
        flow: Flow::Hardware,
        ..port("COM4")
    };
    assert_eq!(
        parse("  COM4 : 9600 , 5o1 , HARDWARE "),
        Ok(expected.clone())
    );
    assert_eq!(parse("COM4:9600,5O1,RtsCts"), Ok(expected));
    assert_eq!(parse("COM4,Software").map(|s| s.flow), Ok(Flow::Software));
    assert_eq!(parse("COM4,8N1,NONE").map(|s| s.flow), Ok(Flow::None));
}

#[test]
fn flow_control_may_follow_the_port_without_a_frame() {
    assert_eq!(
        parse("COM5:19200,rtscts"),
        Ok(Settings {
            baud: 19200,
            flow: Flow::Hardware,
            ..port("COM5")
        })
    );
}

#[test]
fn any_baud_in_range_is_accepted_and_only_digits_after_the_last_colon_count_as_baud() {
    assert_eq!(parse("COM1:12345").map(|s| s.baud), Ok(12345));
    assert_eq!(parse("COM1:50").map(|s| s.baud), Ok(50));
    assert_eq!(parse("COM1:4000000").map(|s| s.baud), Ok(4_000_000));
    assert_eq!(parse("C:"), Ok(port("C:")));
    assert_eq!(
        parse("C:9600"),
        Ok(Settings {
            baud: 9600,
            ..port("C")
        })
    );
    assert_eq!(
        parse("dev:port:9600"),
        Ok(Settings {
            baud: 9600,
            ..port("dev:port")
        })
    );
    assert_eq!(parse("dev:port"), Ok(port("dev:port")));
}

#[test]
fn an_empty_port_is_rejected() {
    for spec in ["", "   ", ":9600", ",8N1"] {
        let error = parse(spec).unwrap_err();
        assert!(error.contains("port name is empty"), "{spec:?}: {error}");
    }
}

#[test]
fn a_baud_out_of_range_is_rejected() {
    for spec in ["COM3:49", "COM3:0", "COM3:4000001", "COM3:99999999999"] {
        let error = parse(spec).unwrap_err();
        assert!(error.contains("Invalid baud rate"), "{spec:?}: {error}");
    }
}

#[test]
fn a_malformed_frame_is_rejected() {
    for spec in [
        "COM3,9N1",
        "COM3,4N1",
        "COM3,8X1",
        "COM3,8N3",
        "COM3,8N",
        "COM3,8N1X",
        "COM3,",
        "COM3,quick",
    ] {
        let error = parse(spec).unwrap_err();
        assert!(error.contains("Invalid frame"), "{spec:?}: {error}");
    }
}

#[test]
fn fast_explicitly_turns_slow_sending_off() {
    assert!(!parse("COM3,fast").unwrap().slow);
    assert!(parse("COM3,8N1,slow").unwrap().slow);
}

#[test]
fn an_unknown_flow_control_is_rejected() {
    for spec in ["COM3,8N1,dtrdsr", "COM3,8N1,"] {
        let error = parse(spec).unwrap_err();
        assert!(error.contains("Unknown flow control"), "{spec:?}: {error}");
    }
}

#[test]
fn extra_parts_are_rejected() {
    for spec in ["COM3,8N1,none,more", "COM3,rtscts,8N1", "COM3,rtscts,none"] {
        let error = parse(spec).unwrap_err();
        assert!(error.contains("extra part"), "{spec:?}: {error}");
    }
}

#[test]
fn format_writes_the_canonical_spec_and_omits_absent_flow_control() {
    assert_eq!(format(&port("COM3")), "COM3:115200,8N1");
    let s = Settings {
        baud: 9600,
        data_bits: 7,
        parity: Parity::Even,
        stop_bits: StopBits::Two,
        flow: Flow::Hardware,
        ..port("/dev/ttyS0")
    };
    assert_eq!(format(&s), "/dev/ttyS0:9600,7E2,rtscts");
    assert_eq!(
        format(&Settings {
            parity: Parity::Odd,
            flow: Flow::Software,
            ..port("COM1")
        }),
        "COM1:115200,8O1,xonxoff"
    );
}

#[test]
fn formatted_specs_parse_back_to_the_same_settings() {
    let samples = [
        port("COM3"),
        port("C:"),
        port("dev:12"),
        Settings {
            baud: 110,
            data_bits: 5,
            parity: Parity::Odd,
            stop_bits: StopBits::Two,
            flow: Flow::Software,
            ..port(r"\\.\COM10")
        },
        Settings {
            baud: 4_000_000,
            data_bits: 6,
            parity: Parity::Even,
            stop_bits: StopBits::One,
            flow: Flow::Hardware,
            ..port("/dev/ttyUSB0")
        },
        Settings {
            baud: 31250,
            data_bits: 7,
            ..port("/dev/tty.usbserial-A1")
        },
    ];
    for settings in samples {
        assert_eq!(
            parse(&format(&settings)),
            Ok(settings.clone()),
            "{}",
            format(&settings)
        );
    }
}

#[test]
fn every_listed_baud_rate_parses() {
    for &baud in BAUDS {
        assert_eq!(parse(&format!("COM1:{baud}")).map(|s| s.baud), Ok(baud));
    }
}

#[test]
fn port_names_sort_with_numbers_in_numeric_order() {
    let mut names = vec![
        "COM10",
        "COM2",
        "/dev/ttyUSB10",
        "COM1",
        "/dev/ttyUSB9",
        "com3",
        "COM02",
    ];
    names.sort_by(|a, b| natural_cmp(a, b));
    assert_eq!(
        names,
        [
            "/dev/ttyUSB9",
            "/dev/ttyUSB10",
            "COM1",
            "COM02",
            "COM2",
            "com3",
            "COM10"
        ]
    );
    assert_eq!(natural_cmp("COM2", "COM2"), Ordering::Equal);
}

#[test]
fn port_types_get_short_descriptions() {
    let usb = |product: Option<&str>, manufacturer: Option<&str>| {
        serialport::SerialPortType::UsbPort(serialport::UsbPortInfo {
            vid: 1,
            pid: 2,
            serial_number: None,
            manufacturer: manufacturer.map(str::to_owned),
            product: product.map(str::to_owned),
        })
    };
    assert_eq!(
        describe(&usb(Some("CP2102 Bridge"), Some("Vendor"))),
        "CP2102 Bridge"
    );
    assert_eq!(describe(&usb(Some(" "), Some("Vendor"))), "Vendor");
    assert_eq!(describe(&usb(None, None)), "USB serial");
    assert_eq!(
        describe(&serialport::SerialPortType::BluetoothPort),
        "Bluetooth"
    );
    assert_eq!(describe(&serialport::SerialPortType::PciPort), "PCI");
    assert_eq!(describe(&serialport::SerialPortType::Unknown), "");
}
