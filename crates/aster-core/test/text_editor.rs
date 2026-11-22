use super::*;

const CTRL: Mods = Mods { ctrl: true, ..Mods::NONE };
const SHIFT: Mods = Mods { shift: true, ..Mods::NONE };

#[test]
fn unicode_selection_and_multiline_paste_are_reversible() {
    let mut editor = TextEditor::new("a界\r\nb");
    editor.place_cursor(0, 1, false);
    editor.key(Key::Right, SHIFT, 4, true);
    assert_eq!(editor.selected_text(), "界");
    editor.insert("x\r\ny");
    assert_eq!(editor.text(), "ax\ny\nb");
    assert_eq!(editor.cursor_position(), (1, 1));
    assert!(editor.key(Key::Char('Z'), CTRL, 4, true));
    assert_eq!(editor.text(), "a界\nb");
    assert_eq!(editor.selected_text(), "界");
    assert!(editor.key(Key::Char('Y'), CTRL, 4, true));
    assert_eq!(editor.text(), "ax\ny\nb");
}

#[test]
fn vertical_motion_restores_the_column_after_a_short_line() {
    let mut editor = TextEditor::new("abcd\nx\nabcd\nlast");
    editor.place_cursor(0, 3, false);
    editor.key(Key::Down, Mods::NONE, 2, true);
    assert_eq!(editor.cursor_position(), (1, 1));
    editor.key(Key::Down, Mods::NONE, 2, true);
    assert_eq!(editor.cursor_position(), (2, 3));
    editor.reveal_cursor(2, 2);
    assert_eq!((editor.top, editor.left), (1, 2));
    editor.scroll(10, 2);
    assert_eq!(editor.top, 2);
}

#[test]
fn readonly_text_allows_navigation_and_selection_without_edits() {
    let mut editor = TextEditor::new("one\ntwo");
    editor.key(Key::Char('A'), CTRL, 4, false);
    assert_eq!(editor.selected_text(), "one\ntwo");
    for key in [Key::Backspace, Key::Delete, Key::Enter, Key::Tab, Key::Char('Z')] {
        assert!(!editor.key(key, CTRL, 4, false));
    }
    assert_eq!(editor.text(), "one\ntwo");
}
