use super::*;
use crate::input::{Event, MouseButton};
use crate::keymap::Action;
use std::sync::Arc;

const ORIGINAL: &str = "version = 1\n[terminal]\nfont-size = 16\n";
const CHANGED: &str = "version = 1\n[terminal]\nfont-size = 19\n";
const CTRL: Mods = Mods { ctrl: true, ..Mods::NONE };

fn key(app: &mut App, key: Key) {
    app.on_event(Event::Key { key, mods: CTRL, pressed: true });
}

fn click(app: &mut App, rect: Rect) {
    let (x, y) = (rect.x + 4.0, rect.y + 4.0);
    app.on_event(Event::MouseDown { x, y, button: MouseButton::Left, clicks: 1 });
    app.on_event(Event::MouseUp { x, y, button: MouseButton::Left });
}

fn open() -> App {
    let mut app = App::new(Config::parse(ORIGINAL).unwrap(), Vec::new(), true, None, Arc::new(|| {}));
    app.set_viewport(1100, 700, 1.0);
    app.chrome = app.tab_chrome();
    app.set_config_path(Some("data/config.toml".into()));
    app.set_config_editor_source(Ok(Some(ORIGINAL.into())));
    app.run(Action::Settings);
    key(&mut app, Key::PageUp);
    let file = app.config_editor_layout().unwrap().file.text;
    click(&mut app, file);
    app.take_effects();
    app
}

fn replace(app: &mut App, text: &str) {
    key(app, Key::Char('A'));
    app.on_event(Event::Pasted(text.into()));
}

#[test]
fn invalid_edits_disable_saving_and_undo_restores_the_document() {
    let mut app = open();
    replace(&mut app, "[terminal]\nfont-size = ]");
    assert_eq!(app.config_editor.save_label(), "Syntax error");
    assert_eq!(app.config_editor.error_line(), Some(1));
    assert!(app.config_editor.status(&app.config).0.contains("Line 2, column 13"));
    key(&mut app, Key::Char('S'));
    assert!(!app.take_effects().iter().any(|e| matches!(e, Effect::SaveRawConfig { .. })));
    key(&mut app, Key::Char('Z'));
    assert_eq!(app.config_editor.file.text(), ORIGINAL);
    replace(&mut app, CHANGED);
    key(&mut app, Key::Char('S'));
    assert!(app.take_effects().iter().any(|e| matches!(e, Effect::SaveRawConfig { text, expected } if text == CHANGED && expected.as_deref() == Some(ORIGINAL))));
    assert_eq!(app.config.terminal.font_size, 16.0);
}

#[test]
fn the_defaults_pane_can_be_copied_and_does_not_accept_paste() {
    let mut app = open();
    let defaults = app.config_editor_layout().unwrap().defaults;
    click(&mut app, defaults);
    let reference = app.config_editor_layout().unwrap().reference.unwrap();
    click(&mut app, reference.text);
    replace(&mut app, "discard");
    key(&mut app, Key::Char('X'));
    let text = Config::defaults_text();
    assert!(app.take_effects().iter().any(|e| matches!(e, Effect::SetClipboard(value) if value == &text)));
    assert_eq!(app.config_editor.defaults.as_ref().unwrap().text(), text);
    assert_eq!(app.config_editor.file.text(), ORIGINAL);
}

#[test]
fn disk_conflicts_keep_the_draft_until_an_explicit_reload() {
    let mut app = open();
    replace(&mut app, CHANGED);
    let updated = "version = 1\n[terminal]\nfont-size = 21\n";
    app.set_config_editor_source(Ok(Some(updated.into())));
    assert_eq!(app.config_editor.file.text(), CHANGED);
    assert!(!app.config_editor.can_save(false));
    let reload = app.config_editor_layout().unwrap().reload.unwrap();
    click(&mut app, reload);
    assert_eq!(app.config_editor.file.text(), updated);
    key(&mut app, Key::Char('Z'));
    assert_eq!(app.config_editor.file.text(), CHANGED);
    key(&mut app, Key::Char('S'));
    assert!(app.take_effects().iter().any(|e| matches!(e, Effect::SaveRawConfig { text, expected } if text == CHANGED && expected.as_deref() == Some(updated))));
}

#[test]
fn a_delayed_clipboard_reply_cannot_insert_after_leaving_the_editor() {
    let mut app = open();
    key(&mut app, Key::Char('V'));
    let request = app.take_effects().into_iter().find_map(|effect| match effect {
        Effect::RequestConfigPaste(request) => Some(request),
        _ => None,
    }).unwrap();
    key(&mut app, Key::PageDown);
    key(&mut app, Key::PageUp);
    let file = app.config_editor_layout().unwrap().file.text;
    click(&mut app, file);
    app.on_event(Event::ConfigEditorPasted { request, text: "delayed".into() });
    assert_eq!(app.config_editor.file.text(), ORIGINAL);
}
