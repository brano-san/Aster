use crate::app::App;
use crate::i18n::t;
use crate::keymap::{Action, format_binding};
use crate::menu::Entry;
use crate::overlay::Pick;

fn menu_path(parents: &[&str], label: &str, translate: fn(&str) -> &str) -> String {
    parents.iter().copied().chain([label]).map(translate).collect::<Vec<_>>().join(" > ")
}

impl App {
    fn command_pick(&self, action: &Action, label: String, source_label: String) -> Pick {
        let hint = self.keymap.bindings_of(action).next().map(format_binding).unwrap_or_default();
        Pick { label, source_label, hint, value: action.id(), ..Pick::default() }
    }

    pub(crate) fn command_picks(&self) -> Vec<Pick> {
        let mut picks: Vec<_> = Action::all()
            .filter(|a| !matches!(a, Action::SerialSpeed | Action::SerialBreak) || self.serial_pane().is_some())
            .filter(|a| *a != Action::SaveLayout || self.can_save_layout())
            .filter(|a| *a != Action::SaveAsProfile || self.focused_pane().is_some_and(|id| self.panes.contains_key(&id)))
            .map(|action| self.command_pick(&action, action.label(), action.source_label()))
            .collect();

        picks.extend(self.dynamic_actions().into_iter().map(|entry| {
            let name = entry.name.as_deref();
            self.command_pick(&entry.action, entry.action.label_for(name), entry.action.source_label_for(name))
        }));
        for (parent, entries) in [("Tab", self.tab_menu_entries(self.active)), ("Terminal", self.terminal_menu_entries())] {
            picks.extend(entries.into_iter().filter_map(|entry| {
                let Entry::Item { label: std::borrow::Cow::Borrowed(label), action, .. } = entry else { return None };
                Some(self.command_pick(&action, menu_path(&[parent], label, t), menu_path(&[parent], label, |name| name)))
            }));
        }
        if cfg!(windows) {
            picks.extend(crate::settings::TRAY_MENU.iter().map(|(label, action)| {
                self.command_pick(action, menu_path(&["Aster"], label, t), menu_path(&["Aster"], label, |name| name))
            }));
        }
        picks.retain(|pick| !self.config.palette.hidden_commands.contains(&pick.value));
        picks
    }
}

#[cfg(test)]
#[path = "../test/palette.rs"]
mod tests;
