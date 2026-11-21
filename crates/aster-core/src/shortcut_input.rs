use super::App;
use crate::input::{Key, Mods};
use crate::keymap::{Action, Chord, Outcome};
use std::mem;

#[derive(Default)]
pub(crate) struct ModifierGesture {
    held: Mods,
    peak: Option<Mods>,
}

fn includes(held: Mods, buttons: Mods) -> bool {
    (!buttons.ctrl || held.ctrl) && (!buttons.shift || held.shift) && (!buttons.alt || held.alt) && (!buttons.meta || held.meta)
}

impl ModifierGesture {
    pub(crate) fn change(&mut self, mods: Mods) -> Option<Mods> {
        let before = mem::replace(&mut self.held, mods);
        if !mods.any() {
            return self.peak.take();
        }
        if !before.any() {
            self.peak = Some(mods);
        } else if let Some(peak) = self.peak {
            if !includes(peak, mods) {
                self.peak = (before == peak && includes(mods, before)).then_some(mods);
            }
        }
        None
    }

    pub(crate) fn cancel(&mut self) {
        self.peak = None;
    }
}

impl App {
    pub(super) fn capture_shortcut(&mut self, chord: Chord) -> bool {
        if !self.settings.as_ref().is_some_and(|page| page.capture.is_some()) {
            return false;
        }
        self.settings_key(chord.key, chord.mods);
        true
    }

    pub(super) fn non_keyboard_shortcut(&mut self, chord: Chord) -> bool {
        if self.overlay.is_some() || self.menu.is_some() || self.paste_confirm.is_some() || self.close_confirm.is_some() || self.rename.is_some() || self.files.is_some()
            || self.settings.as_ref().is_some_and(|page| page.editing() || page.editor.is_some() || page.group_editor.is_some())
            || !self.keymap.accepts(chord)
        {
            return false;
        }
        match self.keymap.press(chord) {
            Outcome::Action(Action::Settings) if self.settings.is_some() => {
                self.close_settings();
                true
            }
            Outcome::Action(action) if self.settings.is_none() || action.works_beside_settings() => {
                if action != Action::FillPassword {
                    self.dismiss_offer();
                }
                let held = if chord.key == Key::ModifiersOnly { Mods::NONE } else { chord.mods };
                self.run_shortcut(action, held);
                true
            }
            Outcome::Pending => true,
            _ => false,
        }
    }
}

#[cfg(test)]
#[path = "../test/shortcut_input.rs"]
mod tests;
