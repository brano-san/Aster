mod indexed_colors;
mod command_handler;
mod command_marks;
pub mod buffer_text;
pub mod fixture;
mod osc;
mod palette;
pub mod schemes;
pub mod search;

pub use alacritty_terminal::grid::Scroll;
pub use alacritty_terminal::index::{Column, Line, Point, Side};
pub use alacritty_terminal::selection::SelectionType;
pub use alacritty_terminal::sync::FairMutex;
pub use alacritty_terminal::term::cell::{Cell, Flags};
pub use alacritty_terminal::term::TermMode;
pub use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor, Rgb};
pub use command_marks::CommandSummary;
pub use osc::{Progress, ProgressState};
pub use palette::Palette;
pub use search::{SearchOverview, SearchStatus, VisibleMatch};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::selection::Selection;
use alacritty_terminal::term::cell::LineLength;
use alacritty_terminal::term::{Config, Osc52, Term};
use alacritty_terminal::vte::ansi::{CursorStyle, Processor, StdSyncHandler};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VtEvent {
    Write(Vec<u8>),
    Title(String),
    ResetTitle,
    Bell,
    Clipboard(String),
    CommandFinished { code: Option<i32>, output_seq: u64 },
}

pub type Sink = Arc<dyn Fn(VtEvent) + Send + Sync>;

#[derive(Clone)]
pub struct Listener {
    sink: Sink,
    cell: Arc<AtomicU32>,
    replaying: Arc<AtomicBool>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let ev = match event {
            Event::PtyWrite(s) => VtEvent::Write(s.into_bytes()),
            Event::Title(t) => VtEvent::Title(t),
            Event::ResetTitle => VtEvent::ResetTitle,
            Event::Bell => VtEvent::Bell,
            Event::ClipboardStore(_, text) => VtEvent::Clipboard(text),
            Event::TextAreaSizeRequest(format) => {
                let packed = self.cell.load(Ordering::Relaxed);
                let size = WindowSize { num_lines: 0, num_cols: 0, cell_width: (packed >> 16) as u16, cell_height: packed as u16 };
                VtEvent::Write(format(size).into_bytes())
            }
            _ => return,
        };
        if !self.replaying.load(Ordering::Relaxed) || matches!(ev, VtEvent::Title(_) | VtEvent::ResetTitle) {
            (self.sink)(ev);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub cols: usize,
    pub rows: usize,
}

impl Size {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self { cols: cols.max(2), rows: rows.max(1) }
    }
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.rows
    }
    fn screen_lines(&self) -> usize {
        self.rows
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    pub scrollback: usize,
    #[serde(with = "fixture::Shape")]
    pub cursor: CursorShape,
    pub blink: bool,
    pub word_separators: String,
}

impl Default for Options {
    fn default() -> Self {
        Self { scrollback: 25_000, cursor: CursorShape::Block, blink: true, word_separators: " ()[]{}'\"".into() }
    }
}

pub struct Terminal {
    term: Term<Listener>,
    parser: Processor<StdSyncHandler>,
    size: Size,
    cell: Arc<AtomicU32>,
    title: Option<String>,
    seq: u64,
    output_seq: u64,
    progress: Option<osc::Progress>,
    sniffer: osc::Sniffer,
    marks: command_marks::Marks,
    deferred_marks: command_handler::DeferredMarks,
    scroll_region: std::ops::Range<i32>,
    cwd: Option<String>,
    cwd_reported: bool,
    search: Option<search::Search>,
    selection_from_buffer_match: bool,
    options: Options,
    listener: Listener,
    recording: Option<fixture::Recording>,
}

pub struct VisibleCell<'a> {
    pub line: usize,
    pub col: usize,
    pub cell: &'a Cell,
    pub selected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub line: usize,
    pub col: usize,
    #[serde(with = "fixture::Shape")]
    pub shape: CursorShape,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wide: bool,
}

impl Terminal {
    pub fn new(size: Size, options: Options, sink: Sink) -> Self {
        let cell = Arc::new(AtomicU32::new(0));
        let listener = Listener { sink, cell: cell.clone(), replaying: Arc::default() };
        Self { term: Term::new(config(&options), &size, listener.clone()), parser: Processor::new(), size, cell, title: None, seq: 0, output_seq: 0, progress: None, sniffer: osc::Sniffer::default(), marks: command_marks::Marks::default(), deferred_marks: Default::default(), scroll_region: 0..size.rows as i32, cwd: None, cwd_reported: false, search: None, selection_from_buffer_match: false, options, listener, recording: None }
    }

