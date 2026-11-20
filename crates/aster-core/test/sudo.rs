use super::*;
use crate::config::Config;
use crate::input::{Event, Key, Mods};
use crate::profile::Profile;
use std::sync::Arc;

fn ssh_app() -> (App, PaneId) {
    let mut profile = Profile::new("ssh:box", "SSH: box", "ssh", &["box"]);
    profile.ssh = Some("ann@box".into());
    let mut a = App::new(Config::default(), vec![profile], true, None, Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    a.open_default_tab();
    let pane = a.focused_pane().unwrap();
    a.panes.get_mut(&pane).unwrap().pending = None;
    a.take_effects();
    (a, pane)
}

fn output(a: &mut App, pane: PaneId, bytes: &[u8]) {
    a.panes[&pane].terminal.lock().feed(bytes);
    a.woken();
}

fn press(a: &mut App, key: Key, mods: Mods) {
    a.on_event(Event::Key { key, mods, pressed: true });
}

fn typed(a: &mut App, text: &str) {
    for c in text.chars() {
        a.on_event(Event::Char(c));
    }
}

const FILL: Mods = Mods { ctrl: true, shift: true, alt: false, meta: false };

#[test]
fn a_saved_password_is_typed_on_request_at_a_sudo_prompt() {
    let (mut a, pane) = ssh_app();
    output(&mut a, pane, b"$ sudo apt update\r\n[sudo] password for ann: ");
    let effects = a.take_effects();
    assert!(effects.iter().any(|e| matches!(e, Effect::CheckSecret { key, .. } if key == "sudo:ann@box")));
    a.secret_known(pane, true);
    press(&mut a, Key::Enter, FILL);
    let effects = a.take_effects();
    assert!(effects.iter().any(|e| matches!(e, Effect::FillSecret { key, .. } if key == "sudo:ann@box")));
    assert!(!effects.iter().any(|e| matches!(e, Effect::Write { .. })), "the shortcut itself is not typed");
    let request = a.pending_secret_fill(pane).unwrap();
    assert!(a.finish_secret_fill(pane, "sudo:ann@box", request, true));
    assert!(!a.finish_secret_fill(pane, "sudo:ann@box", request, true));
    output(&mut a, pane, b"\r\nHit:1 http://archive\r\n");
    assert!(a.password_prompt(pane).is_none());
}

#[test]
fn a_delayed_fill_rechecks_the_live_prompt_before_woken() {
    for replacement in [b"\r\x1b[2K$ ".as_slice(), b"\r\x1b[2KOther password: ", b"\r\nPassword: "] {
        let (mut a, pane) = ssh_app();
        output(&mut a, pane, b"Password: ");
        a.secret_known(pane, true);
        press(&mut a, Key::Enter, FILL);
        let request = a.pending_secret_fill(pane).unwrap();
        a.panes[&pane].terminal.lock().feed(replacement);
        assert!(!a.finish_secret_fill(pane, "sudo:ann@box", request, true));
    }
}

#[test]
fn input_cancels_a_pending_fill_even_when_the_next_request_uses_the_same_line() {
    let (mut a, pane) = ssh_app();
    output(&mut a, pane, b"Password: ");
    a.secret_known(pane, true);
    press(&mut a, Key::Enter, FILL);
    let first = a.pending_secret_fill(pane).unwrap();
    a.note_input(pane, b"\x15");
    assert!(a.pending_secret_fill(pane).is_none());
    press(&mut a, Key::Enter, FILL);
    let second = a.pending_secret_fill(pane).unwrap();
    assert!(!a.finish_secret_fill(pane, "sudo:ann@box", first, true));
    assert!(a.finish_secret_fill(pane, "sudo:ann@box", second, true));
}

#[test]
fn a_missing_fill_result_allows_the_manual_password_to_be_saved() {
    let (mut a, pane) = ssh_app();
    output(&mut a, pane, b"Password: ");
    a.secret_known(pane, true);
    press(&mut a, Key::Enter, FILL);
    let request = a.pending_secret_fill(pane).unwrap();
    assert!(!a.finish_secret_fill(pane, "sudo:ann@box", request, false));
    a.note_input(pane, b"manual\r");
    output(&mut a, pane, b"\r\n$ ");
    assert!(a.password_prompt(pane).unwrap().offer);
}

#[test]
fn an_open_prompt_tracks_vault_changes_without_losing_manual_input_or_save_offers() {
    let (mut a, pane) = ssh_app();
    let key = "sudo:ann@box";
    let locked = VaultView { configured: true, ..VaultView::default() };
    let mut open = VaultView { unlocked: true, entries: vec![("password".into(), key.into())], ..locked.clone() };
    output(&mut a, pane, b"Password: ");
    a.secret_known(pane, true);
    press(&mut a, Key::Enter, FILL);
    let request = a.pending_secret_fill(pane).unwrap();
    a.vault_state(open.clone());
    assert!(!a.finish_secret_fill(pane, key, request, true));
    a.vault_state(locked.clone());
    assert_eq!(a.password_prompt(pane).unwrap().saved, Some(false));
    a.note_input(pane, b"man");
    a.vault_state(open.clone());
    assert_eq!(a.password_prompt(pane).unwrap().saved, Some(true));
    assert!(!a.fill_password());
    open.entries.clear();
    a.vault_state(open);
    assert_eq!(a.password_prompt(pane).unwrap().saved, Some(false));
    a.take_effects();
    a.vault_state(VaultView::default());
    assert_eq!(a.password_prompt(pane).unwrap().saved, None);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::CheckSecret { key: k, .. } if k == key)));
    a.secret_known(pane, false);
    a.note_input(pane, b"ual\r");
    output(&mut a, pane, b"\r\n$ ");
    a.vault_state(locked);
    assert!(a.password_prompt(pane).unwrap().offer);
    press(&mut a, Key::Enter, FILL);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::SaveSecret { value, .. } if value == "manual")));
}

