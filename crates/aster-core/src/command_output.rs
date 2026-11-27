use crate::app::{App, Effect, TOAST};
use crate::i18n::{t, tf};
use crate::notices::Toast;
use crate::overlay::{OverlayKind, Pick};
use crate::panes::PaneId;

impl App {
    pub(crate) fn command_history_picks(&self, pane: PaneId) -> Vec<Pick> {
        let Some(terminal) = self.terminal(pane) else { return Vec::new() };
        let history = terminal.lock().command_history();
        history.into_iter().map(|entry| {
            let number = entry.id.to_string();
            let label = if entry.command.trim().is_empty() {
                tf("Command {number}", &[("number", &number)])
            } else {
                entry.command.split_whitespace().collect::<Vec<_>>().join(" ")
            };
            let preview = if entry.preview.is_empty() { t("No output").to_owned() } else { entry.preview };
            let hint = if let Some(code) = entry.status { format!("{} {code} · {preview}", t("Exit code")) } else { preview };
            Pick { label, hint, value: number, ..Pick::default() }
        }).collect()
    }

    pub(crate) fn search_command_output(&mut self, pane: PaneId, value: &str) {
        let Some(command) = value.parse::<u64>().ok() else { return };
        let available = self.focused_pane() == Some(pane) && self.terminal(pane).is_some_and(|terminal| terminal.lock().reveal_command(command));
        if available {
            self.open_output_search(OverlayKind::CommandOutput(pane, command));
        } else {
            self.command_output_notice(t("This command is no longer in the buffer"));
        }
    }

    pub(crate) fn copy_command_from_history(&mut self, pane: PaneId, value: &str) {
        let Some(command) = value.parse::<u64>().ok() else { return };
        if self.focused_pane() != Some(pane) { return }
        let output = self.terminal(pane).and_then(|terminal| terminal.lock().command_output(command));
        match output {
            Some(output) if !output.is_empty() => {
                self.overlay = None;
                self.push(Effect::SetClipboard(output));
                self.dirty = true;
            }
            Some(_) => self.command_output_notice(t("No output")),
            None => self.command_output_notice(t("This command is no longer in the buffer")),
        }
    }

    fn command_output_notice(&mut self, message: &str) {
        self.tab_toast = Some(Toast::new(message.to_owned(), self.clock(), TOAST, false));
        self.dirty = true;
    }
}
