use crate::app::{App, Metrics};
use crate::i18n::t;
use crate::overlay::{Overlay, OverlayKind};
use crate::panes::PaneId;
use aster_gfx::render::Rect;
use aster_vt::{CursorShape, Flags, SelectionType, Side, Terminal};
use std::hash::{DefaultHasher, Hash, Hasher};

pub type NodeId = u64;
pub const ROOT: NodeId = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Window,
    TabList,
    Tab,
    Terminal,
    Dialog,
    Edit,
    List,
    ListItem,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub focus: Option<NodeId>,
}

impl Tree {
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|node| node.id == id)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub role: Role,
    pub name: String,
    pub bounds: Rect,
    pub focusable: bool,
    pub selected: bool,
    pub invokable: bool,
    pub text: Option<Text>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Text {
    pub value: String,
    pub selection: Option<(usize, usize)>,
    pub caret: Option<usize>,
    pub lines: Vec<TextLine>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    pub start: usize,
    pub end: usize,
    pub bounds: Rect,
    pub offsets: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Focus(NodeId),
    Invoke(NodeId),
    SelectItem(NodeId),
    SelectText { node: NodeId, start: usize, end: usize },
}

const TABS: NodeId = 2;
const PALETTE: NodeId = 3;
const PALETTE_INPUT: NodeId = 4;
const PALETTE_RESULTS: NodeId = 5;
const SEARCH: NodeId = 6;
const SEARCH_INPUT: NodeId = 7;

impl Node {
    fn new(id: NodeId, parent: Option<NodeId>, role: Role, name: impl Into<String>, bounds: Rect) -> Self {
        Self { id, parent, role, name: name.into(), bounds, focusable: false, selected: false, invokable: false, text: None }
    }

    fn edit(id: NodeId, parent: NodeId, name: &str, bounds: Rect, value: &str) -> Self {
        let mut node = Self::new(id, Some(parent), Role::Edit, name, bounds);
        let end = value.chars().count();
        node.focusable = true;
        node.text = Some(Text { value: value.into(), selection: None, caret: Some(end), lines: vec![TextLine { start: 0, end, bounds, offsets: Vec::new() }] });
        node
    }
}

fn tab_id(key: u64) -> NodeId { (key << 4) | 8 }

fn terminal_id(pane: PaneId) -> NodeId { (pane << 4) | 9 }

fn result_id(overlay: &Overlay, item: usize) -> NodeId {
    let mut hash = DefaultHasher::new();
    (overlay.opened_at, item, &overlay.items[item].value).hash(&mut hash);
    (hash.finish() << 4) | 10
}

fn clipped(bounds: Rect, clip: Rect) -> Rect {
    let x = bounds.x.max(clip.x);
    let y = bounds.y.max(clip.y);
    Rect::new(x, y, (bounds.right().min(clip.right()) - x).max(0.0), (bounds.bottom().min(clip.bottom()) - y).max(0.0))
}

impl App {
    pub fn accessibility_tree(&self) -> Tree {
        let window = Rect::new(0.0, 0.0, self.size.0, self.size.1);
        let mut tree = Tree { nodes: vec![Node::new(ROOT, None, Role::Window, "Aster", window)], focus: None };
        if self.accessibility_modal() {
            return tree;
        }
        let focus = if let Some(overlay) = &self.overlay {
            self.accessible_overlay(&mut tree.nodes, overlay, window)
        } else {
            self.accessible_tabs(&mut tree.nodes, window);
            self.accessible_content(&mut tree.nodes, window)
        };
        tree.focus = focus.filter(|_| self.focused);
        tree
    }

    fn accessibility_modal(&self) -> bool {
        self.vault_prompt.is_some() || self.ssh_auth_active() || self.ssh_forward_form.is_some()
            || self.paste_confirm.is_some() || self.close_confirm.is_some() || self.quick_selection.is_some()
            || self.rename.is_some() || self.pane_rename.is_some() || self.menu.is_some()
    }

