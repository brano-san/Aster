use crate::app::{App, Effect};
use crate::panes::PaneId;

const FAILURES: [&str; 6] = ["sorry", "incorrect", "failure", "denied", "wrong", "invalid"];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Prompt {
    pub key: String,
    pub line: usize,
    pub saved: Option<bool>,
    pub typed: String,
    pub submitted: bool,
    pub filled: bool,
    pub offer: bool,
    escape: bool,
}

impl App {
    pub fn password_prompt(&self, pane: PaneId) -> Option<&Prompt> {
        self.prompts.get(&pane)
    }

    fn secret_key(&self, pane: PaneId) -> Option<String> {
        let p = self.panes.get(&pane)?;
        Some(match &p.profile.ssh {
            Some(dest) => format!("sudo:{dest}"),
            None => format!("sudo:{}", p.profile.id),
        })
    }

    pub fn secret_known(&mut self, pane: PaneId, known: bool) {
        if let Some(p) = self.prompts.get_mut(&pane) {
            p.saved = Some(known);
            self.dirty = true;
        }
    }

    pub(crate) fn scan_prompts(&mut self) {
        if !self.config.terminal.sudo_passwords {
            self.prompts.clear();
            return;
        }
        let ids: Vec<PaneId> = self.panes.iter().filter(|(_, p)| p.exited.is_none() && p.pending.is_none()).map(|(id, _)| *id).collect();
        for pane in ids {
            let Some(p) = self.panes.get(&pane) else { continue };
            let (line, at, tail) = {
                let t = p.terminal.lock();
                let (line, at) = t.prompt_line();
                (line, at, t.tail_lines(3).join("\n").to_lowercase())
            };
            let asking = aster_proto::prompt::is_password_prompt(&line);
            match self.prompts.get_mut(&pane) {
                None if asking => {
                    let Some(key) = self.secret_key(pane) else { continue };
                    self.prompts.insert(pane, Prompt { key: key.clone(), line: at, ..Prompt::default() });
                    self.effects.push(Effect::CheckSecret { pane, key });
                    self.dirty = true;
                }
                None => {}
                Some(prompt) if prompt.offer => {
                    if asking && at != prompt.line {
                        let key = prompt.key.clone();
                        *prompt = Prompt { key: key.clone(), line: at, ..Prompt::default() };
                        self.effects.push(Effect::CheckSecret { pane, key });
                        self.dirty = true;
                    }
                }
                Some(prompt) if asking => {
                    if prompt.submitted && at != prompt.line {
                        prompt.typed.clear();
                        prompt.submitted = false;
                        prompt.filled = false;
                        prompt.line = at;
                        self.dirty = true;
                    }
                }
                Some(prompt) => {
                    let failed = FAILURES.iter().any(|f| tail.contains(f));
                    let worked = prompt.submitted && !prompt.filled && !prompt.typed.is_empty() && prompt.saved == Some(false) && !failed;
                    if worked && self.config.ssh.remember_passwords {
                        prompt.offer = true;
                        prompt.line = at;
                    } else {
                        self.prompts.remove(&pane);
                    }
                    self.dirty = true;
                }
            }
        }
    }

    pub(crate) fn note_input(&mut self, pane: PaneId, bytes: &[u8]) {
        let Some(prompt) = self.prompts.get_mut(&pane) else { return };
        if prompt.offer || prompt.submitted {
            return;
        }
        let text = String::from_utf8_lossy(bytes).replace("\x1b[200~", "").replace("\x1b[201~", "");
        for c in text.chars() {
            if prompt.escape {
                prompt.escape = !c.is_ascii_alphabetic() && c != '~';
                continue;
            }
            match c {
                '\r' | '\n' => {
                    prompt.submitted = true;
                    break;
                }
                '\x7f' | '\x08' => {
                    prompt.typed.pop();
                }
                '\x15' => prompt.typed.clear(),
                '\x1b' => prompt.escape = true,
                c if !c.is_control() => prompt.typed.push(c),
                _ => {}
            }
        }
    }

    pub(crate) fn fill_password(&mut self) -> bool {
        let Some(pane) = self.focused_pane() else { return false };
        let Some(prompt) = self.prompts.get_mut(&pane) else { return false };
        self.dirty = true;
        if prompt.offer {
            let (key, value) = (prompt.key.clone(), std::mem::take(&mut prompt.typed));
            self.prompts.remove(&pane);
            self.effects.push(Effect::SaveSecret { key, value });
            return true;
        }
        if prompt.saved == Some(true) && !prompt.submitted {
            prompt.submitted = true;
            prompt.filled = true;
            let key = prompt.key.clone();
            self.effects.push(Effect::FillSecret { pane, key });
            return true;
        }
        false
    }

    pub(crate) fn dismiss_offer(&mut self) {
        if let Some(pane) = self.focused_pane() {
            if self.prompts.get(&pane).is_some_and(|p| p.offer) {
                self.prompts.remove(&pane);
                self.dirty = true;
            }
        }
    }
}

#[cfg(test)]
#[path = "../test/sudo.rs"]
mod tests;
