use aster_proto::input::{InputMode, InputProcessor};
use crossbeam_channel::Sender;
use tokio::sync::mpsc::UnboundedReceiver;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input {
    Data(Vec<u8>),
    Resize(u16, u16),
    Close,
}

pub struct Console {
    out: Sender<Vec<u8>>,
    input: UnboundedReceiver<Input>,
    pub size: (u16, u16),
    pub closed: bool,
}

pub fn crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

impl Console {
    pub fn new(out: Sender<Vec<u8>>, input: UnboundedReceiver<Input>, size: (u16, u16)) -> Self {
        Self { out, input, size, closed: false }
    }

    pub fn print(&self, text: &str) {
        let _ = self.out.send(crlf(text).into_bytes());
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
                Input::Resize(..) => {}
                Input::Close => return None,
            }
        }
    }
}

#[cfg(test)]
#[path = "../test/console.rs"]
mod tests;
