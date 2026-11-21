use super::*;

#[test]
fn plain_http_is_refused_before_curl_starts() {
    let mut out = Vec::new();
    let e = download("http://example.com/a.tar.gz", &mut out, &mut |_, _| true).unwrap_err();
    assert!(e.message.contains("https"), "{}", e.message);
    assert!(out.is_empty());
}

#[test]
fn a_finished_process_is_not_waited_for() {
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    let start = Instant::now();
    wait_exit(pid, Duration::from_secs(5));
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(alive(std::process::id()));
}
