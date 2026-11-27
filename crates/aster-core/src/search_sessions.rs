use crate::app::{App, Effect, Shared};
use crate::i18n::t;
use crate::overlay::{OverlayKind, Pick};
use crate::panes::PaneId;
use aster_vt::buffer_text::BufferMatch;
use aster_vt::search::SearchOptions;
use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const DEBOUNCE: Duration = Duration::from_millis(100);
const MAX_PANES: usize = 128;
const MAX_ROWS_PER_PANE: i32 = 25_000;
const MAX_ROWS: i32 = 125_000;
const MAX_RESULTS: usize = 160;
pub(crate) const MAX_QUERY_CHARS: usize = 256;

struct Source {
    pane: PaneId,
    buffer: SearchBuffer,
    title: String,
    command: Option<u64>,
    next_line: Option<i32>,
    rows: i32,
}

enum SearchBuffer {
    Live(Shared),
    Saved { terminal: Shared, history: Arc<[String]> },
    Replay(Shared),
}

impl SearchBuffer {
    fn findable(pane: &crate::app::Pane) -> Self {
        let terminal = pane.terminal.clone();
        match pane.restore_buffer.as_ref() {
            Some(history) => Self::Saved { terminal, history: history.clone() },
            None => Self::Live(terminal),
        }
    }

    fn prepare(&mut self) {
        let Self::Saved { terminal, history } = self else { return };
        let replay = {
            let live = terminal.lock();
            (live.output_seq() == 0).then(|| live.blank_for_replay(live.size()))
        };
        if let Some(mut replay) = replay {
            replay.feed_saved_output(&crate::session::restore_history_text(history));
            *self = Self::Replay(Arc::new(aster_vt::FairMutex::new(replay)));
        } else {
            *self = Self::Live(terminal.clone());
        }
    }
}

struct Hit {
    pane: PaneId,
    title: String,
    command: Option<u64>,
    found: BufferMatch,
}

pub struct Request {
    id: u64,
    query: String,
    options: SearchOptions,
    cancelled: Arc<AtomicBool>,
    sources: VecDeque<Source>,
}

pub struct Reply {
    id: u64,
    hits: Result<Vec<Hit>, SearchError>,
}

enum SearchError {
    Regex,
    CommandExpired,
}

impl Request {
    pub fn run(mut self) -> Option<Reply> {
        let mut regex = match aster_vt::search::compile(&self.query, self.options) {
            Ok(regex) => regex,
            Err(_) => return Some(Reply { id: self.id, hits: Err(SearchError::Regex) }),
        };
        let mut hits = Vec::new();
        let mut remaining = MAX_ROWS;
        while let Some(mut source) = self.sources.pop_front() {
            if self.cancelled.load(Ordering::Relaxed) {
                return None;
            }
            source.buffer.prepare();
            let terminal = match &source.buffer {
                SearchBuffer::Live(terminal) | SearchBuffer::Replay(terminal) => terminal.lock(),
                SearchBuffer::Saved { .. } => unreachable!(),
            };
            let scope = if let Some(command) = source.command {
                let Some(bounds) = terminal.command_bounds(command) else {
                    return Some(Reply { id: self.id, hits: Err(SearchError::CommandExpired) });
                };
                Some(bounds)
            } else {
                None
            };
            let bottom = scope.map_or(terminal.size().rows as i32 - 1, |(_, end)| end.line.0 - i32::from(end.column.0 == 0));
            let oldest = scope.map_or(-(terminal.history() as i32), |(start, _)| start.line.0);
            let newest = source.next_line.unwrap_or(bottom).min(bottom);
            let chunk = (8192 / terminal.size().cols.max(1)).clamp(1, 128) as i32;
            let allowance = remaining.min(MAX_ROWS_PER_PANE - source.rows);
            if newest < oldest || allowance == 0 {
                continue;
            }
            let rows = chunk.min(allowance);
            let extra = ((2 * MAX_QUERY_CHARS) / terminal.size().cols.max(1)).max(chunk as usize).min(192) as i32;
            let first = terminal.buffer_chunk_start(newest, rows, extra.min(allowance - rows)).max(oldest);
            let found = terminal.buffer_matches(&mut regex, first, newest, MAX_RESULTS - hits.len(), scope);
            drop(terminal);
            hits.extend(found.into_iter().rev().map(|found| Hit { pane: source.pane, title: source.title.clone(), command: source.command, found }));
            remaining -= newest - first + 1;
            source.rows += newest - first + 1;
            if remaining == 0 || hits.len() == MAX_RESULTS {
                break;
            }
            if first > oldest && source.rows < MAX_ROWS_PER_PANE {
                source.next_line = Some(first - 1);
                if matches!(&source.buffer, SearchBuffer::Replay(_)) {
                    self.sources.push_front(source);
                } else {
                    self.sources.push_back(source);
                }
            }
        }
        (!self.cancelled.load(Ordering::Relaxed)).then_some(Reply { id: self.id, hits: Ok(hits) })
    }
}