    fn accessible_tabs(&self, nodes: &mut Vec<Node>, window: Rect) {
        if self.chrome.tabs.is_empty() {
            return;
        }
        nodes.push(Node::new(TABS, Some(ROOT), Role::TabList, t("Terminal tabs"), clipped(self.chrome.tab_bar, window)));
        for (index, (tab, slot)) in self.tabs.iter().zip(&self.chrome.tabs).enumerate() {
            let mut node = Node::new(tab_id(tab.key), Some(TABS), Role::Tab, self.tab_title(index), clipped(slot.rect, window));
            node.focusable = true;
            node.selected = index == self.active;
            node.invokable = true;
            nodes.push(node);
        }
    }

    fn accessible_overlay(&self, nodes: &mut Vec<Node>, overlay: &Overlay, window: Rect) -> Option<NodeId> {
        let geo = crate::view::overlay_geometry(self, overlay);
        let name = if overlay.kind == OverlayKind::Commands { t("Command palette") } else { overlay.placeholder() };
        nodes.push(Node::new(PALETTE, Some(ROOT), Role::Dialog, name, clipped(geo.frame, window)));
        nodes.push(Node::edit(PALETTE_INPUT, PALETTE, overlay.placeholder(), clipped(geo.input, window), &overlay.query));
        let list = clipped(geo.list, window);
        nodes.push(Node::new(PALETTE_RESULTS, Some(PALETTE), Role::List, t("Results"), list));
        for (position, rect) in geo.items {
            let item = overlay.shown[position].0;
            let mut node = Node::new(result_id(overlay, item), Some(PALETTE_RESULTS), Role::ListItem, &overlay.items[item].label, clipped(rect, list));
            node.selected = position == overlay.selected;
            node.invokable = true;
            nodes.push(node);
        }
        Some(PALETTE_INPUT)
    }

    fn accessible_content(&self, nodes: &mut Vec<Node>, window: Rect) -> Option<NodeId> {
        if self.settings.is_some() || self.files.is_some() || self.global_transfers_open() {
            return None;
        }
        let pane = self.focused_pane()?;
        let (bounds, metrics) = self.accessible_terminal_geometry(pane)?;
        let mut node = Node::new(terminal_id(pane), Some(ROOT), Role::Terminal, t("Terminal"), clipped(bounds, window));
        node.focusable = true;
        node.text = Some(Projection::new(&self.terminal(pane)?.lock(), bounds, metrics).text);
        let id = node.id;
        nodes.push(node);
        if let Some((bar, geo)) = self.search.as_ref().zip(self.search_geometry()) {
            nodes.push(Node::new(SEARCH, Some(ROOT), Role::Dialog, t("Search"), clipped(geo.frame, window)));
            nodes.push(Node::edit(SEARCH_INPUT, SEARCH, t("Search"), clipped(geo.field, window), &bar.query));
            Some(SEARCH_INPUT)
        } else {
            self.terminal_has_focus().then_some(id)
        }
    }

    fn accessible_terminal_geometry(&self, pane: PaneId) -> Option<(Rect, Metrics)> {
        let (_, rect) = self.layout.panes.iter().find(|(id, _)| *id == pane)?;
        let inner = crate::view::pane_inner_with(self.pane_terminal_rect(pane, *rect), self.ui(), &self.config.terminal);
        Some((inner, self.metrics?))
    }

    pub(crate) fn perform_accessibility(&mut self, action: Action) {
        if self.accessibility_modal() {
            return;
        }
        match action {
            Action::Invoke(id) | Action::Focus(id) | Action::SelectItem(id) => self.accessibility_activate(id, action),
            Action::SelectText { node, start, end } => self.accessibility_select(node, start, end),
        }
    }

    fn accessibility_activate(&mut self, id: NodeId, action: Action) {
        if let Some(overlay) = &self.overlay {
            let Some(position) = overlay.shown.iter().position(|(item, _)| result_id(overlay, *item) == id) else { return };
            if matches!(action, Action::Invoke(_)) {
                self.choose(overlay.kind, overlay.items[overlay.shown[position].0].value.clone());
            } else if matches!(action, Action::SelectItem(_)) {
                self.overlay.as_mut().expect("overlay").select(position);
                self.preview_scheme();
                self.dirty = true;
            }
            return;
        }
        if let Some(index) = self.tabs.iter().position(|tab| tab_id(tab.key) == id) {
            self.activate(index);
        } else if matches!(action, Action::Focus(_)) && self.focused_pane().is_some_and(|pane| terminal_id(pane) == id) {
            self.close_search();
        }
    }

