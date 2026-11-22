use crate::input::{Key, Mods};
use std::ops::Range;

const HISTORY_LIMIT: usize = 128;

#[derive(Clone, Debug)]
struct Edit {
    start: usize,
    removed: String,
    inserted: String,
    cursor: usize,
    anchor: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TextEditor {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
    preferred_column: Option<usize>,
    pub top: usize,
    pub left: usize,
    undo: Vec<Edit>,
    redo: Vec<Edit>,
}

impl TextEditor {
    pub fn new(text: &str) -> Self {
        Self { text: text.replace("\r\n", "\n"), ..Self::default() }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn selection(&self) -> Range<usize> {
        let anchor = self.anchor.unwrap_or(self.cursor);
        anchor.min(self.cursor)..anchor.max(self.cursor)
    }

    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }

    pub fn lines(&self) -> impl Iterator<Item = (usize, &str)> {
        self.text.split('\n').scan(0, |next, line| {
            let start = *next;
            *next += line.len() + 1;
            Some((start, line))
        })
    }

    fn line_count(&self) -> usize {
        self.text.bytes().filter(|&byte| byte == b'\n').count() + 1
    }

    pub fn cursor_position(&self) -> (usize, usize) {
        let prefix = &self.text[..self.cursor];
        let row = prefix.bytes().filter(|&byte| byte == b'\n').count();
        let col = prefix.rsplit('\n').next().unwrap_or_default().chars().count();
        (row, col)
    }

    fn offset(&self, row: usize, col: usize) -> usize {
        self.lines().nth(row).map_or(self.text.len(), |(start, line)| {
            start + line.char_indices().nth(col).map_or(line.len(), |(index, _)| index)
        })
    }

    fn move_to(&mut self, offset: usize, extend: bool) {
        self.anchor = if extend { Some(self.anchor.unwrap_or(self.cursor)) } else { None };
        self.cursor = offset;
        self.preferred_column = None;
    }

    pub fn place_cursor(&mut self, row: usize, col: usize, extend: bool) {
        self.move_to(self.offset(row, col), extend);
    }

    pub fn reveal_cursor(&mut self, rows: usize, cols: usize) {
        let (row, col) = self.cursor_position();
        self.top = self.top.clamp(row.saturating_sub(rows.saturating_sub(1)), row);
        self.left = self.left.clamp(col.saturating_sub(cols.saturating_sub(1)), col);
    }

    pub fn scroll(&mut self, lines: isize, rows: usize) {
        self.top = self.top.saturating_add_signed(lines).min(self.line_count().saturating_sub(rows.max(1)));
    }

    fn replace(&mut self, range: Range<usize>, text: &str) -> bool {
        if self.text[range.clone()] == *text {
            self.move_to(range.start + text.len(), false);
            return false;
        }
        let edit = Edit {
            start: range.start, removed: self.text[range.clone()].to_owned(), inserted: text.to_owned(),
            cursor: self.cursor, anchor: self.anchor,
        };
        self.text.replace_range(range, text);
        self.move_to(edit.start + text.len(), false);
        if self.undo.len() == HISTORY_LIMIT { self.undo.remove(0); }
        self.undo.push(edit);
        self.redo.clear();
        true
    }

    pub fn insert(&mut self, text: &str) -> bool {
        if text.contains('\r') {
            return self.replace(self.selection(), &text.replace("\r\n", "\n").replace('\r', "\n"));
        }
        self.replace(self.selection(), text)
    }

    fn undo(&mut self, redo: bool) -> bool {
        if redo {
            let Some(edit) = self.redo.pop() else { return false };
            self.text.replace_range(edit.start..edit.start + edit.removed.len(), &edit.inserted);
            self.move_to(edit.start + edit.inserted.len(), false);
            self.undo.push(edit);
        } else {
            let Some(edit) = self.undo.pop() else { return false };
            self.text.replace_range(edit.start..edit.start + edit.inserted.len(), &edit.removed);
            self.move_to(edit.cursor, false);
            self.anchor = edit.anchor;
            self.redo.push(edit);
        }
        true
    }

    fn previous(&self) -> usize {
        self.text[..self.cursor].char_indices().next_back().map_or(0, |(offset, _)| offset)
    }

    fn next(&self) -> usize {
        self.cursor + self.text[self.cursor..].chars().next().map_or(0, char::len_utf8)
    }

    fn vertical(&mut self, movement: isize, select: bool) {
        let (row, col) = self.cursor_position();
        let target = row.saturating_add_signed(movement).min(self.line_count() - 1);
        let wanted = self.preferred_column.unwrap_or(col);
        self.place_cursor(target, wanted, select);
        self.preferred_column = Some(wanted);
    }

    pub fn key(&mut self, key: Key, mods: Mods, rows: usize, writable: bool) -> bool {
        let selection = self.selection();
        match key {
            Key::Char('A') if mods.ctrl => {
                self.move_to(self.text.len(), false);
                self.anchor = Some(0);
            }
            Key::Char('Z') if mods.ctrl && writable => return self.undo(mods.shift),
            Key::Char('Y') if mods.ctrl && writable => return self.undo(true),
            Key::Backspace if writable => return self.replace(if selection.is_empty() { self.previous()..self.cursor } else { selection }, ""),
            Key::Delete if writable => return self.replace(if selection.is_empty() { self.cursor..self.next() } else { selection }, ""),
            Key::Enter if writable && !mods.ctrl => return self.insert("\n"),
            Key::Tab if writable && !mods.ctrl => return self.insert("    "),
            Key::Left => self.move_to(if !mods.shift && !selection.is_empty() { selection.start } else { self.previous() }, mods.shift),
            Key::Right => self.move_to(if !mods.shift && !selection.is_empty() { selection.end } else { self.next() }, mods.shift),
            Key::Up => self.vertical(-1, mods.shift),
            Key::Down => self.vertical(1, mods.shift),
            Key::PageUp => self.vertical(-(rows as isize), mods.shift),
            Key::PageDown => self.vertical(rows as isize, mods.shift),
            Key::Home if mods.ctrl => self.move_to(0, mods.shift),
            Key::End if mods.ctrl => self.move_to(self.text.len(), mods.shift),
            Key::Home => self.place_cursor(self.cursor_position().0, 0, mods.shift),
            Key::End => self.place_cursor(self.cursor_position().0, usize::MAX, mods.shift),
            _ => {}
        }
        false
    }
}

#[cfg(test)]
#[path = "../test/text_editor.rs"]
mod tests;
