use crate::osc::CommandMark;
use crate::{Cell, Column, Flags, Line, Listener, Point, Scroll, SelectionType, Side, Size, TermMode, Terminal};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::grid::Cursor;
use alacritty_terminal::selection::Selection;
use alacritty_terminal::term::cell::{CellExtra, LineLength};
use alacritty_terminal::term::Term;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write;
use std::ops::Range;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    row: i64,
    column: usize,
}

const COMMANDS_PER_POSITION: usize = 16;
const COMMAND_CHARS: usize = 480;
const PREVIEW_CHARS: usize = 120;
const PREVIEW_ROWS: i32 = 8;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommandSummary {
    pub id: u64,
    pub command: String,
    pub preview: String,
    pub status: Option<i32>,
}

struct Pending {
    output: Option<Position>,
    command: String,
    saved_id: Option<u64>,
}

struct Command {
    range: Range<Position>,
    command: String,
    preview: String,
    status: Option<i32>,
}

#[derive(Default)]
pub(crate) struct Marks {
    base: i64,
    prompts: BTreeSet<Position>,
    navigation: Option<Position>,
    input: Option<Position>,
    running: Option<Pending>,
    commands: BTreeMap<u64, Command>,
    commands_by_start: BTreeSet<(Position, u64)>,
    last_id: u64,
    alt_saved_cursor: Option<Cursor<Cell>>,
    pub(crate) integrated: bool,
}

impl Marks {
    pub(crate) fn new_session(&mut self) -> Self {
        let mut previous = std::mem::take(self);
        previous.integrated = false;
        previous.running = None;
        previous.input = None;
        previous.navigation = None;
        previous.alt_saved_cursor = None;
        previous
    }

    pub(crate) fn reset(&mut self) {
        *self = Self { last_id: self.last_id, ..Self::default() };
    }

