use aster_core::notifications::Notification;
use std::io;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub(crate) fn show(notification: &Notification) -> io::Result<bool> {
    let mut body = String::new();
    for c in notification.body.chars().take(512).filter(|c| !c.is_control()) {
        match c {
            '&' => body.push_str("&amp;"),
            '<' => body.push_str("&lt;"),
            '>' => body.push_str("&gt;"),
            _ => body.push(c),
        }
    }
    let mut child = Command::new("notify-send")
        .args(["--app-name=Aster", "--wait", "--expire-time=12000"])
        .arg(format!("--action=default={}", notification.action))
        .arg("--")
        .args([&notification.title, &body])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(60);
    while child.try_wait()?.is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(io::Error::other(String::from_utf8_lossy(&output.stderr).trim().to_owned()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).lines().any(|line| line.trim() == "default"))
}