    pub fn blank_for_replay(&self, size: Size) -> Self {
        let mut fresh = Self::new(size, self.options.clone(), self.listener.sink.clone());
        fresh.cwd.clone_from(&self.cwd);
        fresh.cwd_reported = self.cwd_reported;
        if self.recording.is_some() { fresh.enable_fixture_recording(); }
        fresh
    }

    pub fn feed_replay(&mut self, bytes: &[u8]) {
        self.feed_replay_inner(bytes, false);
    }

    fn feed_replay_inner(&mut self, bytes: &[u8], saved: bool) {
        self.listener.replaying.store(true, Ordering::Relaxed);
        self.feed_inner(bytes, saved);
        self.listener.replaying.store(false, Ordering::Relaxed);
    }

    pub fn replace_with_replay(&mut self, mut fresh: Self) {
        fresh.resize(self.size);
        if fresh.options != self.options { fresh.set_options(self.options.clone()); }
        fresh.term.is_focused = self.term.is_focused;
        fresh.cell.store(self.cell.load(Ordering::Relaxed), Ordering::Relaxed);
        fresh.title.clone_from(&self.title);
        fresh.seq = self.seq.wrapping_add(1);
        fresh.output_seq = self.output_seq.wrapping_add(1);
        fresh.search = self.search.take();
        if let Some(found) = fresh.search.as_mut() { found.reset_index(); }
        *self = fresh;
    }

    pub fn next_session(&mut self, options: Options, sink: Sink, cwd: Option<String>) -> Self {
        if let Some(recording) = &mut self.recording {
            recording.push(fixture::Step::NewSession { options: options.clone() });
        }
        if self.mode().contains(TermMode::ALT_SCREEN) {
            self.term.swap_alt();
            self.marks.leave_alt(&mut self.term);
        }
        let mut fresh = Self::new(self.size, options, sink);
        fresh.term.is_focused = self.term.is_focused;
        fresh.cell.store(self.cell.load(Ordering::Relaxed), Ordering::Relaxed);
        let grid = self.term.grid_mut();
        let point = grid.cursor.point;
        grid.cursor = Default::default();
        grid.cursor.point = point;
        grid.saved_cursor = grid.cursor.clone();
        std::mem::swap(fresh.term.grid_mut(), grid);
        fresh.term.grid_mut().update_history(fresh.options.scrollback);
        fresh.marks = self.marks.new_session();
        fresh.recording = self.recording.take();
        fresh.marks.retain_history(fresh.history(), fresh.size.rows);
        fresh.cwd = cwd;
        fresh.seq = self.seq.wrapping_add(1);
        fresh.output_seq = self.output_seq;
        fresh.search = self.search.take();
        if let Some(found) = fresh.search.as_mut() {
            found.reset_index();
        }
        fresh
    }

    pub fn set_search(&mut self, query: &str, options: search::SearchOptions) -> Result<(), String> {
        self.search = None;
        self.bump();
        if query.is_empty() {
            return Ok(());
        }
        let mut found = search::Search::new(search::compile(query, options)?);
        found.refresh(&self.term);
        self.search = Some(found);
        Ok(())
    }

    pub fn clear_search(&mut self) {
        self.search = None;
        self.bump();
    }

    pub fn search_step(&mut self, older: bool) -> bool {
        let Some(found) = self.search.as_mut() else { return false };
        found.refresh(&self.term);
        let Some(point) = found.step(older) else { return false };
        self.term.scroll_to_point(point);
        self.record_fixture_view();
        self.bump();
        true
    }

    pub fn search_status(&mut self) -> Option<SearchStatus> {
        let found = self.search.as_mut()?;
        found.refresh(&self.term);
        Some(found.status())
    }

    pub fn visible_matches(&mut self) -> Vec<VisibleMatch> {
        let Some(found) = self.search.as_mut() else { return Vec::new() };
        found.refresh(&self.term);
        found.visible(self.term.grid().display_offset(), self.size.rows, self.size.cols)
    }