    pub(crate) fn event(&mut self, term: &Term<Listener>, mark: CommandMark) -> Option<Option<i32>> {
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return None;
        }
        let cursor = &term.grid().cursor;
        let position = Position {
            row: self.base + i64::from(cursor.point.line.0),
            column: cursor.point.column.0 + usize::from(cursor.input_needs_wrap),
        };
        self.integrated = true;
        match mark {
            CommandMark::Prompt => {
                self.prompts.insert(position);
                self.input = None;
                if self.running.as_ref().is_none_or(|running| running.saved_id.is_none()) {
                    self.running = None;
                }
            }
            CommandMark::Input => self.input = Some(position),
            CommandMark::Output(saved) if self.running.is_none() => {
                let input = self.input.take();
                let (saved_id, command) = if let Some((id, command)) = saved {
                    (Some(id), command)
                } else {
                    let command = input.filter(|start| *start < position)
                        .and_then(|start| self.bounds(term, &(start..position)))
                        .map_or_else(String::new, |(start, end)| range_text(term, start, end, COMMAND_CHARS).trim().to_owned());
                    (None, command)
                };
                self.running = Some(Pending { output: Some(position), command, saved_id });
            }
            CommandMark::Done(status) => {
                let running = self.running.take()?;
                let Some(id) = running.saved_id.filter(|&id| id > self.last_id)
                    .or_else(|| self.last_id.checked_add(1)) else { return Some(status) };
                self.last_id = id;
                if let Some(start) = running.output.filter(|start| *start <= position) {
                    let range = start..position;
                    let preview = self.bounds(term, &range).map_or_else(String::new, |(first, last)| {
                        let end = last.min(Point::new(first.line + PREVIEW_ROWS, Column(0)));
                        let output = range_text(term, first, end, PREVIEW_CHARS);
                        output.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or_default().to_owned()
                    });
                    self.commands.insert(id, Command { command: running.command, preview, status, range });
                    self.commands_by_start.insert((start, id));
                    let mut at_start = self.commands_by_start.range((start, 0)..=(start, u64::MAX));
                    let first = at_start.next().copied().filter(|_| at_start.nth(COMMANDS_PER_POSITION - 1).is_some());
                    if let Some(first) = first {
                        self.commands_by_start.remove(&first);
                        self.commands.remove(&first.1);
                    }
                    self.limit_commands(term.history_size(), term.screen_lines());
                }
                return Some(status);
            }
            CommandMark::Output(_) => {}
        }
        None
    }

    fn limit_commands(&mut self, history: usize, rows: usize) {
        let limit = history.saturating_add(rows).max(COMMANDS_PER_POSITION);
        while self.commands.len() > limit {
            let (id, command) = self.commands.pop_first().unwrap();
            self.commands_by_start.remove(&(command.range.start, id));
        }
    }

    fn remap(&mut self, mut position: impl FnMut(Position) -> Option<Position>) {
        self.prompts = std::mem::take(&mut self.prompts).into_iter().filter_map(&mut position).collect();
        self.navigation = self.navigation.and_then(&mut position);
        self.input = self.input.and_then(&mut position);
        if let Some(running) = self.running.as_mut() {
            running.output = running.output.and_then(&mut position);
        }
        self.commands.retain(|_, command| {
            let Some(start) = position(command.range.start) else { return false };
            let Some(end) = position(command.range.end) else { return false };
            command.range = start..end;
            start <= end
        });
        self.commands_by_start = self.commands.iter().map(|(&id, command)| (command.range.start, id)).collect();
    }

    pub(crate) fn retain_history(&mut self, history: usize, rows: usize) {
        let oldest = self.base - history as i64;
        while self.prompts.first().is_some_and(|p| p.row < oldest) {
            self.prompts.pop_first();
        }
        self.navigation = self.navigation.filter(|p| p.row >= oldest);
        self.input = self.input.filter(|p| p.row >= oldest);
        if let Some(running) = self.running.as_mut() {
            running.output = running.output.filter(|p| p.row >= oldest);
        }
        while self.commands_by_start.first().is_some_and(|(start, _)| start.row < oldest) {
            let (_, id) = self.commands_by_start.pop_first().unwrap();
            self.commands.remove(&id);
        }
        self.limit_commands(history, rows);
    }

    pub(crate) fn shift(&mut self, region: Range<i32>, delta: i32, history: usize, rows: i32) {
        if delta < 0 && region.start == 0 {
            let fixed = self.base + i64::from(region.end);
            self.base -= i64::from(delta);
            if region.end < rows {
                self.remap(|mut p| {
                    if p.row >= fixed {
                        p.row -= i64::from(delta);
                    }
                    Some(p)
                });
            }
            self.retain_history(history, rows as usize);
        } else if delta != 0 {
            let from = self.base + i64::from(region.start);
            let to = self.base + i64::from(region.end);
            self.remap(|mut p| {
                if (from..to).contains(&p.row) {
                    p.row += i64::from(delta);
                    if !(from..to).contains(&p.row) { return None; }
                }
                Some(p)
            });
        }
    }

    pub(crate) fn positions_present(&self) -> bool {
        !self.prompts.is_empty() || self.input.is_some() || self.running.as_ref().is_some_and(|running| running.output.is_some())
            || !self.commands.is_empty()
    }

    pub(crate) fn stop_navigation(&mut self) {
        self.navigation = None;
    }

    pub(crate) fn leave_alt(&mut self, term: &mut Term<Listener>) {
        if let Some(saved_cursor) = self.alt_saved_cursor.take() {
            term.grid_mut().saved_cursor = saved_cursor;
        }
        self.retain_history(term.history_size(), term.screen_lines());
    }

    pub(crate) fn resize(&mut self, term: &mut Term<Listener>, size: Size, preserve_selection: bool) {
        let alt = term.mode().contains(TermMode::ALT_SCREEN);
        let selection = if preserve_selection && !alt && size.cols != term.columns() {
            term.selection.as_ref().and_then(|selection| selection.to_range(term)).map(|range| {
                let position = |point: Point| Position { row: self.base + i64::from(point.line.0), column: point.column.0 };
                [position(range.start), position(range.end)]
            })
        } else {
            None
        };
        if !self.positions_present() && selection.is_none() {
            term.resize(size);
            return;
        }
        let mut alt_grid = None;
        if alt {
            let size = (term.screen_lines(), term.columns());
            alt_grid = Some(std::mem::replace(term.grid_mut(), alacritty_terminal::grid::Grid::new(size.0, size.1, 0)));
            term.swap_alt();
            self.leave_alt(term);
        }
        let mut cells = HashMap::new();
        let mut anchors = HashMap::new();
        let points = self.prompts.iter().copied().chain(self.navigation).chain(self.input)
            .chain(self.running.as_ref().and_then(|running| running.output))
            .chain(self.commands.values().flat_map(|command| [command.range.start, command.range.end]))
            .chain(selection.into_iter().flatten());
        let grid = term.grid_mut();
        let oldest = -(grid.history_size() as i32);
        let bottom = grid.screen_lines() as i32;
        for p in points {
            let line = Line((p.row - self.base) as i32);
            if !(oldest..bottom).contains(&line.0) { continue; }
            let row = &grid[line];
            let end = row.line_length().0;
            let mut col = p.column.min(end.saturating_sub(1));
            let mut after = p.column.saturating_sub(col);
            let flags = row[Column(col)].flags;
            if flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) && line.0 < grid.screen_lines() as i32 - 1 {
                col = 0;
                after = 0;
                cells.entry((line.0 + 1, col)).or_insert_with(Vec::new).push((p, after));
                continue;
            }
            if flags.contains(Flags::WIDE_CHAR_SPACER) {
                col -= 1;
                after += 1;
            }
            cells.entry((line.0, col)).or_insert_with(Vec::new).push((p, after));
        }
        let mut markers = HashMap::new();
        for ((line, col), positions) in cells {
            let cell = &mut grid[Line(line)][Column(col)];
            let original = cell.extra.clone();
            cell.push_zerowidth('\0');
            let key = cell.extra.as_ref().map(Arc::as_ptr).unwrap();
            markers.insert(key, (original, positions));
        }
        term.resize(size);
        let grid = term.grid_mut();
        let oldest = -(grid.history_size() as i32);
        let rows = grid.screen_lines() as i32;
        for line in (oldest..rows).map(Line) {
            if markers.is_empty() { break; }
            for col in 0..size.cols {
                let key = grid[line][Column(col)].extra.as_ref().map(Arc::<CellExtra>::as_ptr);
                let Some((original, positions)) = key.and_then(|key| markers.remove(&key)) else { continue };
                grid[line][Column(col)].extra = original;
                for (old, after) in positions {
                    anchors.insert(old, Position { row: self.base + i64::from(line.0), column: (col + after).min(size.cols) });
                }
            }
        }
        self.remap(|p| anchors.get(&p).copied());
        self.limit_commands(term.history_size(), term.screen_lines());
        if let Some((start, end)) = selection.and_then(|[start, end]| {
            let start = *anchors.get(&start)?;
            let end = *anchors.get(&end)?;
            self.bounds(term, &(start..end)).filter(|(start, end)| start <= end && end.column.0 < size.cols)
        }) {
            let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
            selection.update(end, Side::Right);
            term.selection = Some(selection);
        }
        if let Some(mut alt_grid) = alt_grid {
            alt_grid.resize(false, size.rows, size.cols);
            self.alt_saved_cursor = Some(term.grid().saved_cursor.clone());
            term.swap_alt();
            *term.grid_mut() = alt_grid;
        }
    }

    fn bounds(&self, term: &Term<Listener>, range: &Range<Position>) -> Option<(Point, Point)> {
        let start = Point::new(Line(i32::try_from(range.start.row - self.base).ok()?), Column(range.start.column));
        let end = Point::new(Line(i32::try_from(range.end.row - self.base).ok()?), Column(range.end.column));
        (start.line >= term.topmost_line() && end.line <= term.bottommost_line()
            && start.column.0 <= term.columns() && end.column.0 <= term.columns()).then_some((start, end))
    }
}

