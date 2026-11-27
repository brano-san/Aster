use crate::{
    Color, CommandSummary, Cursor, CursorShape, Flags, NamedColor, Options, Scroll, Size, Terminal,
};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
const MAX_STREAM_BYTES: usize = 1024 * 1024;
const MAX_STEPS: usize = 8192;
const MAX_HISTORY: usize = 100_000;
const MAX_GRID_CELLS: usize = 128 * 1024;
const MAX_BUFFER_CELLS: usize = 16 * 1024 * 1024;
const VERSION: u64 = 1;

#[derive(Clone, Copy, Debug)]
pub enum Chunking {
    Captured,
    Coalesced,
    Bytewise,
    Staggered,
}

impl Chunking {
    pub const ALL: [Self; 4] = [
        Self::Captured,
        Self::Coalesced,
        Self::Bytewise,
        Self::Staggered,
    ];
}

#[derive(Debug)]
pub enum FixtureError {
    Inactive,
    Limit,
    Version(u64),
    Invalid(&'static str),
    Json(serde_json::Error),
    Mismatch(String),
}

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inactive => f.write_str("open a recording tab to capture a fixture"),
            Self::Limit => f.write_str("terminal fixture exceeds its capture limit"),
            Self::Version(version) => write!(f, "unsupported terminal fixture version {version}"),
            Self::Invalid(field) => write!(f, "invalid terminal fixture {field}"),
            Self::Json(error) => write!(f, "invalid terminal fixture JSON: {error}"),
            Self::Mismatch(part) => write!(f, "terminal fixture mismatch at {part}"),
        }
    }
}

impl std::error::Error for FixtureError {}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    version: u64,
    initial_size: Size,
    initial_options: Options,
    steps: Vec<Step>,
    expected: Snapshot,
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Step {
    Output {
        #[serde(with = "hex")]
        data: Vec<u8>,
    },
    SavedOutput {
        #[serde(with = "hex")]
        data: Vec<u8>,
    },
    Resize {
        size: Size,
    },
    View {
        offset: usize,
    },
    Options {
        options: Options,
    },
    NewSession {
        options: Options,
    },
    ClearHistory,
}

impl Step {
    fn output(&self) -> Option<(bool, &[u8])> {
        match self {
            Self::Output { data } => Some((false, data)),
            Self::SavedOutput { data } => Some((true, data)),
            _ => None,
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(remote = "CursorShape", rename_all = "snake_case")]
pub(crate) enum Shape {
    Block,
    Underline,
    Beam,
    HollowBlock,
    Hidden,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    size: Size,
    rows: Vec<String>,
    cursor: Cursor,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    cells: Vec<CellDetails>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    commands: Vec<CommandSummary>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    integrated: bool,
    #[serde(default)]
    display_offset: usize,
}

#[derive(Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CellDetails {
    line: usize,
    col: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    flags: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    foreground: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    background: Option<Color>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    underline: Option<Color>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    combining: String,
}

impl Snapshot {
    fn new(terminal: &Terminal) -> Self {
        let size = terminal.size();
        let mut rows = vec![String::new(); size.rows];
        let mut cells = Vec::new();
        terminal.visible(|visible| {
            let crate::VisibleCell {
                line, col, cell, ..
            } = visible;
            rows[line].push(cell.c);
            let details = CellDetails {
                flags: (!cell.flags.is_empty()).then_some(cell.flags.bits()),
                foreground: (cell.fg != Color::Named(NamedColor::Foreground)).then_some(cell.fg),
                background: (cell.bg != Color::Named(NamedColor::Background)).then_some(cell.bg),
                underline: cell.underline_color(),
                combining: cell.zerowidth().into_iter().flatten().collect(),
                ..CellDetails::default()
            };
            if details != CellDetails::default() {
                cells.push(CellDetails {
                    line,
                    col,
                    ..details
                });
            }
        });
        for row in &mut rows {
            row.truncate(row.trim_end_matches(' ').len());
        }
        Self {
            size,
            rows,
            cursor: terminal.cursor(),
            cells,
            commands: terminal.command_history(),
            integrated: terminal.shell_integrated(),
            display_offset: terminal.display_offset(),
        }
    }
}

pub(crate) struct Recording {
    size: Size,
    options: Options,
    steps: Option<Vec<Step>>,
    bytes: usize,
}

impl Recording {
    pub(crate) fn push(&mut self, step: Step) {
        let Some(steps) = &mut self.steps else { return };
        let options_too_large = matches!(&step,
            Step::Options { options } | Step::NewSession { options } if options.word_separators.len() > 4096);
        if steps.len() == MAX_STEPS || options_too_large {
            self.steps = None;
        } else {
            steps.push(step);
        }
    }

