use crate::app::{App, TOAST};
use crate::config::Snippet;
use crate::i18n::{t, tf};
use crate::keys::paste_bytes;
use crate::overlay::Pick;
use aster_vt::TermMode;

const SNIPPET: &str = "snippet:";

impl App {
    pub(crate) fn snippet_picks(&self) -> impl Iterator<Item = Pick> + '_ {
        self.config.snippets.iter().filter(|(name, snippet)| {
            !name.trim().is_empty() && !name.chars().any(char::is_control) && !snippet.text.trim().is_empty()
        }).map(|(name, snippet)| Pick {
            label: name.clone(),
            source_label: format!("{name} {} {}", snippet.tags.join(" "), snippet.text),
            hint: if snippet.tags.is_empty() { t("Paste snippet").into() } else { snippet.tags.join(", ") },
            value: format!("{SNIPPET}{name}"),
            ..Pick::default()
        })
    }

    pub(crate) fn choose_snippet(&mut self, value: &str) -> bool {
        let Some(name) = value.strip_prefix(SNIPPET) else { return false };
        self.paste_snippet(name);
        true
    }

    fn paste_snippet(&mut self, name: &str) {
        let Some(snippet) = self.config.snippets.get(name) else { return };
        let Some(pane) = self.focused_pane().filter(|pane| {
            self.panes.get(pane).is_some_and(|p| p.exited.is_none()) && self.offered_pane().is_none() && !self.ssh_auth_active()
        }) else {
            self.toast(t("Focus a terminal pane to paste a snippet").to_owned(), TOAST);
            return;
        };
        let mode = self.paste_mode();
        let mut text: String = snippet.text.chars().filter(|c| !c.is_control() || matches!(c, '\r' | '\n' | '\t')).collect();
        text.truncate(text.trim_end_matches(['\r', '\n']).len());
        if self.config.terminal.paste_newlines_as_spaces || !mode.contains(TermMode::BRACKETED_PASTE) {
            text = text.replace("\r\n", " ").replace(['\r', '\n', '\t'], " ");
        }
        if !text.trim().is_empty() {
            self.write(pane, paste_bytes(&text, mode, self.config.terminal.trim_whitespace_on_paste));
        }
    }

    pub(crate) fn manage_snippets(&mut self) {
        self.open_settings();
        if let Some(page) = crate::settings::PAGES.iter().position(|page| *page == crate::settings::CONFIG_FILE) {
            self.show_settings_page(page);
        }
        self.toast(t("Edit the [snippets] entries in the config file").to_owned(), TOAST);
    }

    pub(crate) fn save_selection_as_snippet(&mut self) {
        if self.config.read_only() { return self.warn_read_only_config(); }
        let Some(text) = self.focused_terminal().and_then(|terminal| terminal.lock().selection_text()) else { return };
        let text = text.trim_matches(['\r', '\n']);
        let Some(first) = text.lines().map(str::trim).find(|line| !line.is_empty()) else { return };
        let base: String = first.split_whitespace().collect::<Vec<_>>().join(" ").chars().filter(|c| !c.is_control()).take(60).collect();
        if base.is_empty() { return; }
        let mut config = self.config.clone();
        let name = if config.snippets.contains_key(&base) {
            (2..).map(|number| format!("{base} ({number})")).find(|name| !config.snippets.contains_key(name)).unwrap()
        } else {
            base
        };
        config.snippets.insert(name.clone(), Snippet { text: text.to_owned(), tags: Vec::new() });
        self.set_config(config);
        self.push_save_config();
        self.toast(tf("Saved snippet {name}", &[("name", &name)]), TOAST);
    }
}