fn range_text(term: &Term<Listener>, start: Point, end: Point, mut remaining: usize) -> String {
    let grid = term.grid();
    let mut text = String::new();
    for y in start.line.0..=end.line.0 {
        let row = &grid[Line(y)];
        let from = if y == start.line.0 { start.column.0 } else { 0 };
        let to = if y == end.line.0 { end.column.0 } else { row.line_length().0 };
        for cell in &row[..][from.min(to)..to] {
            if remaining == 0 {
                return text;
            }
            if !cell.flags.intersects(Flags::LEADING_WIDE_CHAR_SPACER | Flags::WIDE_CHAR_SPACER) {
                text.push(cell.c);
                remaining -= 1;
                for combining in cell.zerowidth().into_iter().flatten().take(remaining) {
                    text.push(*combining);
                    remaining -= 1;
                }
            }
        }
        if y != end.line.0 && !row[term.last_column()].flags.contains(Flags::WRAPLINE) && remaining > 0 {
            text.push('\n');
            remaining -= 1;
        }
    }
    text
}

impl Terminal {
    pub fn shell_integrated(&self) -> bool {
        self.marks.integrated
    }

    pub fn shell_command_running(&self) -> bool {
        self.marks.running.is_some()
    }

    pub fn jump_prompt(&mut self, older: bool) -> bool {
        if self.mode().contains(TermMode::ALT_SCREEN) {
            return false;
        }
        let anchor = self.marks.navigation.unwrap_or_else(|| {
            if self.display_offset() == 0 {
                let cursor = self.term.grid().cursor.point;
                let row = self.marks.base + i64::from(cursor.line.0);
                self.marks.prompts.iter().rev().find(|p| (self.marks.base..=row).contains(&p.row)).copied()
                    .unwrap_or(Position { row: self.marks.base, column: 0 })
            } else {
                Position { row: self.marks.base - self.display_offset() as i64, column: 0 }
            }
        });
        let target = if older {
            self.marks.prompts.range(..anchor).next_back()
        } else {
            self.marks.prompts.range((std::ops::Bound::Excluded(anchor), std::ops::Bound::Unbounded)).next()
        };
        let Some(&target) = target else { return false };
        self.marks.navigation = Some(target);
        let target_offset = (self.marks.base - target.row).max(0) as i32;
        self.term.scroll_display(Scroll::Delta(target_offset - self.display_offset() as i32));
        self.record_fixture_view();
        self.bump();
        true
    }

