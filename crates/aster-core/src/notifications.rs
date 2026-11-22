use crate::app::{App, Effect};
use crate::i18n::t;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub tab: u64,
    pub title: String,
    pub body: String,
    pub action: String,
}

pub(crate) struct Activity {
    last_output: Instant,
    notified: bool,
}

impl Activity {
    pub(crate) fn new(now: Instant) -> Self {
        Self { last_output: now, notified: false }
    }

    pub(crate) fn visit(&mut self) {
        self.notified = false;
    }

    fn observe(&mut self, now: Instant, background: bool, new_output: bool, silence: Duration) -> bool {
        if !background {
            self.visit();
        }
        if !new_output {
            return false;
        }
        let quiet = now.saturating_duration_since(self.last_output) >= silence;
        self.last_output = now;
        if background && quiet && !self.notified {
            self.notified = true;
            return true;
        }
        false
    }
}

impl App {
    pub(crate) fn reset_activity_tracking(&mut self) {
        let now = Instant::now();
        for tab in &mut self.tabs {
            tab.activity = Activity::new(now);
        }
        for pane in self.panes.values_mut() {
            pane.activity_seq = pane.terminal.lock().output_seq();
        }
    }

    pub(crate) fn poll_background_activity(&mut self, now: Instant) {
        if !self.config.notifications.activity {
            return;
        }
        let silence = Duration::from_secs(u64::from(self.config.notifications.activity_silence_seconds.clamp(1, 3600)));
        for index in 0..self.tabs.len() {
            let tab = &mut self.tabs[index];
            let mut changed = false;
            for id in tab.root.panes() {
                if let Some(pane) = self.panes.get_mut(&id) {
                    let seq = pane.terminal.lock().output_seq();
                    changed |= seq != pane.activity_seq;
                    pane.activity_seq = seq;
                }
            }
            if tab.activity.observe(now, index != self.active, changed, silence) {
                self.notify_tab(index, t("Background tab activity"));
            }
        }
    }

    pub(crate) fn notify_command_finished(&mut self, index: usize) {
        self.push(Effect::Attention);
        if self.config.notifications.command_finished {
            self.notify_tab(index, t("Command finished"));
        }
    }

    pub(crate) fn notify_session_exit(&mut self, tab_key: u64) {
        if !self.config.notifications.command_finished {
            return;
        }
        if let Some(index) = self.tabs.iter().position(|tab| {
            tab.key == tab_key && tab.command_watch == crate::close::CommandWatch::Off
        }) {
            self.notify_tab(index, t("Process finished"));
        }
    }

    fn notify_tab(&mut self, index: usize, title: &str) {
        let Some(tab) = self.tabs.get(index) else { return };
        self.push(Effect::Notify(Notification {
            tab: tab.key,
            title: title.to_owned(),
            body: self.tab_title(index),
            action: t("Open").to_owned(),
        }));
    }

    pub(crate) fn notification_clicked(&mut self, key: u64) {
        if let Some(index) = self.tabs.iter().position(|tab| tab.key == key) {
            self.activate(index);
        }
    }
}

#[cfg(test)]
#[path = "../test/notifications.rs"]
mod tests;