    pub fn search_overview(&mut self) -> Option<SearchOverview> {
        let found = self.search.as_mut()?;
        found.refresh(&self.term);
        let grid = self.term.grid();
        Some(found.overview(grid.history_size(), grid.display_offset(), self.size.rows))
    }

    pub fn search_jump_to_row(&mut self, row: usize) -> bool {
        let Some(found) = self.search.as_mut() else { return false };
        found.refresh(&self.term);
        let grid = self.term.grid();
        let history = grid.history_size();
        let row = row.min(history + self.size.rows - 1);
        if !found.select_nearest(row, history) { return false; }
        let viewport_start = row.saturating_sub(self.size.rows / 2).min(history);
        let offset = history - viewport_start;
        self.term.scroll_display(Scroll::Delta(offset as i32 - grid.display_offset() as i32));
        self.record_fixture_view();
        self.bump();
        true
    }

    pub fn search_marks(&mut self) -> Vec<(f32, bool)> {
        let Some(found) = self.search.as_mut() else { return Vec::new() };
        found.refresh(&self.term);
        found.marks(self.term.grid().history_size(), self.size.rows)
    }

    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    pub fn set_cwd(&mut self, cwd: Option<String>) {
        self.cwd = cwd;
        self.cwd_reported = false;
    }

    pub fn reported_cwd(&self) -> Option<&str> {
        if self.cwd_reported { self.cwd() } else { None }
    }

    pub fn line_text(&self, line: usize) -> (String, Vec<usize>) {
        let mut text = String::new();
        let mut cols = Vec::new();
        let point = self.viewport_point(line, 0);
        let row = &self.term.grid()[point.line];
        for col in 0..self.size.cols {
            let cell = &row[Column(col)];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            text.push(cell.c);
            cols.push(col);
        }
        (text, cols)
    }

    pub fn set_options(&mut self, options: Options) {
        if self.options != options {
            if let Some(recording) = &mut self.recording {
                recording.push(fixture::Step::Options { options: options.clone() });
            }
        }
        self.term.set_options(config(&options));
        let primary_history = if self.mode().contains(TermMode::ALT_SCREEN) { options.scrollback } else { self.history() };
        self.marks.retain_history(primary_history, self.size.rows);
        self.options = options;
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.feed_inner(bytes, false);
    }

    fn feed_inner(&mut self, bytes: &[u8], saved: bool) {
        if let Some(recording) = &mut self.recording { recording.output(bytes, saved); }
        let anchor = self.search.as_ref().map(|_| search::Search::anchor(&self.term, self.options.scrollback));
        let (cwd, progress) = (&mut self.cwd, &mut self.progress);
        let cwd_reported = &mut self.cwd_reported;
        let output_seq = self.output_seq.wrapping_add(1);
        let notify = !self.listener.replaying.load(Ordering::Relaxed);
        let mut handler = command_handler::HandlerWithMarks {
            term: &mut self.term, marks: &mut self.marks, region: &mut self.scroll_region,
            deferred: &mut self.deferred_marks, listener: &self.listener, output_seq,
        };
        let parser = &mut self.parser;
        let mut consumed = 0;
        self.sniffer.feed(bytes, |end, payload| {
            if let Some(dir) = osc::cwd(payload) {
                *cwd = Some(dir);
                *cwd_reported = true;
            }
            if let Some(state) = osc::progress(payload) {
                *progress = state;
            }
            if let Some(mark) = osc::command_mark(payload, saved) {
                parser.advance(&mut handler, &bytes[consumed..end]);
                consumed = end;
                if parser.sync_bytes_count() == 0 {
                    handler.command_mark(mark, notify);
                } else {
                    let dispatch = handler.defer_command_mark(mark, notify);
                    parser.advance(&mut handler, &dispatch);
                }
            }
        });
        parser.advance(&mut handler, &bytes[consumed..]);
        if let (Some(found), Some(anchor)) = (self.search.as_mut(), anchor) {
            found.after_feed(&self.term, anchor);
        }
        self.output_seq = output_seq;
        self.bump();
    }

