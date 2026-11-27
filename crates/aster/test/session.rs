use super::*;

#[test]
fn wakes_at_frame_intervals_and_only_yields_for_a_stale_frame() {
    let start = Instant::now();
    let ms = Duration::from_millis;
    let mut pacer = Pacer::default();
    assert_eq!(pacer.delay(start), Duration::ZERO);
    pacer.woke(start);
    assert_eq!(pacer.delay(start + ms(3)), ms(5));
    assert_eq!(pacer.delay(start + ms(8)), Duration::ZERO);
    assert!(!Pacer::should_yield(start + ms(99), start));
    assert!(Pacer::should_yield(start + ms(100), start));
    assert!(!Pacer::should_yield(start + ms(101), start + ms(100)));
}

#[test]
fn a_pending_wake_arrives_after_the_reader_goes_idle() {
    let (tx, rx) = unbounded();
    let flow = OutputFlow::new(move || tx.send(()).unwrap());
    (flow.wake)();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    (flow.wake)();
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
}

#[test]
fn raw_output_wraps_in_byte_order_and_keeps_only_the_last_mib() {
    let raw = RawOutput::default();
    raw.append(&vec![0x80; 1024 * 1024 + 7]);
    raw.append(&[0, 0xff, 0x1b]);
    let snapshot = raw.snapshot();
    assert_eq!(snapshot.len(), 1024 * 1024);
    assert!(snapshot[..snapshot.len() - 3].iter().all(|&b| b == 0x80));
    assert_eq!(&snapshot[snapshot.len() - 3..], &[0, 0xff, 0x1b]);
}

#[test]
fn saving_writes_the_requested_snapshot_and_reports_the_path() {
    let dir = std::env::temp_dir().join(format!("aster-0901-save-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    let downloads = dir.clone();
    let raw = RawOutput::default();
    raw.append(b"\x1b[31m\x00\xff");
    let vt = aster_vt::Terminal::new(aster_vt::Size::new(512, 4), Default::default(), Arc::new(|_| {}));
    let terminal = Arc::new(aster_vt::FairMutex::new(vt));
    let (tx, rx) = bounded(1);
    raw.save(42, move || Some(downloads), terminal.clone(), Arc::new(move || tx.send(()).unwrap()));
    raw.append(b"later");
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let path = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
    assert!(path.file_name().unwrap().to_str().unwrap().starts_with("aster-output-42-"));
    assert_eq!(path.extension().unwrap(), "bin");
    assert_eq!(std::fs::read(&path).unwrap(), b"\x1b[31m\x00\xff");
    assert!(terminal.lock().screen_text().contains(&format!("[saved to {}]", path.display())));
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn buffer_export_writes_utf8_and_leaves_previous_exports_intact() {
    let dir = std::env::temp_dir().join(format!("aster-0903-export-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    let first = OutputFile::Buffer("one\n界".into()).write(&dir, 7).unwrap();
    let second = OutputFile::Buffer("two".into()).write(&dir, 7).unwrap();
    assert!(first.file_stem().unwrap().to_str().unwrap().strip_prefix("aster-buffer-7-").unwrap().parse::<u128>().is_ok());
    assert_eq!(first.extension().unwrap(), "txt");
    assert_eq!(std::fs::read_to_string(&first).unwrap(), "one\n界");
    assert_eq!(std::fs::read_to_string(&second).unwrap(), "two");
    std::fs::remove_file(first).unwrap();
    std::fs::remove_file(second).unwrap();
    std::fs::remove_dir(dir).unwrap();
}

#[test]
fn fixture_export_writes_a_self_contained_replay_file() {
    let dir = std::env::temp_dir().join(format!("aster-fixture-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
    let downloads = dir.clone();
    let mut vt = aster_vt::Terminal::new(aster_vt::Size::new(80, 5), Default::default(), Arc::new(|_| {}));
    assert!(vt.enable_fixture_recording());
    vt.feed_saved_output(b"restored\r\n");
    vt.resize(aster_vt::Size::new(82, 5));
    vt.feed(b"live");
    let terminal = Arc::new(aster_vt::FairMutex::new(vt));
    let (tx, rx) = bounded(1);
    save_fixture(42, move || Some(downloads), terminal.clone(), Arc::new(move || tx.send(()).unwrap()));
    rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let path = std::fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
    assert!(path.file_name().unwrap().to_str().unwrap().starts_with("aster-fixture-42-"));
    verify_fixture_file(&path).unwrap();
    assert!(terminal.lock().screen_text().contains("[saved to "));
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
