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
pub use search::{SearchStatus, VisibleMatch};

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
    search: Option<search::Search>,
    scrollback: usize,
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
        Self { term: Term::new(config(options), &size, listener), parser: Processor::new(), size, cell, title: None, seq: 0, sniffer: osc::Sniffer::default(), cwd: None, search: None, scrollback: options.scrollback }
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
        self.scrollback = options.scrollback;
        self.term.set_options(config(options));
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        let anchor = self.search.as_ref().map(|_| search::Search::anchor(&self.term, self.scrollback));
        let cwd = &mut self.cwd;
        self.sniffer.feed(bytes, |payload| {
            if let Some(dir) = osc::cwd(payload) {
                *cwd = Some(dir);
            }
        });
        self.parser.advance(&mut self.term, bytes);
        if let (Some(found), Some(anchor)) = (self.search.as_mut(), anchor) {
            found.after_feed(&self.term, anchor);
        }
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

#[cfg(test)]
#[path = "../test/search_stream.rs"]
mod search_stream;
