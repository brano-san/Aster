use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Direction, Line, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::search::{RegexIter, RegexSearch};
use alacritty_terminal::term::Term;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchOptions {
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if "\\.+*?()|[]{}^$#&-~".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

pub fn pattern(query: &str, options: SearchOptions) -> String {
    let body = if options.regex { query.to_owned() } else { escape(query) };
    let body = if options.whole_word { format!(r"(?-u:\b)(?:{body})(?-u:\b)") } else { body };
    if options.case_sensitive {
        format!("(?-i){body}")
    } else {
        format!("(?i){body}")
    }
}

pub fn compile(query: &str, options: SearchOptions) -> Result<RegexSearch, String> {
    RegexSearch::new(&pattern(query, options)).map_err(|e| e.to_string())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Span {
    pub start: (i64, usize),
    pub end: (i64, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleMatch {
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub current: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchStatus {
    pub current: Option<usize>,
    pub count: usize,
}

const SIGNATURE_ROWS: usize = 3;
const MAX_SHIFT_SCAN: usize = 100_000;

pub struct Search {
    regex: RegexSearch,
    scrolled: i64,
    history: Vec<Span>,
    screen: Vec<Span>,
    indexed_top: Option<i64>,
    current: Option<Span>,
    dirty: bool,
}

pub(crate) fn row_text<T>(term: &Term<T>, line: i32) -> String {
    let row = &term.grid()[Line(line)];
    let mut s = String::with_capacity(term.columns());
    for col in 0..term.columns() {
        let cell = &row[Column(col)];
        if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            s.push(cell.c);
        }
    }
    s
}

pub(crate) struct Anchor {
    history: usize,
    signature: Option<(i32, Vec<String>)>,
}

impl Search {
    pub fn new(regex: RegexSearch) -> Self {
        Self { regex, scrolled: 0, history: Vec::new(), screen: Vec::new(), indexed_top: None, current: None, dirty: true }
    }

    pub(crate) fn anchor<T>(term: &Term<T>, max_history: usize) -> Anchor {
        let history = term.grid().history_size();
        let signature = (history >= max_history).then(|| {
            let rows = term.screen_lines() as i32;
            (0..rows.saturating_sub(SIGNATURE_ROWS as i32 - 1)).find_map(|top| {
                let texts: Vec<String> = (0..SIGNATURE_ROWS as i32).map(|k| row_text(term, top + k)).collect();
                texts.iter().any(|t| !t.trim().is_empty()).then_some((top, texts))
            })
        });
        Anchor { history, signature: signature.flatten() }
    }

    pub(crate) fn after_feed<T>(&mut self, term: &Term<T>, anchor: Anchor) {
        self.dirty = true;
        let history = term.grid().history_size();
        if history > anchor.history {
            self.scrolled += (history - anchor.history) as i64;
            return;
        }
        if history < anchor.history {
            self.reset_index();
            return;
        }
        let Some((top, texts)) = anchor.signature else { return };
        let topmost = -(history as i32);
        let mut shift = 0i32;
        loop {
            let at = top - shift;
            if at < topmost || shift as usize > MAX_SHIFT_SCAN {
                self.reset_index();
                return;
            }
            if (0..SIGNATURE_ROWS as i32).all(|k| row_text(term, at + k) == texts[k as usize]) {
                break;
            }
            shift += 1;
        }
        self.scrolled += shift as i64;
    }

    pub(crate) fn reset_index(&mut self) {
        self.history.clear();
        self.screen.clear();
        self.indexed_top = None;
        self.current = None;
        self.dirty = true;
    }

    fn line_of(&self, abs: i64) -> i32 {
        (abs - self.scrolled) as i32
    }

    fn scan<T>(&mut self, term: &Term<T>, from: i32, to: i32) -> Vec<Span> {
        if from > to {
            return Vec::new();
        }
        let start = Point::new(Line(from), Column(0));
        let end = Point::new(Line(to), term.last_column());
        let scrolled = self.scrolled;
        RegexIter::new(start, end, Direction::Right, term, &mut self.regex)
            .map(|m| Span { start: (m.start().line.0 as i64 + scrolled, m.start().column.0), end: (m.end().line.0 as i64 + scrolled, m.end().column.0) })
            .collect()
    }

    pub fn refresh<T>(&mut self, term: &Term<T>) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let history = term.grid().history_size() as i64;
        let oldest_line = -(history as i32);
        let screen_top = self.scrolled;
        let topmost = screen_top - history;
        let from = match self.indexed_top {
            Some(t) => t.max(topmost),
            None => {
                self.history.clear();
                topmost
            }
        };
        self.history.retain(|s| s.start.0 >= topmost && s.end.0 < from);
        if from < screen_top {
            let fresh = self.scan(term, (self.line_of(from) - 1).max(oldest_line), -1);
            let known = self.history.last().copied();
            self.history.extend(fresh.into_iter().filter(|s| s.end.0 >= from && s.end.0 < screen_top && known.is_none_or(|k| s.start > k.start)));
        }
        self.indexed_top = Some(screen_top);
        let rows = term.screen_lines() as i32;
        let screen = self.scan(term, (-1).max(oldest_line), rows - 1);
        self.screen = screen.into_iter().filter(|s| s.end.0 >= screen_top).collect();
        if let Some(c) = self.current {
            if !self.all().any(|s| s == c) {
                self.current = None;
            }
        }
    }

    fn all(&self) -> impl Iterator<Item = Span> + '_ {
        self.history.iter().chain(self.screen.iter()).copied()
    }

    pub fn status(&self) -> SearchStatus {
        let count = self.history.len() + self.screen.len();
        let current = self.current.and_then(|c| self.all().position(|s| s == c)).map(|i| i + 1);
        SearchStatus { current, count }
    }

    pub fn step(&mut self, older: bool) -> Option<Point> {
        let count = self.history.len() + self.screen.len();
        if count == 0 {
            self.current = None;
            return None;
        }
        let all: Vec<Span> = self.all().collect();
        let next = match (self.current, older) {
            (None, _) => all[count - 1],
            (Some(c), true) => all.iter().rev().find(|s| s.start < c.start).copied().unwrap_or(all[count - 1]),
            (Some(c), false) => all.iter().find(|s| s.start > c.start).copied().unwrap_or(all[0]),
        };
        self.current = Some(next);
        Some(Point::new(Line(self.line_of(next.start.0)), Column(next.start.1)))
    }

    pub fn visible(&self, display_offset: usize, rows: usize, cols: usize) -> Vec<VisibleMatch> {
        let top = self.scrolled - display_offset as i64;
        let bottom = top + rows as i64 - 1;
        let mut out = Vec::new();
        for s in self.all().filter(|s| s.end.0 >= top && s.start.0 <= bottom) {
            let current = self.current == Some(s);
            for abs in s.start.0.max(top)..=s.end.0.min(bottom) {
                let start = if abs == s.start.0 { s.start.1 } else { 0 };
                let end = if abs == s.end.0 { s.end.1 + 1 } else { cols };
                out.push(VisibleMatch { line: (abs - top) as usize, start, end, current });
            }
        }
        out
    }

    pub fn marks(&self, history: usize, rows: usize) -> Vec<(f32, bool)> {
        let topmost = self.scrolled - history as i64;
        let total = (history + rows).max(1) as f32;
        self.all().map(|s| (((s.start.0 - topmost) as f32 + 0.5) / total, self.current == Some(s))).collect()
    }

    pub fn current_point(&self) -> Option<Point> {
        self.current.map(|c| Point::new(Line(self.line_of(c.start.0)), Column(c.start.1)))
    }
}