    fn bump(&mut self) {
        self.seq = self.seq.wrapping_add(1);
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn output_seq(&self) -> u64 {
        self.output_seq
    }

    pub fn progress(&self) -> Option<Progress> {
        self.progress
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn resize(&mut self, size: Size) {
        if size != self.size {
            if let Some(recording) = &mut self.recording {
                recording.push(fixture::Step::Resize { size });
            }
            self.size = size;
            self.marks.resize(&mut self.term, size, self.selection_from_buffer_match);
            self.scroll_region = 0..size.rows as i32;
            if let Some(found) = self.search.as_mut() {
                found.reset_index();
            }
            self.bump();
        }
    }

    pub fn set_cell_pixels(&mut self, w: u16, h: u16) {
        self.cell.store(u32::from(w) << 16 | u32::from(h), Ordering::Relaxed);
    }

    pub fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    pub fn set_focus(&mut self, focused: bool) {
        self.term.is_focused = focused;
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn set_title(&mut self, title: Option<String>) {
        self.title = title;
    }

    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    pub fn history(&self) -> usize {
        self.term.grid().history_size()
    }

    pub fn scroll(&mut self, scroll: Scroll) {
        self.marks.stop_navigation();
        let before = self.display_offset();
        self.term.scroll_display(scroll);
        if before != self.display_offset() { self.record_fixture_view(); }
        self.bump();
    }

    pub fn colors(&self) -> &alacritty_terminal::term::color::Colors {
        self.term.colors()
    }

    pub fn cursor(&self) -> Cursor {
        let content = self.term.renderable_content();
        let p = content.cursor.point;
        let line = p.line.0 + content.display_offset as i32;
        let visible = (0..self.size.rows as i32).contains(&line);
        let shape = if visible { content.cursor.shape } else { CursorShape::Hidden };
        let wide = self.term.grid()[p].flags.contains(Flags::WIDE_CHAR);
        Cursor { line: line.max(0) as usize, col: p.column.0, shape, wide }
    }

    pub fn visible(&self, mut f: impl FnMut(VisibleCell<'_>)) {
        let content = self.term.renderable_content();
        let offset = content.display_offset as i32;
        let selection = content.selection;
        for item in content.display_iter {
            let line = item.point.line.0 + offset;
            if line < 0 {
                continue;
            }
            let selected = selection.is_some_and(|s| s.contains(item.point));
            f(VisibleCell { line: line as usize, col: item.point.column.0, cell: item.cell, selected });
        }
    }

    pub fn viewport_point(&self, line: usize, col: usize) -> Point {
        let line = line.min(self.size.rows - 1) as i32 - self.display_offset() as i32;
        Point::new(Line(line), Column(col.min(self.size.cols - 1)))
    }

    pub fn start_selection(&mut self, ty: SelectionType, point: Point, side: Side) {
        self.selection_from_buffer_match = false;
        self.term.selection = Some(Selection::new(ty, point, side));
        self.bump();
    }

    pub fn update_selection(&mut self, point: Point, side: Side) {
        self.selection_from_buffer_match = false;
        if let Some(s) = self.term.selection.as_mut() {
            s.update(point, side);
            self.bump();
        }
    }

    pub fn select_all(&mut self) {
        self.selection_from_buffer_match = false;
        let mut s = Selection::new(SelectionType::Simple, Point::new(self.term.topmost_line(), Column(0)), Side::Left);
        s.update(Point::new(self.term.bottommost_line(), Column(self.size.cols - 1)), Side::Right);
        self.term.selection = Some(s);
        self.bump();
    }

    pub fn clear_selection(&mut self) {
        self.selection_from_buffer_match = false;
        if self.term.selection.take().is_some() {
            self.bump();
        }
    }

    pub fn has_selection(&self) -> bool {
        self.term.selection.as_ref().is_some_and(|s| !s.is_empty())
    }

    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string().filter(|s| !s.is_empty())
    }

    pub fn buffer_text(&self) -> String {
        self.term.bounds_to_string(Point::new(self.term.topmost_line(), Column(0)), Point::new(self.term.bottommost_line(), self.term.last_column()))
    }

    pub fn selected_cells(&self, mut visit: impl FnMut(Option<&Cell>)) {
        let Some(selection) = &self.term.selection else { return };
        let Some(range) = selection.to_range(&self.term) else { return };
        let last = self.term.last_column();
        for line in (range.start.line.0..=range.end.line.0).map(Line) {
            let row = &self.term.grid()[line];
            let mut start = if range.is_block || line == range.start.line { range.start.column.0 } else { 0 };
            let end = if range.is_block || line == range.end.line { range.end.column } else { last };
            if row[Column(start)].flags.contains(Flags::WIDE_CHAR_SPACER) {
                start = start.saturating_sub(1);
            }
            let occupied = row.line_length().0.min(end.0 + 1);
            let mut cells = &row[..][start.min(occupied)..occupied];
            if range.is_block {
                let used = cells.iter().rposition(|c| !c.c.is_whitespace() || c.zerowidth().is_some()).map_or(0, |i| i + 1);
                cells = &cells[..used];
            }
            for cell in cells {
                if !cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                    visit(Some(cell));
                }
            }
            if end == last && row[last].flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) && line < self.term.bottommost_line()
                && (line == range.end.line || range.is_block && range.start.column.0 != 0) {
                visit(Some(&self.term.grid()[line + 1i32][Column(0)]));
            }
            if line != range.end.line && (range.is_block || !row[last].flags.contains(Flags::WRAPLINE)) {
                visit(None);
            }
        }
        if selection.ty == SelectionType::Lines {
            visit(None);
        }
    }

    pub fn clear_history(&mut self) {
        if let Some(recording) = &mut self.recording {
            recording.push(fixture::Step::ClearHistory);
        }
        self.term.grid_mut().clear_history();
        if !self.mode().contains(TermMode::ALT_SCREEN) {
            self.marks.retain_history(0, self.size.rows);
        }
        if let Some(found) = self.search.as_mut() {
            found.reset_index();
        }
        self.bump();
    }

    pub fn prompt_line(&self) -> (String, usize) {
        let grid = self.term.grid();
        let point = grid.cursor.point;
        let row = &grid[point.line];
        let text: String = (0..point.column.0.min(self.size.cols)).map(|c| row[Column(c)].c).collect();
        (text, grid.history_size() + point.line.0.max(0) as usize)
    }

    pub fn tail_lines(&self, max: usize) -> Vec<String> {
        let top = self.term.topmost_line().0;
        let bottom = self.term.bottommost_line().0;
        let mut lines: Vec<String> = Vec::new();
        let mut joining = false;
        for line in top..=bottom {
            let row = &self.term.grid()[Line(line)];
            let wrapped = row[Column(self.size.cols - 1)].flags.contains(Flags::WRAPLINE);
            let text = search::row_text(&self.term, line);
            let text = if wrapped { text } else { text.trim_end().to_owned() };
            match (joining, lines.last_mut()) {
                (true, Some(last)) => last.push_str(&text),
                _ => lines.push(text),
            }
            joining = wrapped;
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        let skip = lines.len().saturating_sub(max);
        lines.split_off(skip)
    }

    pub fn screen_text(&self) -> String {
        let mut rows = vec![String::new(); self.size.rows];
        self.visible(|v| {
            if !v.cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                if let Some(row) = rows.get_mut(v.line) {
                    row.push(v.cell.c);
                }
            }
        });
        let mut out: Vec<&str> = rows.iter().map(|r| r.trim_end()).collect();
        while out.last().is_some_and(|r| r.is_empty()) {
            out.pop();
        }
        out.join("\n")
    }

    pub fn debug_dump(&self) -> String {
        use std::fmt::Write;
        let p = self.term.grid().cursor.point;
        let mut dump = format!("Grid: {}x{}\nCursor (row, column; 1-based): {}, {}\nModes: {:?}\nScrollback:\n",
            self.size.cols, self.size.rows, p.line.0 + 1, p.column.0 + 1, self.mode());
        for line in self.term.topmost_line().0.max(-200)..0 {
            writeln!(dump, "{}", search::row_text(&self.term, line).trim_end()).unwrap();
        }
        dump.push_str("Screen:\n");
        dump.push_str(&self.screen_text());
        dump
    }
}

fn config(options: &Options) -> Config {
    Config {
        scrolling_history: options.scrollback,
        default_cursor_style: CursorStyle { shape: options.cursor, blinking: options.blink },
        semantic_escape_chars: options.word_separators.clone(),
        osc52: Osc52::OnlyCopy,
        ..Config::default()
    }
}

#[cfg(test)]
#[path = "../test/terminal.rs"]
mod tests;

#[cfg(test)]
#[path = "../test/search_stream.rs"]
mod search_stream;

#[cfg(test)]
#[path = "../test/fixture.rs"]
mod fixture_tests;
