use crate::command_marks::Marks;
use crate::osc::CommandMark;
use crate::{Line, Listener, TermMode, VtEvent};
use alacritty_terminal::grid::{BidirectionalIterator, Dimensions, GridCell};
use alacritty_terminal::index::{Column, Point};
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::*;
use alacritty_terminal::vte::ansi::cursor_icon::CursorIcon;
use std::collections::VecDeque;
use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use unicode_width::UnicodeWidthChar;

pub(crate) struct HandlerWithMarks<'a> {
    pub(crate) term: &'a mut Term<Listener>,
    pub(crate) marks: &'a mut Marks,
    pub(crate) region: &'a mut Range<i32>,
    pub(crate) deferred: &'a mut DeferredMarks,
    pub(crate) listener: &'a Listener,
    pub(crate) output_seq: u64,
}

const DEFERRED_TITLE: &str = "\u{10ffff}ASTER_OSC_133_";

#[derive(Default)]
pub(crate) struct DeferredMarks(VecDeque<(u64, CommandMark, bool)>);

impl HandlerWithMarks<'_> {
    pub(crate) fn command_mark(&mut self, mark: CommandMark, notify: bool) {
        if let Some(code) = self.marks.event(self.term, mark).filter(|_| notify) {
            (self.listener.sink)(VtEvent::CommandFinished { code, output_seq: self.output_seq });
        }
    }

    pub(crate) fn defer_command_mark(&mut self, mark: CommandMark, notify: bool) -> Vec<u8> {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        self.deferred.0.push_back((id, mark, notify));
        format!("\x1b]2;{DEFERRED_TITLE}{id}\x07").into_bytes()
    }

    fn tracking(&self) -> bool {
        self.marks.positions_present() && !self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    fn at_scroll_bottom(&self) -> bool {
        self.term.grid().cursor.point.line.0 + 1 == self.region.end
    }

    fn shift(&mut self, from: i32, delta: i32) {
        if self.tracking() {
            let region = from..self.region.end;
            self.marks.shift(region, delta, self.term.history_size(), self.term.screen_lines() as i32);
        }
    }

    fn column_mode(&mut self, mode: PrivateMode) {
        if mode == PrivateMode::Named(NamedPrivateMode::ColumnMode) {
            *self.region = 0..self.term.screen_lines() as i32;
        }
    }
}

macro_rules! forward {
    ($($name:ident($($arg:ident: $kind:ty),*);)*) => {$(
        fn $name(&mut self, $($arg: $kind),*) {
            self.term.$name($($arg),*);
        }
    )*};
}