    pub(crate) fn output(&mut self, bytes: &[u8], saved: bool) {
        if self.steps.is_none() || (bytes.is_empty() && !saved) {
            return;
        }
        self.bytes = self.bytes.saturating_add(bytes.len());
        if self.bytes > MAX_STREAM_BYTES {
            self.steps = None;
        } else if saved {
            self.push(Step::SavedOutput {
                data: bytes.to_vec(),
            });
        } else {
            self.push(Step::Output {
                data: bytes.to_vec(),
            });
        }
    }
}

impl Terminal {
    pub fn enable_fixture_recording(&mut self) -> bool {
        if self.seq != 0 {
            return false;
        }
        if self.recording.is_some() {
            return true;
        }
        self.recording = Some(Recording {
            size: self.size(),
            options: self.options.clone(),
            steps: Some(Vec::new()),
            bytes: 0,
        });
        true
    }

    pub fn capture_fixture(&self) -> Result<Fixture, FixtureError> {
        let recording = self.recording.as_ref().ok_or(FixtureError::Inactive)?;
        let steps = recording.steps.as_ref().ok_or(FixtureError::Limit)?;
        validate_size(self.size(), &self.options)?;
        let fixture = Fixture {
            version: VERSION,
            initial_size: recording.size,
            initial_options: recording.options.clone(),
            steps: steps.clone(),
            expected: Snapshot::new(self),
        };
        fixture.validate()?;
        Ok(fixture)
    }

    pub(crate) fn record_fixture_view(&mut self) {
        let offset = self.display_offset();
        if let Some(recording) = &mut self.recording {
            recording.push(Step::View { offset });
        }
    }
}

impl Fixture {
    pub fn from_json(bytes: &[u8]) -> Result<Self, FixtureError> {
        #[derive(Deserialize)]
        struct Header {
            version: u64,
        }
        if bytes.len() > MAX_FILE_BYTES {
            return Err(FixtureError::Limit);
        }
        let header: Header = serde_json::from_slice(bytes).map_err(FixtureError::Json)?;
        if header.version != VERSION {
            return Err(FixtureError::Version(header.version));
        }
        let fixture: Self = serde_json::from_slice(bytes).map_err(FixtureError::Json)?;
        fixture.validate()?;
        Ok(fixture)
    }

    pub fn to_json(&self) -> Result<Vec<u8>, FixtureError> {
        let bytes = serde_json::to_vec(self).map_err(FixtureError::Json)?;
        if bytes.len() > MAX_FILE_BYTES {
            return Err(FixtureError::Limit);
        }
        Ok(bytes)
    }

    pub fn verify(&self, chunking: Chunking) -> Result<(), FixtureError> {
        self.verify_and_replay(chunking).map(drop)
    }

    pub fn verify_and_replay(&self, chunking: Chunking) -> Result<Terminal, FixtureError> {
        let terminal = self.replay(chunking);
        let actual = Snapshot::new(&terminal);
        let expected = &self.expected;
        let mismatch = if actual.size != expected.size {
            "grid dimensions".into()
        } else if let Some(row) = actual
            .rows
            .iter()
            .zip(&expected.rows)
            .position(|(a, e)| a != e)
        {
            format!("row {}", row + 1)
        } else if actual.cells != expected.cells {
            "cell attributes".into()
        } else if actual.cursor != expected.cursor {
            "cursor".into()
        } else if actual.display_offset != expected.display_offset {
            "scroll position".into()
        } else if actual.commands != expected.commands || actual.integrated != expected.integrated {
            "shell integration or command history".into()
        } else {
            return Ok(terminal);
        };
        Err(FixtureError::Mismatch(mismatch))
    }

