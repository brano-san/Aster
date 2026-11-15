use super::*;
use std::io::Read;

#[test]
fn the_working_directory_of_another_process_is_read_from_its_parameters() {
    let dir = std::env::temp_dir();
    let mut child = std::process::Command::new("cmd.exe")
        .args(["/c", "ping -n 3 127.0.0.1 >nul"])
        .current_dir(&dir)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("spawn cmd");
    std::thread::sleep(std::time::Duration::from_millis(300));
    let seen = current_directory(child.id());
    let _ = child.kill();
    let mut sink = String::new();
    let _ = child.stdout.take().map(|mut o| o.read_to_string(&mut sink));
    let _ = child.wait();
    let expected = dir.to_string_lossy().trim_end_matches('\\').to_lowercase();
    assert_eq!(seen.map(|s| s.to_lowercase()), Some(expected));
}

#[test]
fn a_missing_process_has_no_directory() {
    assert_eq!(current_directory(0), None);
}