impl Handler for HandlerWithMarks<'_> {
    fn set_title(&mut self, title: Option<String>) {
        let id = title.as_deref().and_then(|title| title.strip_prefix(DEFERRED_TITLE))
            .and_then(|id| id.parse::<u64>().ok());
        if self.deferred.0.front().is_some_and(|(next, _, _)| Some(*next) == id) {
            let (_, mark, notify) = self.deferred.0.pop_front().unwrap();
            self.command_mark(mark, notify);
        } else {
            self.term.set_title(title);
        }
    }

    fn input(&mut self, c: char) {
        let scrolls = self.tracking() && self.at_scroll_bottom() && self.term.mode().contains(TermMode::LINE_WRAP) && {
            let cursor = &self.term.grid().cursor;
            let at_right = cursor.point.column == self.term.last_column();
            (cursor.input_needs_wrap && c.width().is_some_and(|width| width > 0))
                || (at_right && c.width() == Some(2))
        };
        self.term.input(c);
        if scrolls { self.shift(self.region.start, -1); }
    }

    fn put_tab(&mut self, count: u16) {
        let scrolls = self.at_scroll_bottom() && self.term.grid().cursor.input_needs_wrap
            && self.term.mode().contains(TermMode::LINE_WRAP);
        self.term.put_tab(count);
        if scrolls { self.shift(self.region.start, -1); }
    }

    fn linefeed(&mut self) {
        let scrolls = self.at_scroll_bottom();
        self.term.linefeed();
        if scrolls { self.shift(self.region.start, -1); }
    }

    fn newline(&mut self) {
        let scrolls = self.at_scroll_bottom();
        self.term.newline();
        if scrolls { self.shift(self.region.start, -1); }
    }

    fn scroll_up(&mut self, count: usize) {
        self.term.scroll_up(count);
        let count = count.min((self.region.end - self.region.start) as usize);
        self.shift(self.region.start, -(count as i32));
    }

    fn scroll_down(&mut self, count: usize) {
        self.term.scroll_down(count);
        let count = count.min((self.region.end - self.region.start) as usize);
        self.shift(self.region.start, count as i32);
    }

    fn reverse_index(&mut self) {
        let scrolls = self.term.grid().cursor.point.line.0 == self.region.start;
        self.term.reverse_index();
        if scrolls { self.shift(self.region.start, 1); }
    }

    fn insert_blank_lines(&mut self, count: usize) {
        let from = self.term.grid().cursor.point.line.0;
        self.term.insert_blank_lines(count);
        if self.region.contains(&from) {
            self.shift(from, count.min((self.region.end - from) as usize) as i32);
        }
    }

    fn delete_lines(&mut self, count: usize) {
        let from = self.term.grid().cursor.point.line.0;
        self.term.delete_lines(count);
        if self.region.contains(&from) {
            self.shift(from, -(count.min((self.region.end - from) as usize) as i32));
        }
    }

    fn clear_screen(&mut self, mode: ClearMode) {
        let primary = self.tracking();
        let rows = self.term.screen_lines() as i32;
        let scrolled = if primary && matches!(mode, ClearMode::All) {
            let grid = self.term.grid();
            let mut cells = grid.iter_from(Point::new(Line(rows - 1), Column(self.term.columns())));
            while let Some(cell) = cells.prev() {
                if !cell.is_empty() || cell.point.line.0 < 0 { break; }
            }
            cells.point().line.0 + 1
        } else {
            0
        };
        let clears_history = matches!(mode, ClearMode::Saved);
        self.term.clear_screen(mode);
        if primary {
            self.marks.shift(0..rows, -scrolled, self.term.history_size(), rows);
            if clears_history { self.marks.retain_history(0, rows as usize); }
        }
    }

    fn reset_state(&mut self) {
        self.term.reset_state();
        self.marks.reset();
        *self.region = 0..self.term.screen_lines() as i32;
    }

    fn set_scrolling_region(&mut self, top: usize, bottom: Option<usize>) {
        let rows = self.term.screen_lines();
        let bottom = bottom.unwrap_or(rows);
        if top < bottom {
            *self.region = (top.saturating_sub(1).min(rows) as i32)..(bottom.min(rows) as i32);
        }
        self.term.set_scrolling_region(top, Some(bottom));
    }

    fn set_private_mode(&mut self, mode: PrivateMode) {
        self.term.set_private_mode(mode);
        self.column_mode(mode);
    }

    fn unset_private_mode(&mut self, mode: PrivateMode) {
        let old_alt = self.term.mode().contains(TermMode::ALT_SCREEN);
        self.term.unset_private_mode(mode);
        if old_alt && !self.term.mode().contains(TermMode::ALT_SCREEN) {
            self.marks.leave_alt(self.term);
        }
        self.column_mode(mode);
    }

    forward! {
        set_cursor_style(style: Option<CursorStyle>);
        set_cursor_shape(shape: CursorShape);
        goto(line: i32, col: usize);
        goto_line(line: i32);
        goto_col(col: usize);
        insert_blank(count: usize);
        move_up(count: usize);
        move_down(count: usize);
        identify_terminal(intermediate: Option<char>);
        device_status(status: usize);
        move_forward(count: usize);
        move_backward(count: usize);
        move_down_and_cr(count: usize);
        move_up_and_cr(count: usize);
        backspace();
        carriage_return();
        bell();
        substitute();
        set_horizontal_tabstop();
        erase_chars(count: usize);
        delete_chars(count: usize);
        move_backward_tabs(count: u16);
        move_forward_tabs(count: u16);
        save_cursor_position();
        restore_cursor_position();
        clear_line(mode: LineClearMode);
        clear_tabs(mode: TabulationClearMode);
        set_tabs(interval: u16);
        terminal_attribute(attr: Attr);
        set_mode(mode: Mode);
        unset_mode(mode: Mode);
        report_mode(mode: Mode);
        report_private_mode(mode: PrivateMode);
        set_keypad_application_mode();
        unset_keypad_application_mode();
        set_active_charset(index: CharsetIndex);
        configure_charset(index: CharsetIndex, charset: StandardCharset);
        set_color(index: usize, rgb: Rgb);
        dynamic_color_sequence(prefix: String, index: usize, terminator: &str);
        reset_color(index: usize);
        clipboard_store(clipboard: u8, data: &[u8]);
        clipboard_load(clipboard: u8, terminator: &str);
        decaln();
        push_title();
        pop_title();
        text_area_size_pixels();
        text_area_size_chars();
        set_hyperlink(link: Option<Hyperlink>);
        set_mouse_cursor_icon(icon: CursorIcon);
        report_keyboard_mode();
        push_keyboard_mode(mode: KeyboardModes);
        pop_keyboard_modes(count: u16);
        set_keyboard_mode(mode: KeyboardModes, behavior: KeyboardModesApplyBehavior);
        set_modify_other_keys(mode: ModifyOtherKeys);
        report_modify_other_keys();
        set_scp(path: ScpCharPath, update: ScpUpdateMode);
    }
}
