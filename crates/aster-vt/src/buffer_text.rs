use crate::{Column, Flags, Line, Point, Scroll, SelectionType, Side, Terminal};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Direction;
use alacritty_terminal::term::search::{RegexIter, RegexSearch};
use std::ops::Range;

#[derive(Default)]
pub struct VisibleLine {
    pub text: String,
    pub cells: Vec<(usize, usize)>,
}

#[derive(Clone)]
pub struct BufferMatch {
    pub line: i32,
    pub first: usize,
    pub end_line: i32,
    pub last: usize,
    pub history: usize,
    pub columns: usize,
    pub text: String,
    pub preview: Option<MatchPreview>,
    end_text: Option<String>,
}

#[derive(Clone)]
pub struct MatchPreview {
    pub text: String,
    pub highlight: Option<Range<usize>>,
}

impl Terminal {
    pub fn visible_hyperlinks(&self) -> Vec<(usize, usize, String)> {
        let mut links = Vec::new();
        let mut last = None;
        self.visible(|cell| {
            let hyperlink = cell.cell.hyperlink();
            if hyperlink != last {
                if let Some(link) = &hyperlink {
                    links.push((cell.line, cell.col, link.uri().to_owned()));
                }
                last = hyperlink;
            }
        });
        links
    }

    pub fn visible_logical_lines(&self) -> Vec<VisibleLine> {
        let mut lines = Vec::new();
        let mut logical = VisibleLine::default();
        for line in 0..self.size.rows {
            let row = &self.term.grid()[self.viewport_point(line, 0).line];
            let wraps = row[Column(self.size.cols - 1)].flags.contains(Flags::WRAPLINE);
            for col in 0..self.size.cols {
                let cell = &row[Column(col)];
                if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                    continue;
                }
                logical.text.push(cell.c);
                logical.cells.push((line, col));
                if let Some(combining) = cell.zerowidth() {
                    logical.text.extend(combining);
                    logical.cells.extend(std::iter::repeat_n((line, col), combining.len()));
                }
            }
            if !wraps || line + 1 == self.size.rows {
                lines.push(std::mem::take(&mut logical));
            }
        }
        lines
    }

    pub fn buffer_chunk_start(&self, newest: i32, rows: i32, extra_rows: i32) -> i32 {
        let oldest = self.term.topmost_line().0;
        let newest = newest.min(self.term.bottommost_line().0);
        let mut first = (newest - rows + 1).max(oldest);
        let minimum = (first - extra_rows).max(oldest);
        let grid = self.term.grid();
        while first > minimum && grid[Line(first - 1)][self.term.last_column()].flags.contains(Flags::WRAPLINE) {
            first -= 1;
        }
        first
    }

    pub fn buffer_matches(&self, regex: &mut RegexSearch, from: i32, to: i32, limit: usize, scope: Option<(Point, Point)>) -> Vec<BufferMatch> {
        let from = from.max(self.term.topmost_line().0);
        let to = to.min(self.term.bottommost_line().0);
        if from > to || limit == 0 {
            return Vec::new();
        }
        let grid = self.term.grid();
        let mut start = Point::new(Line(from), Column(0));
        let mut end = Point::new(Line(to), self.term.last_column());
        if let Some((first, last)) = scope {
            let Some((first, last)) = self.inclusive_span(first, last) else { return Vec::new() };
            start = start.max(first);
            end = end.min(last);
            if start > end { return Vec::new() }
        }
        let mut last_line = None;
        RegexIter::new(start, end, Direction::Right, &self.term, regex)
            .filter(|hit| *hit.start() >= start && *hit.end() <= end)
            .filter_map(|hit| {
                let line = hit.start().line.0;
                if scope.is_none() && last_line.replace(line) == Some(line) {
                    return None;
                }
                let end_line = hit.end().line.0;
                Some(BufferMatch {
                    line,
                    first: hit.start().column.0,
                    end_line,
                    last: hit.end().column.0,
                    history: grid.history_size(),
                    columns: self.size.cols,
                    text: crate::search::row_text(&self.term, line).trim_end().to_owned(),
                    preview: scope.map(|_| self.match_preview(hit.start(), hit.end(), start, end)),
                    end_text: (line != end_line).then(|| crate::search::row_text(&self.term, end_line).trim_end().to_owned()),
                })
            })
            .take(limit)
            .collect()
    }

    fn inclusive_span(&self, mut start: Point, mut end: Point) -> Option<(Point, Point)> {
        if start.column.0 >= self.size.cols {
            start = Point::new(start.line + 1, Column(0));
        }
        if end.column.0 == 0 {
            end = Point::new(end.line - 1, self.term.last_column());
        } else {
            end.column = Column(end.column.0.min(self.size.cols) - 1);
        }
        (start <= end).then_some((start, end))
    }

    fn match_preview(&self, hit_start: &Point, hit_end: &Point, from: Point, to: Point) -> MatchPreview {
        let line = hit_start.line;
        let first = if line == from.line { from.column.0 } else { 0 };
        let last = if line == to.line { to.column.0 + 1 } else { self.size.cols };
        let matched_last = if line == hit_end.line { hit_end.column.0 } else { self.size.cols - 1 };
        let context = hit_start.column.0.saturating_sub(32).max(first);
        let row = &self.term.grid()[line];
        let mut preview = if context > first { "…".to_owned() } else { String::new() };
        let mut highlight: Option<Range<usize>> = None;
        for col in context..(context + 128).min(last) {
            let cell = &row[Column(col)];
            if !cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                let byte_start = preview.len();
                preview.push(cell.c);
                preview.extend(cell.zerowidth().into_iter().flatten());
                if (hit_start.column.0..=matched_last).contains(&col) {
                    highlight.get_or_insert(byte_start..byte_start).end = preview.len();
                }
            }
        }
        let end = preview.trim_end().len().max(highlight.as_ref().map_or(0, |range| range.end));
        preview.truncate(end);
        MatchPreview { text: preview, highlight }
    }

    pub fn reveal_buffer_match(&mut self, hit: &BufferMatch, command: Option<u64>) -> bool {
        if self.size.cols != hit.columns {
            return false;
        }
        let scope = match command {
            Some(command) => {
                let Some((start, end)) = self.command_bounds(command).and_then(|(start, end)| self.inclusive_span(start, end)) else { return false };
                Some((start, end))
            }
            None => None,
        };
        let history = self.history();
        let top = self.term.topmost_line().0;
        let bottom = self.term.bottommost_line().0;
        let line = hit.line.saturating_sub(history.saturating_sub(hit.history) as i32);
        let height = hit.end_line - hit.line;
        let matches = |at| {
            (top..=bottom).contains(&at)
                && at + height <= bottom
                && scope.is_none_or(|(start, end)| {
                    start <= Point::new(Line(at), Column(hit.first)) && Point::new(Line(at + height), Column(hit.last)) <= end
                })
                && crate::search::row_text(&self.term, at).trim_end() == hit.text
                && hit.end_text.as_ref().is_none_or(|text| crate::search::row_text(&self.term, at + height).trim_end() == text)
        };
        let Some(line) = (0..=32).flat_map(|delta| [line - delta, line + delta]).find(|&at| matches(at)) else {
            return false;
        };
        let offset = (self.size.rows as i32 / 2 - line).clamp(0, history as i32);
        self.scroll(Scroll::Delta(offset - self.display_offset() as i32));
        self.start_selection(SelectionType::Simple, Point::new(Line(line), Column(hit.first)), Side::Left);
        self.update_selection(Point::new(Line(line + height), Column(hit.last)), Side::Right);
        self.selection_from_buffer_match = true;
        true
    }
}