#[test]
fn a_typed_password_that_worked_can_be_saved() {
    let (mut a, pane) = ssh_app();
    output(&mut a, pane, b"[sudo] password for ann: ");
    a.secret_known(pane, false);
    a.take_effects();
    typed(&mut a, "s3cr3x");
    press(&mut a, Key::Backspace, Mods::NONE);
    a.on_event(Event::Char('\u{7f}'));
    typed(&mut a, "t\r");
    output(&mut a, pane, b"\r\n$ ");
    assert!(a.password_prompt(pane).is_some_and(|p| p.offer));
    press(&mut a, Key::Enter, FILL);
    let effects = a.take_effects();
    assert!(effects.iter().any(|e| matches!(e, Effect::SaveSecret { key, value } if key == "sudo:ann@box" && value == "s3cr3t")), "{:?}", effects.iter().filter(|e| matches!(e, Effect::SaveSecret { .. })).count());
    assert!(a.password_prompt(pane).is_none());
}

#[test]
fn a_rejected_password_is_not_offered_and_the_next_prompt_starts_fresh() {
    let (mut a, pane) = ssh_app();
    output(&mut a, pane, b"[sudo] password for ann: ");
    a.secret_known(pane, false);
    typed(&mut a, "wrong\r");
    output(&mut a, pane, b"\r\nSorry, try again.\r\n[sudo] password for ann: ");
    let prompt = a.password_prompt(pane).cloned().expect("still asking");
    assert!(!prompt.offer && prompt.typed.is_empty() && !prompt.submitted);
    typed(&mut a, "nope\r");
    output(&mut a, pane, b"\r\nsudo: 3 incorrect password attempts\r\n$ ");
    assert!(a.password_prompt(pane).is_none(), "failures never turn into an offer");
}

#[test]
fn any_other_key_dismisses_the_offer_and_reaches_the_shell() {
    let (mut a, pane) = ssh_app();
    output(&mut a, pane, b"Password: ");
    a.secret_known(pane, false);
    typed(&mut a, "pw\r");
    output(&mut a, pane, b"\r\n$ ");
    assert!(a.password_prompt(pane).is_some_and(|p| p.offer));
    a.take_effects();
    press(&mut a, Key::Char('L'), Mods::NONE);
    typed(&mut a, "l");
    assert!(a.password_prompt(pane).is_none());
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Write { bytes, .. } if bytes == b"l")));
}

#[test]
fn local_tabs_use_their_profile_as_the_key_and_the_feature_can_be_off() {
    let mut a = App::new(Config::default(), vec![Profile::new("local:wsl:u", "WSL", "wsl.exe", &[])], true, None, Arc::new(|| {}));
    a.open_default_tab();
    let pane = a.focused_pane().unwrap();
    a.panes.get_mut(&pane).unwrap().pending = None;
    output(&mut a, pane, b"[sudo] password for me: ");
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::CheckSecret { key, .. } if key == "sudo:local:wsl:u")));
    let mut config = Config::default();
    config.terminal.sudo_passwords = false;
    a.set_config(config);
    output(&mut a, pane, b"\r\n[sudo] password for me: ");
    assert!(a.password_prompt(pane).is_none());
}
