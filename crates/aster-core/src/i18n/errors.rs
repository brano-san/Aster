use super::{t, tf};
use std::io::ErrorKind;

pub fn ssh_error(message: &str) -> String {
    if let Some(target) = message.strip_suffix(": permission denied") {
        return tf("{target}: permission denied", &[("target", target)]);
    }
    if let Some((target, detail)) = message.strip_prefix("cannot connect to ").and_then(|s| s.split_once(": ")) {
        return tf("Cannot connect to {target}: {error}", &[("target", target), ("error", detail)]);
    }
    if let Some(handshake) = message.strip_prefix("SSH handshake with ") {
        if let Some((host, detail)) = handshake.split_once(" failed: ") {
            return tf("SSH handshake with {host} failed: {error}", &[("host", host), ("error", detail)]);
        }
        if let Some((host, ms)) = handshake.strip_suffix(" ms").and_then(|s| s.split_once(" timed out after ")) {
            return tf("SSH handshake with {host} timed out after {ms} ms", &[("host", host), ("ms", ms)]);
        }
    }
    if let Some((target, detail)) = message.split_once(" is not reachable through the jump host: ") {
        return tf("{target} is not reachable through the jump host: {error}", &[("target", target), ("error", detail)]);
    }
    if let Some((host, error)) = message.strip_prefix("Cannot check the host key of ").and_then(|s| s.split_once(": ")) {
        return tf("Cannot check the host key of {host}: {error}", &[("host", host), ("error", error)]);
    }
    if let Some(host) = message.strip_prefix("WARNING: the host key of ").and_then(|s| s.strip_suffix(" has changed.")) {
        return tf("WARNING: the host key of {host} has changed.", &[("host", host)]);
    }
    if let Some(host) = message.strip_prefix("The authenticity of host '").and_then(|s| s.strip_suffix("' can't be established.")) {
        return tf("The authenticity of host '{host}' can't be established.", &[("host", host)]);
    }
    if let Some(host) = message.strip_suffix(" presented a host certificate; only plain host keys are supported.") {
        return tf("{host} presented a host certificate; only plain host keys are supported.", &[("host", host)]);
    }
    let fingerprint = message.strip_prefix("The ").unwrap_or(message);
    if let Some((algorithm, digest)) = fingerprint.strip_suffix('.').and_then(|s| s.split_once(" key fingerprint is ")) {
        return tf("The {algorithm} key fingerprint is {fingerprint}.", &[("algorithm", algorithm), ("fingerprint", digest)]);
    }
    if let Some((line, path)) = message.strip_prefix("The stored key is on line ").and_then(|s| s.strip_suffix('.')).and_then(|s| s.split_once(" of ")) {
        return tf("The stored key is on line {line} of {path}.", &[("line", line), ("path", path)]);
    }
    for (prefix, template) in [
        ("cannot open a file channel: ", "Cannot open a file channel: {error}"),
        ("the server has no SFTP: ", "The server has no SFTP: {error}"),
        ("SFTP failed to start: ", "SFTP failed to start: {error}"),
        ("Could not save the host key: ", "Could not save the host key: {error}"),
    ] {
        if let Some(detail) = message.strip_prefix(prefix) {
            return tf(template, &[("error", detail)]);
        }
    }
    match message {
        known @ ("cannot start the SSH client" | "the connection is closed" | "connection pool is broken" | "too many jump hosts" |
            "Session closed during X11 setup" | "Bad passphrase, try again." |
            "Someone could be intercepting the connection, or the host key was replaced." |
            "Remove that line to connect anyway." | "The server refused to start the remote shell." |
            "The server refused to start the remote shell in the remembered folder.") => t(known).to_owned(),
        _ => message.to_owned(),
    }
}

pub fn platform_error(kind: ErrorKind, message: &str) -> String {
    let translated = platform_error_key(message);
    if translated != message { return translated.to_owned(); }
    match kind {
        ErrorKind::NotFound => tf("File or program not found: {error}", &[("error", message)]),
        ErrorKind::PermissionDenied => tf("Access denied: {error}", &[("error", message)]),
        ErrorKind::TimedOut => tf("The operation timed out: {error}", &[("error", message)]),
        ErrorKind::ConnectionRefused => tf("Connection refused: {error}", &[("error", message)]),
        ErrorKind::InvalidInput => tf("Invalid argument: {error}", &[("error", message)]),
        _ => message.to_owned(),
    }
}

fn platform_error_key(message: &str) -> &str {
    match message {
        "The administrator request was declined" => t("The administrator request was declined"),
        "the terminal host could not connect" => t("the terminal host could not connect"),
        "the terminal host did not start in time" => t("the terminal host did not start in time"),
        "Administrator tabs are only available on Windows" => t("Administrator tabs are only available on Windows"),
        "The terminal host is only available on Windows" => t("The terminal host is only available on Windows"),
        "Installing with administrator rights is only available on Windows" => t("Installing with administrator rights is only available on Windows"),
        _ => message,
    }
}