    pub fn latest_command_output(&self) -> Option<String> {
        self.command_output(self.marks.last_id).filter(|text| !text.is_empty())
    }

    fn command(&self, id: u64) -> Option<&Command> {
        if self.mode().contains(TermMode::ALT_SCREEN) { return None; }
        self.marks.commands.get(&id)
    }

    pub fn command_history(&self) -> Vec<CommandSummary> {
        if self.mode().contains(TermMode::ALT_SCREEN) { return Vec::new(); }
        self.marks.commands.iter().rev().filter_map(|(&id, command)| {
            self.marks.bounds(&self.term, &command.range)?;
            Some(CommandSummary { id, command: command.command.clone(), preview: command.preview.clone(), status: command.status })
        }).collect()
    }

    pub fn command_bounds(&self, id: u64) -> Option<(Point, Point)> {
        self.marks.bounds(&self.term, &self.command(id)?.range)
    }

    pub fn command_output(&self, id: u64) -> Option<String> {
        let (start, end) = self.command_bounds(id)?;
        Some(range_text(&self.term, start, end, usize::MAX))
    }

    pub fn reveal_command(&mut self, id: u64) -> bool {
        let Some((start, _)) = self.command_bounds(id) else { return false };
        let target = (-start.line.0).max(0);
        self.scroll(Scroll::Delta(target - self.display_offset() as i32));
        true
    }

    pub fn saved_tail(&self, max: usize) -> Vec<String> {
        if max == 0 { return Vec::new(); }
        if !self.marks.positions_present() || self.mode().contains(TermMode::ALT_SCREEN) {
            return self.tail_lines(max);
        }
        let grid = self.term.grid();
        let is_wrapped = |line| grid[Line(line)][self.term.last_column()].flags.contains(Flags::WRAPLINE);
        let oldest = self.term.topmost_line().0;
        let newest_marker = self.marks.prompts.last().map(|prompt| prompt.row).into_iter()
            .chain(self.marks.commands.values().map(|command| command.range.end.row)).max();
        let Some(bottom) = (oldest..=self.term.bottommost_line().0).rev().find(|&line| {
            grid[Line(line)].line_length().0 > 0 || newest_marker == Some(self.marks.base + i64::from(line))
        }) else { return Vec::new() };
        let first = (oldest..bottom).rev().filter(|&line| !is_wrapped(line)).nth(max - 1).map_or(oldest, |line| line + 1);
        let visible = (self.marks.base + i64::from(first))..=(self.marks.base + i64::from(bottom));
        let mut markers: Vec<_> = self.marks.prompts.iter().copied().filter(|p| visible.contains(&p.row))
            .map(|p| (p, "\x1b]133;A\x07".to_owned())).collect();
        for (&id, command) in &self.marks.commands {
            if !visible.contains(&command.range.start.row) || !visible.contains(&command.range.end.row) { continue; }
            let mut start = format!("\x1b]133;C;aster={id},");
            for byte in command.command.bytes() {
                write!(start, "{byte:02x}").unwrap();
            }
            start.push('\x07');
            let status = command.status.map_or_else(String::new, |status| format!(";{status}"));
            let end = format!("\x1b]133;D{status}\x07");
            markers.extend([(command.range.start, start), (command.range.end, end)]);
        }
        markers.sort_by_key(|(p, _)| *p);
        let mut markers = markers.into_iter().peekable();
        let mut lines = Vec::new();
        let mut logical = String::new();
        for line in first..=bottom {
            let row = &grid[Line(line)];
            let width = row.line_length().0;
            let absolute = self.marks.base + i64::from(line);
            let mut col = 0;
            while let Some((position, marker)) = markers.next_if(|(p, _)| p.row == absolute) {
                for cell in &row[..][col..position.column] {
                    append_cell(&mut logical, cell);
                }
                logical.push_str(&marker);
                col = position.column;
            }
            for cell in &row[..][col..width.max(col)] {
                append_cell(&mut logical, cell);
            }
            if !is_wrapped(line) {
                lines.push(std::mem::take(&mut logical));
            }
        }
        if !logical.is_empty() { lines.push(logical); }
        lines
    }

    pub fn feed_saved_output(&mut self, bytes: &[u8]) {
        self.feed_replay_inner(bytes, true);
        self.marks.integrated = false;
    }
}

fn append_cell(text: &mut String, cell: &Cell) {
    if !cell.flags.intersects(Flags::LEADING_WIDE_CHAR_SPACER | Flags::WIDE_CHAR_SPACER) {
        text.push(cell.c);
        text.extend(cell.zerowidth().into_iter().flatten());
    }
}