#[derive(Default)]
pub(crate) struct SessionSearch {
    id: u64,
    query: String,
    pub(crate) due: Option<Instant>,
    cancelled: Option<Arc<AtomicBool>>,
    hits: Vec<Hit>,
    pub(crate) status: Option<&'static str>,
}

impl Drop for SessionSearch {
    fn drop(&mut self) {
        if let Some(cancelled) = self.cancelled.take() {
            cancelled.store(true, Ordering::Relaxed);
        }
    }
}

impl App {
    pub(crate) fn open_session_search(&mut self) {
        self.open_output_search(OverlayKind::SessionSearch);
    }

    pub(crate) fn open_output_search(&mut self, kind: OverlayKind) {
        self.close_search();
        self.session_search = SessionSearch::default();
        self.open_overlay(kind);
        self.session_search.status = Some(search_prompt(kind));
    }

    pub(crate) fn session_search_edited(&mut self) {
        let Some(overlay) = self.overlay.as_mut().filter(|overlay| overlay.kind.searches_output()) else { return };
        let kind = overlay.kind;
        let query = overlay.query.trim().to_owned();
        if query == self.session_search.query {
            return;
        }
        overlay.replace_results(Vec::new());
        let mut search = SessionSearch::default();
        search.query = query;
        search.status = if search.query.is_empty() {
            Some(search_prompt(kind))
        } else {
            search.due = Some(self.clock() + DEBOUNCE);
            Some(if matches!(kind, OverlayKind::CommandOutput(..)) { "Searching command output…" } else { "Searching open sessions…" })
        };
        self.session_search = search;
        self.dirty = true;
        self.schedule_timer();
    }

    pub(crate) fn request_session_search(&mut self, now: Instant) {
        let Some(kind) = self.overlay.as_ref().map(|overlay| overlay.kind).filter(|kind| kind.searches_output()) else {
            if self.session_search.due.is_some() || self.session_search.cancelled.is_some() {
                self.session_search = SessionSearch::default();
            }
            return;
        };
        if !self.session_search.due.is_some_and(|due| now >= due) {
            return;
        }
        self.session_search.due = None;
        let sources = match kind {
            OverlayKind::CommandOutput(pane, command) => {
                let Some(terminal) = self.terminal(pane).cloned() else {
                    self.session_search.status = Some("This command is no longer in the buffer");
                    self.dirty = true;
                    return;
                };
                VecDeque::from([Source { pane, buffer: SearchBuffer::Live(terminal), title: String::new(), command: Some(command), next_line: None, rows: 0 }])
            }
            _ => self.session_sources(),
        };
        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let cancelled = Arc::new(AtomicBool::new(false));
        let request = Request {
            id,
            query: self.session_search.query.clone(),
            options: crate::search::vt(self.config.terminal.search_options),
            cancelled: cancelled.clone(),
            sources,
        };
        self.session_search.id = id;
        self.session_search.cancelled = Some(cancelled);
        self.push(Effect::SearchSessions(request));
    }

