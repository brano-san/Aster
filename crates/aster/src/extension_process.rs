use aster_core::extensions::{Action, Installed, Profile};
use aster_ext::{Message, Notification, Request, Response, RpcError, decode_message, encode_notification, encode_request, encode_response};
use aster_platform::Platform;
use crossbeam_channel::{Receiver, Sender, bounded, select_biased};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(1);
const POLL: Duration = Duration::from_millis(25);
const MAX_LINE: u64 = 1024 * 1024;

pub enum Update {
    Ready(Vec<Profile>),
    Failed(String),
    Action(Action),
}

pub struct Process {
    input: Option<Sender<String>>,
    pub thread: JoinHandle<()>,
}

impl Process {
    pub fn start<P: Platform>(entry: &Installed, generation: u64, language: String, events: Sender<(String, u64, Update)>) -> std::io::Result<Self> {
        let (input, queue) = bounded(64);
        let entry = entry.clone();
        let id = entry.manifest.id.clone();
        let thread = thread::Builder::new().name(format!("aster-ext-{id}")).spawn(move || {
            let post = |update| { let _ = events.send((id.clone(), generation, update)); };
            if let Err(error) = run::<P>(&entry, &language, queue, &post) {
                post(Update::Failed(error));
            }
        })?;
        Ok(Self { input: Some(input), thread })
    }

    pub fn send(&self, command: &str, title: &str, cwd: Option<&str>) {
        if let Some(input) = &self.input {
            let _ = input.try_send(notification("command/run", Some(json!({"id": command, "pane": {"title": title, "cwd": cwd}}))));
        }
    }

    pub fn stop(&mut self) {
        self.input.take();
    }
}

fn notification(method: &str, params: Option<Value>) -> String {
    encode_notification(&Notification { method: method.into(), params })
}

fn request(id: u64, method: &str, params: Option<Value>) -> String {
    encode_request(&Request { id: json!(id), method: method.into(), params })
}

fn run<P: Platform>(entry: &Installed, language: &str, input: Receiver<String>, post: &impl Fn(Update)) -> Result<(), String> {
    let file = super::extensions::entry_path(&entry.directory, entry.manifest.command.as_deref().unwrap_or_default())?;
    let mut command = Command::new(file);
    command.args(&entry.manifest.args).current_dir(&entry.directory).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = P::spawn_extension(&mut command).map_err(|error| error.to_string())?;
    let last_stderr = Arc::new(Mutex::new(String::new()));
    let outcome = communicate(&mut child, entry.manifest.profiles.provider, language, input, post, &last_stderr);
    if let Err(error) = outcome {
        let _ = child.kill();
        let _ = child.wait();
        let stderr = last_stderr.lock().unwrap_or_else(|e| e.into_inner());
        return Err(if stderr.is_empty() { error } else { format!("{error}: {stderr}") });
    }
    Ok(())
}

fn communicate(child: &mut Child, profiles: bool, language: &str, input: Receiver<String>, post: &impl Fn(Update), stderr: &Arc<Mutex<String>>) -> Result<(), String> {
    let (incoming, messages) = bounded(64);
    let (out, outgoing) = bounded(64);
    let stdout = child.stdout.take().ok_or("Extension stdout is unavailable")?;
    thread::Builder::new().spawn(move || {
        let mut reader = BufReader::new(stdout);
        while let Ok(Some(line)) = read_line(&mut reader) {
            if String::from_utf8(line).ok().is_none_or(|line| incoming.send(line).is_err()) {
                break;
            }
        }
    }).map_err(error_text)?;
    let last = stderr.clone();
    let stderr = child.stderr.take().ok_or("Extension stderr is unavailable")?;
    let (stderr_closed, stderr_done) = bounded::<()>(0);
    thread::Builder::new().spawn(move || {
        let _closed = stderr_closed;
        let mut reader = BufReader::new(stderr);
        while let Ok(Some(line)) = read_line(&mut reader) {
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end();
            if !line.is_empty() {
                *last.lock().unwrap_or_else(|e| e.into_inner()) = line.to_owned();
            }
        }
    }).map_err(error_text)?;
    let mut stdin = child.stdin.take().ok_or("Extension stdin is unavailable")?;
    thread::Builder::new().spawn(move || {
        for line in outgoing.iter() {
            let line: String = line;
            if stdin.write_all(line.as_bytes()).is_err() {
                break;
            }
        }
    }).map_err(error_text)?;

    let result = protocol(child, profiles, language, input, messages, out, post);
    if result.is_err() {
        let _ = stderr_done.recv_timeout(Duration::from_millis(50));
    }
    result
}

