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
pub use palette::Palette;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::selection::Selection;
use alacritty_terminal::term::{Config, Osc52, Term};
use alacritty_terminal::vte::ansi::{CursorStyle, Processor, StdSyncHandler};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VtEvent {
    Write(Vec<u8>),
    Title(String),
    ResetTitle,
    Bell,
    Clipboard(String),
}

pub type Sink = Arc<dyn Fn(VtEvent) + Send + Sync>;

#[derive(Clone)]
pub struct Listener {
    sink: Sink,
    cell: Arc<AtomicU32>,
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
        (self.sink)(ev);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    pub scrollback: usize,
    pub cursor: CursorShape,
    pub blink: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self { scrollback: 25_000, cursor: CursorShape::Block, blink: true }
    }
}

pub struct Terminal {
    term: Term<Listener>,
    parser: Processor<StdSyncHandler>,
    size: Size,
    cell: Arc<AtomicU32>,
    title: Option<String>,
    seq: u64,
    sniffer: osc::Sniffer,
    cwd: Option<String>,
    search: Option<alacritty_terminal::term::search::RegexSearch>,
    found: Option<alacritty_terminal::term::search::Match>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleMatch {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub current: bool,
}

pub struct VisibleCell<'a> {
    pub line: usize,
    pub col: usize,
    pub cell: &'a Cell,
    pub selected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cursor {
    pub line: usize,
    pub col: usize,
    pub shape: CursorShape,
    pub wide: bool,
}

impl Terminal {
    pub fn new(size: Size, options: Options, sink: Sink) -> Self {
        let cell = Arc::new(AtomicU32::new(0));
        let listener = Listener { sink, cell: cell.clone() };
        Self { term: Term::new(config(options), &size, listener), parser: Processor::new(), size, cell, title: None, seq: 0, sniffer: osc::Sniffer::default(), cwd: None, search: None, found: None }
    }

    pub fn set_search(&mut self, query: &str, options: search::SearchOptions) -> Result<(), String> {
        self.found = None;
        self.search = None;
        self.bump();
        if query.is_empty() {
            return Ok(());
        }
        self.search = Some(search::compile(query, options)?);
        Ok(())
    }

    pub fn clear_search(&mut self) {
        self.search = None;
        self.found = None;
        self.bump();
    }

    pub fn search_step(&mut self, older: bool) -> bool {
        use alacritty_terminal::index::Direction;
        let Some(mut regex) = self.search.take() else { return false };
        let (origin, direction, side) = match (&self.found, older) {
            (Some(m), true) => (*m.start(), Direction::Left, Side::Left),
            (Some(m), false) => (*m.end(), Direction::Right, Side::Right),
            (None, _) => (Point::new(self.term.bottommost_line(), Column(self.size.cols - 1)), Direction::Left, Side::Right),
        };
        let origin = match (&self.found, direction) {
            (Some(_), Direction::Left) => origin.sub(&self.term, alacritty_terminal::index::Boundary::None, 1),
            (Some(_), Direction::Right) => origin.add(&self.term, alacritty_terminal::index::Boundary::None, 1),
            _ => origin,
        };
        let found = self.term.search_next(&mut regex, origin, direction, side, None);
        self.search = Some(regex);
        self.bump();
        match found {
            Some(m) => {
                self.term.scroll_to_point(*m.start());
                self.found = Some(m);
                true
            }
            None => false,
        }
    }

    pub fn visible_matches(&mut self) -> Vec<VisibleMatch> {
        use alacritty_terminal::index::Direction;
        use alacritty_terminal::term::search::RegexIter;
        let Some(mut regex) = self.search.take() else { return Vec::new() };
        let offset = self.display_offset() as i32;
        let top = Point::new(Line(-offset), Column(0));
        let bottom = Point::new(Line(self.size.rows as i32 - 1 - offset), Column(self.size.cols - 1));
        let current = self.found.clone();
        let mut out = Vec::new();
        for m in RegexIter::new(top, bottom, Direction::Right, &self.term, &mut regex).take(2000) {
            let (s, e) = (*m.start(), *m.end());
            let is_current = current.as_ref().is_some_and(|c| *c.start() == s && *c.end() == e);
            for line in s.line.0..=e.line.0 {
                let row = line + offset;
                if row < 0 || row >= self.size.rows as i32 {
                    continue;
                }
                let start = if line == s.line.0 { s.column.0 } else { 0 };
                let end = if line == e.line.0 { e.column.0 + 1 } else { self.size.cols };
                out.push(VisibleMatch { line: row as usize, start, end, current: is_current });
            }
        }
        self.search = Some(regex);
        out
    }

    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
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
        self.term.set_options(config(options));
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        let cwd = &mut self.cwd;
        self.sniffer.feed(bytes, |payload| {
            if let Some(dir) = osc::cwd(payload) {
                *cwd = Some(dir);
            }
        });
        self.parser.advance(&mut self.term, bytes);
        self.bump();
    }

    fn bump(&mut self) {
        self.seq = self.seq.wrapping_add(1);
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    pub fn size(&self) -> Size {
        self.size
    }

    pub fn resize(&mut self, size: Size) {
        if size != self.size {
            self.size = size;
            self.term.resize(size);
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
        self.term.scroll_display(scroll);
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
        self.term.selection = Some(Selection::new(ty, point, side));
        self.bump();
    }

    pub fn update_selection(&mut self, point: Point, side: Side) {
        if let Some(s) = self.term.selection.as_mut() {
            s.update(point, side);
            self.bump();
        }
    }

    pub fn select_all(&mut self) {
        let mut s = Selection::new(SelectionType::Simple, Point::new(self.term.topmost_line(), Column(0)), Side::Left);
        s.update(Point::new(self.term.bottommost_line(), Column(self.size.cols - 1)), Side::Right);
        self.term.selection = Some(s);
        self.bump();
    }

    pub fn clear_selection(&mut self) {
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

    pub fn clear_history(&mut self) {
        self.term.grid_mut().clear_history();
        self.bump();
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
}

fn config(options: Options) -> Config {
    Config {
        scrolling_history: options.scrollback,
        default_cursor_style: CursorStyle { shape: options.cursor, blinking: options.blink },
        osc52: Osc52::OnlyCopy,
        ..Config::default()
    }
}

#[cfg(test)]
#[path = "../test/terminal.rs"]
mod tests;