    fn session_sources(&self) -> VecDeque<Source> {
        let mut sources = VecDeque::new();
        for index in std::iter::once(self.active).chain((0..self.tabs.len()).filter(|&i| i != self.active)) {
            let Some(tab) = self.tabs.get(index) else { continue };
            let title = self.tab_label(index);
            let mut panes = tab.root.panes();
            panes.sort_by_key(|&pane| pane != tab.focus);
            for pane in panes {
                if sources.len() == MAX_PANES {
                    break;
                }
                if let Some(p) = self.panes.get(&pane) {
                    let mut source_title = if title == p.profile.name { title.clone() } else { format!("{title} · {}", p.profile.name) };
                    if let Some(name) = &p.name {
                        source_title.push_str(" · ");
                        source_title.push_str(name);
                    }
                    sources.push_back(Source { pane, buffer: SearchBuffer::findable(p), title: source_title, command: None, next_line: None, rows: 0 });
                }
            }
            if sources.len() == MAX_PANES {
                break;
            }
        }
        let remaining = MAX_PANES - sources.len();
        if remaining != 0 {
            sources.extend(self.hidden_workspace_search_panes().into_iter().take(remaining).filter_map(|(pane, title)| {
                let buffer = SearchBuffer::findable(self.panes.get(&pane)?);
                Some(Source { pane, buffer, title, command: None, next_line: None, rows: 0 })
            }));
        }
        sources
    }

    pub fn session_search_reply(&mut self, reply: Reply) {
        let Some(overlay) = self.overlay.as_mut().filter(|overlay| overlay.kind.searches_output()) else { return };
        if reply.id != self.session_search.id {
            return;
        }
        self.session_search.cancelled = None;
        self.session_search.status = match &reply.hits {
            Ok(_) => None,
            Err(SearchError::Regex) => Some("Invalid regular expression"),
            Err(SearchError::CommandExpired) => Some("This command is no longer in the buffer"),
        };
        self.session_search.hits = reply.hits.unwrap_or_default();
        overlay.replace_results(self.session_search.hits.iter().enumerate().map(|(index, hit)| {
            let label = hit.found.preview.as_ref().map(|preview| preview.text.clone()).unwrap_or_else(|| {
                let context_start = hit.found.first.saturating_sub(32);
                let label: String = hit.found.text.chars().skip(context_start).take(128).collect();
                if context_start > 0 { format!("…{label}") } else { label }
            });
            Pick { label, hint: hit.title.clone(), value: index.to_string(), ..Pick::default() }
        }).collect());
        for ((_, ranges), hit) in overlay.shown.iter_mut().zip(&self.session_search.hits) {
            if let Some(range) = hit.found.preview.as_ref().and_then(|preview| preview.highlight.clone()) {
                ranges.push(range);
            }
        }
        self.dirty = true;
    }

    pub(crate) fn choose_session_match(&mut self, value: &str) -> bool {
        let Some(hit) = value.parse::<usize>().ok().and_then(|index| self.session_search.hits.get(index)) else { return false };
        let pane = hit.pane;
        let revealed = self.panes.get_mut(&pane).is_some_and(|p| {
            if p.host_id.is_none() {
                p.restore_history();
            }
            p.terminal.lock().reveal_buffer_match(&hit.found, hit.command)
        });
        let tab = if revealed && self.activate_workspace_for_pane(pane) {
            self.tabs.iter().position(|tab| tab.root.contains(pane))
        } else {
            None
        };
        let Some(tab) = tab else {
            self.session_search.hits.clear();
            if let Some(overlay) = self.overlay.as_mut() {
                overlay.replace_results(Vec::new());
            }
            self.session_search.status = Some("Output changed. Searching again…");
            self.session_search.due = Some(self.clock() + DEBOUNCE);
            self.dirty = true;
            self.schedule_timer();
            return false;
        };
        self.overlay = None;
        self.session_search = SessionSearch::default();
        self.activate(tab);
        if self.tabs[tab].zoomed.is_some() {
            self.tabs[tab].zoomed = Some(pane);
        }
        self.focus_pane(pane);
        self.dirty = true;
        true
    }

    pub(crate) fn session_search_status(&self) -> &'static str {
        self.session_search.status.map(t).unwrap_or_else(|| t("No matches"))
    }
}

fn search_prompt(kind: OverlayKind) -> &'static str {
    if matches!(kind, OverlayKind::CommandOutput(..)) {
        "Esc returns to the command"
    } else {
        "Search the output of open tabs and panes"
    }
}