fn protocol(child: &mut Child, profiles: bool, language: &str, input: Receiver<String>, messages: Receiver<String>, out: Sender<String>, post: &impl Fn(Update)) -> Result<(), String> {
    let mut awaiting = Some(1);
    let mut deadline = Instant::now() + REQUEST_TIMEOUT;
    let mut initialized = false;
    out.try_send(request(1, "initialize", Some(json!({"version": env!("CARGO_PKG_VERSION"), "language": language})))).map_err(error_text)?;
    loop {
        if let Some(status) = child.try_wait().map_err(error_text)? {
            return Err(format!("Extension exited ({status})"));
        }
        if awaiting.is_some() && Instant::now() >= deadline {
            return Err("Extension request timed out".into());
        }
        select_biased! {
            recv(input) -> message => match message {
                Ok(line) => out.try_send(line).map_err(error_text)?,
                Err(_) => {
                    let _ = out.try_send(notification("shutdown", None));
                    drop(out);
                    return finish(child);
                }
            },
            recv(messages) -> line => {
                match decode_message(&line.map_err(|_| "Extension closed stdout")?)? {
                    Message::Response(response) if response.id.as_u64().is_some() && response.id.as_u64() == awaiting => {
                        let result = response.result.map_err(|error| error.message)?;
                        if awaiting == Some(1) {
                            if !result.as_object().is_some_and(|result| result.is_empty()) {
                                return Err("Invalid initialize result".into());
                            }
                            initialized = true;
                            if profiles {
                                out.try_send(request(2, "profiles/list", None)).map_err(error_text)?;
                                awaiting = Some(2);
                                deadline = Instant::now() + REQUEST_TIMEOUT;
                            } else {
                                awaiting = None;
                                post(Update::Ready(Vec::new()));
                            }
                        } else {
                            awaiting = None;
                            let profiles = serde_json::from_value(result).map_err(error_text)?;
                            post(Update::Ready(profiles));
                        }
                    }
                    Message::Notification(note) if initialized => {
                        if let Ok(action) = action(&note.method, note.params.as_ref()) {
                            post(Update::Action(action));
                        }
                    }
                    Message::Request(call) => {
                        let result = if initialized {
                            action(&call.method, call.params.as_ref()).map(|action| post(Update::Action(action)))
                        } else {
                            Err(RpcError { code: -32000, message: "Extension is not initialized".into(), data: None })
                        };
                        out.try_send(encode_response(&Response { id: call.id, result: result.map(|()| json!({})) })).map_err(error_text)?;
                    }
                    _ => {}
                }
            },
            default(POLL) => {}
        }
    }
}

fn finish(child: &mut Child) -> Result<(), String> {
    let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
    while Instant::now() < deadline {
        if child.try_wait().map_err(error_text)?.is_some() {
            return Ok(());
        }
        thread::sleep(POLL);
    }
    if child.try_wait().map_err(error_text)?.is_none() {
        let _ = child.kill();
        child.wait().map_err(error_text)?;
    }
    Ok(())
}

fn read_line(reader: &mut impl BufRead) -> std::io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    reader.take(MAX_LINE + 1).read_until(b'\n', &mut bytes)?;
    if bytes.len() as u64 > MAX_LINE {
        return Err(std::io::Error::other("Extension message is too large"));
    }
    if bytes.is_empty() {
        return Ok(None);
    }
    Ok(Some(bytes))
}

fn action(method: &str, params: Option<&Value>) -> Result<Action, RpcError> {
    let make = match method {
        "pane/write" => Action::Write,
        "notify" => Action::Notify,
        "clipboard/set" => Action::Clipboard,
        _ => return Err(RpcError { code: -32601, message: "Method not found".into(), data: None }),
    };
    let text = params.and_then(|params| params.get("text")).and_then(Value::as_str)
        .ok_or_else(|| RpcError { code: -32602, message: "Expected text".into(), data: None })?;
    Ok(make(text.to_owned()))
}

fn error_text(error: impl std::fmt::Display) -> String {
    error.to_string()
}
