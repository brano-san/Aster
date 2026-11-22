use aster_proto::input::{InputMode, InputProcessor};
pub use aster_proto::ssh::{InteractiveAnswer, InteractiveChallenge, SshEvent, InteractivePrompt};
use crossbeam_channel::Sender;
use tokio::sync::mpsc::UnboundedReceiver;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Data(Vec<u8>),
    Resize(u16, u16),
    Close,
    Sftp(crate::sftp::Request),
    Interactive(InteractiveAnswer),
}

pub struct Console {
    out: Sender<Vec<u8>>,
    input: UnboundedReceiver<Input>,
    pub size: (u16, u16),
    pub closed: bool,
    pub(crate) interactive: Option<Sender<SshEvent>>,
}

pub fn crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

impl Console {
    pub fn new(out: Sender<Vec<u8>>, input: UnboundedReceiver<Input>, size: (u16, u16)) -> Self {
        Self { out, input, size, closed: false, interactive: None }
    }

    pub fn print(&self, text: &str) {
        let _ = self.out.send(crlf(text).into_bytes());
    }

    fn service(&self, color: u8, text: &str) {
        let mut output = String::new();
        for line in text.lines() {
            use std::fmt::Write;
            let _ = write!(output, "\x1b[{color}m");
            output.extend(line.chars().filter(|c| !c.is_control()));
            output.push_str("\x1b[0m\r\n");
        }
        self.write(output.into_bytes());
    }

    pub fn note(&self, text: &str) {
        self.service(2, text);
    }

    pub fn warn(&self, text: &str) {
        self.service(33, text);
    }

    pub fn error(&self, text: &str) {
        self.service(31, text);
    }

    pub fn write(&self, bytes: Vec<u8>) {
        let _ = self.out.send(bytes);
    }

    pub async fn next(&mut self) -> Option<Input> {
        let input = self.input.recv().await;
        match &input {
            Some(Input::Resize(c, r)) => self.size = (*c, *r),
            Some(Input::Close) | None => self.closed = true,
            _ => {}
        }
        input
    }

    pub async fn ask(&mut self, prompt: &str, echo: bool) -> Option<String> {
        self.print(prompt);
        let mut editor = InputProcessor::new(InputMode::Readline);
        loop {
            match self.next().await? {
                Input::Data(bytes) => {
                    let p = editor.feed(&bytes);
                    if p.send == [3] {
                        self.write(p.echo);
                        return None;
                    }
                    if echo {
                        self.write(p.echo);
                    }
                    if let Some(line) = p.send.strip_suffix(b"\r") {
                        if !echo {
                            self.write(b"\r\n".to_vec());
                        }
                        return Some(String::from_utf8_lossy(line).into_owned());
                    }
                }
                Input::Resize(..) | Input::Sftp(_) | Input::Interactive(_) => {}
                Input::Close => return None,
            }
        }
    }

    pub async fn ask_interactive(&mut self, challenge: InteractiveChallenge) -> Option<InteractiveAnswer> {
        let id = challenge.id;
        if let Some(tx) = &self.interactive {
            let count = challenge.prompts.len();
            if count == 0 && challenge.name.trim().is_empty() && challenge.instructions.trim().is_empty() {
                return Some(InteractiveAnswer { id, answers: Some(Vec::new()), remember: false });
            }
            tx.send(SshEvent::Challenge(challenge)).ok()?;
            loop {
                match self.next().await? {
                    Input::Interactive(answer) if answer.id == id && answer.answers.as_ref().is_none_or(|items| items.len() == count) => return Some(answer),
                    Input::Data(bytes) if bytes.contains(&3) => return None,
                    Input::Close => return None,
                    _ => {}
                }
            }
        }
        for text in [challenge.name, challenge.instructions] {
            if !text.trim().is_empty() { self.note(text.trim_end()); }
        }
        let mut answers = Vec::with_capacity(challenge.prompts.len());
        for prompt in challenge.prompts {
            answers.push(self.ask(&prompt.label, prompt.echo).await?);
        }
        Some(InteractiveAnswer { id, answers: Some(answers), remember: false })
    }

    pub(crate) fn finish_interactive(&self) {
        if let Some(tx) = &self.interactive { let _ = tx.send(SshEvent::Finished); }
    }

    pub(crate) fn forward_status(&self, statuses: Vec<aster_proto::ssh_forward::ForwardStatus>) {
        if let Some(tx) = &self.interactive { let _ = tx.send(SshEvent::Forwards(statuses)); }
    }

    pub(crate) async fn wait_for_close(&mut self) {
        while !self.closed {
            if matches!(self.next().await, Some(Input::Data(bytes)) if bytes.contains(&3)) {
                self.closed = true;
            }
        }
    }
}

#[cfg(test)]
#[path = "../test/console.rs"]
mod tests;