    fn accessibility_select(&mut self, node: NodeId, start: usize, end: usize) {
        if self.overlay.is_some() || self.files.is_some() || self.settings.is_some() || self.global_transfers_open() {
            return;
        }
        let Some(pane) = self.focused_pane().filter(|pane| terminal_id(*pane) == node) else { return };
        let Some((bounds, metrics)) = self.accessible_terminal_geometry(pane) else { return };
        let Some(terminal) = self.terminal(pane) else { return };
        let mut terminal = terminal.lock();
        let projection = Projection::new(&terminal, bounds, metrics);
        let Some(positions) = projection.positions.get(start..end) else { return };
        if positions.is_empty() {
            terminal.clear_selection();
        } else {
            let mut cells = positions.iter().flatten();
            let Some(&(line, col)) = cells.next() else { return };
            let first = terminal.viewport_point(line, col);
            let last = cells.next_back().map_or(first, |&(line, col)| terminal.viewport_point(line, col));
            terminal.start_selection(SelectionType::Simple, first, Side::Left);
            terminal.update_selection(last, Side::Right);
        }
        drop(terminal);
        self.dirty = true;
    }
}

#[derive(Default)]
struct Row {
    cells: Vec<CellText>,
    wrapped: bool,
}

struct CellText {
    character: char,
    col: usize,
    width: usize,
    selected: bool,
}

struct Projection {
    text: Text,
    positions: Vec<Option<(usize, usize)>>,
}

impl Projection {
    fn new(terminal: &Terminal, bounds: Rect, metrics: Metrics) -> Self {
        let size = terminal.size();
        let cursor = terminal.cursor();
        let mut rows: Vec<Row> = (0..size.rows).map(|_| Row::default()).collect();
        terminal.visible(|visible| {
            let Some(row) = rows.get_mut(visible.line) else { return };
            let cell = visible.cell;
            row.wrapped |= cell.flags.contains(Flags::WRAPLINE);
            if cell.flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                return;
            }
            let width = if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
            let entry = |character| CellText { character, col: visible.col, width, selected: visible.selected };
            row.cells.push(entry(cell.c));
            if let Some(combining) = cell.zerowidth() {
                row.cells.extend(combining.iter().copied().map(entry));
            }
        });
        let mut text = Text::default();
        let mut positions = Vec::new();
        let mut selection_ended = false;
        for (line, row) in rows.iter_mut().enumerate() {
            if !row.wrapped {
                while row.cells.last().is_some_and(|cell| {
                    cell.character == ' ' && !cell.selected && (line != cursor.line || cursor.shape == CursorShape::Hidden || cell.col >= cursor.col)
                }) {
                    row.cells.pop();
                }
            }
            let start = positions.len();
            if line == cursor.line && cursor.shape != CursorShape::Hidden {
                text.caret = Some(start + row.cells.partition_point(|cell| cell.col < cursor.col));
            }
            let rect = Rect::new(bounds.x, bounds.y + line as f32 * metrics.h, size.cols as f32 * metrics.w, metrics.h);
            let line_bounds = clipped(rect, bounds);
            let x = |col| (col as f32 * metrics.w).min(line_bounds.w);
            let mut offsets = Vec::with_capacity(row.cells.len() + 2);
            for cell in &row.cells {
                if cell.selected && !selection_ended {
                    let (_, end) = text.selection.get_or_insert((positions.len(), positions.len()));
                    *end = positions.len() + 1;
                } else if !cell.selected && text.selection.is_some() {
                    selection_ended = true;
                }
                text.value.push(cell.character);
                positions.push(Some((line, cell.col)));
                offsets.push(x(cell.col));
            }
            let end_x = row.cells.last().map_or(0.0, |cell| x(cell.col + cell.width));
            offsets.push(end_x);
            if !row.wrapped && line + 1 < size.rows {
                text.value.push('\n');
                positions.push(None);
                offsets.push(end_x);
            }
            text.lines.push(TextLine { start, end: positions.len(), bounds: line_bounds, offsets });
        }
        Self { text, positions }
    }
}