    fn validate(&self) -> Result<(), FixtureError> {
        if self.steps.len() > MAX_STEPS {
            return Err(FixtureError::Limit);
        }
        let mut size = self.initial_size;
        let mut options = &self.initial_options;
        validate_size(size, options)?;
        let mut bytes = 0;
        for step in &self.steps {
            match step {
                Step::Output { data } | Step::SavedOutput { data } => {
                    bytes += data.len();
                    if bytes > MAX_STREAM_BYTES {
                        return Err(FixtureError::Limit);
                    }
                }
                Step::Resize { size: next } => {
                    size = *next;
                    validate_size(size, options)?;
                }
                Step::Options { options: next } | Step::NewSession { options: next } => {
                    options = next;
                    validate_size(size, options)?;
                }
                Step::View { offset } if *offset > options.scrollback => {
                    return Err(FixtureError::Invalid("scroll position"))
                }
                Step::View { .. } | Step::ClearHistory => {}
            }
        }
        let Snapshot {
            size: expected_size,
            rows,
            cursor,
            cells,
            display_offset,
            ..
        } = &self.expected;
        if size != *expected_size
            || rows.len() != size.rows
            || rows.iter().any(|row| row.chars().count() > size.cols)
        {
            return Err(FixtureError::Invalid("expected grid"));
        }
        if *display_offset > options.scrollback
            || cursor.col >= size.cols
            || cursor.line >= size.rows + display_offset
            || cursor.line >= size.rows && cursor.shape != CursorShape::Hidden
        {
            return Err(FixtureError::Invalid("expected cursor or scroll position"));
        }
        if cells.iter().any(|cell| {
            cell.line >= size.rows
                || cell.col >= size.cols
                || cell
                    .flags
                    .is_some_and(|flags| Flags::from_bits(flags).is_none())
        }) || cells
            .windows(2)
            .any(|pair| (pair[0].line, pair[0].col) >= (pair[1].line, pair[1].col))
        {
            return Err(FixtureError::Invalid("expected cell attributes"));
        }
        Ok(())
    }

    fn replay(&self, chunking: Chunking) -> Terminal {
        let mut terminal = Terminal::new(
            self.initial_size,
            self.initial_options.clone(),
            Arc::new(|_| {}),
        );
        let mut index = 0;
        while let Some(step) = self.steps.get(index) {
            index += 1;
            match step {
                Step::Output { data } | Step::SavedOutput { data } => {
                    let saved = matches!(step, Step::SavedOutput { .. });
                    if matches!(chunking, Chunking::Captured) {
                        feed_chunks(&mut terminal, data, saved, chunking);
                        continue;
                    }
                    let mut group = data.clone();
                    while let Some((next_saved, data)) =
                        self.steps.get(index).and_then(Step::output)
                    {
                        if next_saved != saved {
                            break;
                        }
                        group.extend_from_slice(data);
                        index += 1;
                    }
                    feed_chunks(&mut terminal, &group, saved, chunking);
                }
                Step::Resize { size } => terminal.resize(*size),
                Step::View { offset } => terminal.scroll(Scroll::Delta(
                    *offset as i32 - terminal.display_offset() as i32,
                )),
                Step::Options { options } => terminal.set_options(options.clone()),
                Step::NewSession { options } => {
                    terminal = terminal.next_session(options.clone(), Arc::new(|_| {}), None)
                }
                Step::ClearHistory => terminal.clear_history(),
            }
        }
        terminal
    }
}

fn validate_size(size: Size, options: &Options) -> Result<(), FixtureError> {
    if !(2..=1024).contains(&size.cols)
        || !(1..=512).contains(&size.rows)
        || size.cols * size.rows > MAX_GRID_CELLS
    {
        return Err(FixtureError::Invalid("grid dimensions"));
    }
    if options.scrollback > MAX_HISTORY
        || options.word_separators.len() > 4096
        || (options.scrollback + size.rows) * size.cols > MAX_BUFFER_CELLS
    {
        return Err(FixtureError::Invalid("terminal options"));
    }
    Ok(())
}

fn feed_chunks(terminal: &mut Terminal, mut bytes: &[u8], saved: bool, chunking: Chunking) {
    let chunk_sizes: &[usize] = match chunking {
        Chunking::Captured | Chunking::Coalesced => &[usize::MAX],
        Chunking::Bytewise => &[1],
        Chunking::Staggered => &[1, 7, 17, 3, 64, 257, 4096],
    };
    for len in chunk_sizes.iter().cycle() {
        let (chunk, rest) = bytes.split_at(bytes.len().min(*len));
        if saved {
            terminal.feed_saved_output(chunk);
        } else {
            terminal.feed_replay(chunk);
        }
        if rest.is_empty() {
            break;
        }
        bytes = rest;
    }
}

mod hex {
    use super::{Deserialize, MAX_STREAM_BYTES};
    use serde::{de, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let alphabet = b"0123456789abcdef";
        let mut text = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            text.push(alphabet[(byte >> 4) as usize] as char);
            text.push(alphabet[(byte & 0x0f) as usize] as char);
        }
        serializer.serialize_str(&text)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.len() % 2 != 0 || text.len() > MAX_STREAM_BYTES * 2 {
            return Err(de::Error::custom("invalid hexadecimal output length"));
        }
        text.as_bytes()
            .chunks_exact(2)
            .map(|pair| {
                let nibble = |byte: u8| {
                    (byte as char)
                        .to_digit(16)
                        .map(|n| n as u8)
                        .ok_or_else(|| de::Error::custom("invalid hexadecimal output"))
                };
                Ok(nibble(pair[0])? << 4 | nibble(pair[1])?)
            })
            .collect()
    }
}
