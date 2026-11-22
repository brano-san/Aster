use crate::app::{App, Effect};
use crate::config::Config;
use crate::i18n::{t, tf};
use crate::input::{Key, Mods};
use crate::text_editor::TextEditor;
use aster_gfx::render::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    Current,
    Defaults,
}

#[derive(Clone, Debug, Default)]
enum Validation {
    #[default]
    Valid,
    Syntax { line: usize, message: String },
    Invalid(String),
}

#[derive(Default)]
pub(crate) struct ConfigEditor {
    pub file: TextEditor,
    pub defaults: Option<TextEditor>,
    pub focus: Option<Side>,
    pub cell_width: f32,
    pub path_rows: usize,
    ready: bool,
    available: bool,
    base: Option<String>,
    latest: Option<String>,
    modified: bool,
    validation: Validation,
    saving: bool,
    dragging: bool,
    feedback: Option<String>,
    paste_sequence: u64,
    pending_paste: Option<u64>,
}

impl ConfigEditor {
    pub fn cancel_paste(&mut self) {
        self.pending_paste = None;
    }

    fn reset(&mut self, preserve_undo: bool) {
        let text = self.latest.as_deref().unwrap_or_default();
        if preserve_undo {
            self.file.key(Key::Char('A'), Mods { ctrl: true, ..Mods::NONE }, 1, true);
            self.file.insert(text);
            self.file.place_cursor(0, 0, false);
            self.file.top = 0;
            self.file.left = 0;
        } else {
            self.file = TextEditor::new(text);
        }
        self.base.clone_from(&self.latest);
        self.changed();
    }

    fn receive(&mut self, source: Option<String>) {
        let reset = !self.ready || (!self.modified && self.base != source);
        self.ready = true;
        self.available = true;
        self.latest = source;
        if reset {
            self.cancel_paste();
            self.reset(false);
        }
    }

    fn changed(&mut self) {
        self.feedback = None;
        let default = self.base.as_deref().unwrap_or_default();
        self.modified = if default.contains("\r\n") {
            self.file.text() != default.replace("\r\n", "\n")
        } else {
            self.file.text() != default
        };
        let text = self.file.text();
        self.validation = match toml::from_str::<toml::Value>(text) {
            Err(e) => {
                let prefix = e.span().and_then(|span| text.get(..span.start)).unwrap_or_default();
                let line = prefix.bytes().filter(|&b| b == b'\n').count() + 1;
                let column = prefix.rsplit('\n').next().unwrap_or_default().chars().count() + 1;
                let message = tf("Line {line}, column {column}: {error}", &[("line", &line.to_string()), ("column", &column.to_string()), ("error", e.message())]);
                Validation::Syntax { line, message }
            }
            Ok(value) => match Config::from_value(text, value) {
                Ok(config) if config.read_only() => Validation::Invalid(config.read_only_message()),
                Ok(_) => Validation::Valid,
                Err(error) => Validation::Invalid(error),
            },
        };
    }

    fn conflict(&self) -> bool {
        self.ready && self.available && self.base != self.latest
    }

    fn writable(&self, read_only: bool) -> bool {
        self.available && !read_only && !self.saving
    }

    pub fn can_save(&self, read_only: bool) -> bool {
        self.writable(read_only) && self.modified && !self.conflict() && matches!(self.validation, Validation::Valid)
    }

    pub fn save_label(&self) -> &str {
        match (&self.validation, self.saving) {
            (_, true) => t("Saving configuration…"),
            (Validation::Syntax { .. }, _) => t("Syntax error"),
            (Validation::Invalid(_), _) => t("Invalid configuration"),
            _ => t("Save and apply"),
        }
    }

    pub fn status(&self, config: &Config) -> (String, bool) {
        if config.read_only() { return (config.read_only_message(), true); }
        if let Some(feedback) = &self.feedback { return (feedback.clone(), true); }
        if !self.available { return (t("The config file could not be read").into(), true); }
        if self.conflict() { return (t("The config file changed on disk").into(), true); }
        match &self.validation {
            Validation::Syntax { message, .. } | Validation::Invalid(message) => (message.clone(), true),
            Validation::Valid => (t("The configuration is valid").into(), false),
        }
    }

    pub fn error_line(&self) -> Option<usize> {
        match &self.validation {
            Validation::Syntax { line, .. } => Some(*line - 1),
            _ => None,
        }
    }

    fn editor_mut(&mut self, side: Side) -> &mut TextEditor {
        match side {
            Side::Current => &mut self.file,
            Side::Defaults => self.defaults.as_mut().expect("visible defaults"),
        }
    }

    fn toggle_defaults(&mut self) {
        if self.defaults.take().is_none() {
            self.defaults = Some(TextEditor::new(&Config::defaults_text()));
        } else if self.focus == Some(Side::Defaults) {
            self.focus = Some(Side::Current);
        }
        self.dragging = false;
    }
}

#[derive(Clone, Copy)]
pub(crate) struct EditorView {
    pub frame: Rect,
    pub title: Rect,
    pub text: Rect,
    pub row_height: f32,
    pub cell_width: f32,
}

impl EditorView {
    fn new(frame: Rect, s: f32, cell_width: f32) -> Self {
        let title = Rect::new(frame.x + 10.0 * s, frame.y, (frame.w - 20.0 * s).max(0.0), 28.0 * s);
        let gutter = (50.0 * s).min(frame.w * 0.25);
        let text = Rect::new(frame.x + gutter, title.bottom(), (frame.w - gutter - 6.0 * s).max(0.0), (frame.bottom() - title.bottom() - 5.0 * s).max(0.0));
        Self { frame, title, text, row_height: (20.0 * s).round().max(1.0), cell_width }
    }

    pub fn rows(self) -> usize { (self.text.h / self.row_height).floor().max(1.0) as usize }
    pub fn cols(self) -> usize { (self.text.w / self.cell_width).floor().max(1.0) as usize }

    fn position(self, x: f32, y: f32, editor: &TextEditor) -> (usize, usize) {
        let row = ((y - self.text.y) / self.row_height).floor().clamp(0.0, self.rows().saturating_sub(1) as f32) as usize;
        let col = ((x - self.text.x) / self.cell_width).round().clamp(0.0, self.cols() as f32) as usize;
        (editor.top + row, if x < self.text.x { 0 } else { editor.left + col })
    }
}

#[derive(Clone, Copy)]
pub(crate) struct ConfigLayout {
    pub path: Rect,
    pub save: Rect,
    pub defaults: Rect,
    pub reveal: Rect,
    pub open: Rect,
    pub reload: Option<Rect>,
    pub status: Rect,
    pub file: EditorView,
    pub reference: Option<EditorView>,
}

impl ConfigLayout {
    fn new(area: Rect, scale: f32, editor: &ConfigEditor) -> Self {
        let s = scale;
        let gap = 8.0 * s;
        let path = Rect::new(area.x, area.y, area.w, (6.0 + editor.path_rows.clamp(1, 3) as f32 * 16.0) * s);
        let count = 4 + usize::from(editor.conflict());
        let per_row: usize = if area.w >= 680.0 * s { 4 } else { 2 };
        let width = ((area.w - (per_row - 1) as f32 * gap) / per_row as f32).max(0.0);
        let button = |i: usize| Rect::new(
            area.x + (i % per_row) as f32 * (width + gap),
            path.bottom() + (i / per_row) as f32 * (30.0 * s + gap),
            width, 30.0 * s,
        );
        let status = Rect::new(area.x, button(count - 1).bottom() + 3.0 * s, area.w, 38.0 * s);
        let work = Rect::new(area.x, status.bottom(), area.w, (area.bottom() - status.bottom()).max(0.0));
        let (file, reference) = if editor.defaults.is_none() {
            (work, None)
        } else if work.w >= 520.0 * s {
            let w = ((work.w - gap) / 2.0).max(0.0);
            (Rect::new(work.x, work.y, w, work.h), Some(Rect::new(work.x + w + gap, work.y, w, work.h)))
        } else {
            let h = ((work.h - gap) / 2.0).max(0.0);
            (Rect::new(work.x, work.y, work.w, h), Some(Rect::new(work.x, work.y + h + gap, work.w, h)))
        };
        let cell = if editor.cell_width > 0.0 { editor.cell_width } else { 8.0 * s };
        Self {
            path, save: button(0), defaults: button(1), reveal: button(2), open: button(3),
            reload: (count > 4).then(|| button(4)), status, file: EditorView::new(file, s, cell),
            reference: reference.map(|r| EditorView::new(r, s, cell)),
        }
    }

    fn view(self, side: Side) -> Option<EditorView> {
        match side { Side::Current => Some(self.file), Side::Defaults => self.reference }
    }

    fn hit(self, x: f32, y: f32) -> Option<Side> {
        if y >= self.file.text.y && self.file.frame.contains(x, y) { return Some(Side::Current); }
        self.reference.filter(|view| y >= view.text.y && view.frame.contains(x, y)).map(|_| Side::Defaults)
    }
}

impl App {
    pub fn set_config_editor_source(&mut self, source: Result<Option<String>, ()>) {
        match source {
            Ok(source) => self.config_editor.receive(source),
            Err(()) => {
                self.config_editor.ready = true;
                self.config_editor.available = false;
            }
        }
        self.dirty = true;
    }

    pub fn config_editor_saved(&mut self, text: String) {
        let editor = &mut self.config_editor;
        editor.saving = false;
        editor.base = Some(text.clone());
        editor.receive(Some(text));
        editor.changed();
        self.dirty = true;
    }

    pub fn config_editor_failed(&mut self, error: String) {
        self.config_editor.saving = false;
        self.config_editor.feedback = Some(error);
        self.dirty = true;
    }

    pub(crate) fn config_editor_layout(&self) -> Option<ConfigLayout> {
        let page = self.settings.as_ref()?;
        if !self.config_editor.ready || page.searching() || crate::settings::PAGES.get(page.page) != Some(&crate::settings::CONFIG_FILE) { return None; }
        Some(ConfigLayout::new(self.settings_geometry()?.list, self.ui(), &self.config_editor))
    }

    pub(crate) fn config_editor_focused(&self) -> bool {
        self.config_editor.focus.is_some() && self.config_editor_layout().is_some()
    }

    fn save_raw_config(&mut self) {
        let editor = &mut self.config_editor;
        if !editor.can_save(self.config.read_only()) { return; }
        editor.saving = true;
        let text = if editor.base.as_deref().is_some_and(|source| source.contains("\r\n")) {
            editor.file.text().replace('\n', "\r\n")
        } else {
            editor.file.text().to_owned()
        };
        self.effects.push(Effect::SaveRawConfig { text, expected: editor.base.clone() });
        self.dirty = true;
    }

    pub(crate) fn config_editor_text(&mut self, text: &str) -> bool {
        if self.overlay.is_some() || !self.config_editor_focused() { return false; }
        if let Some(layout) = self.config_editor_layout() {
            let editor = &mut self.config_editor;
            if editor.focus == Some(Side::Current) && editor.writable(self.config.read_only()) {
                if editor.file.insert(text) { editor.changed(); }
                editor.file.reveal_cursor(layout.file.rows(), layout.file.cols());
                self.dirty = true;
            }
        }
        true
    }

    pub fn paste_config_editor(&mut self, request: u64, text: &str) {
        if self.config_editor.pending_paste == Some(request) {
            self.config_editor.cancel_paste();
            self.config_editor_text(text);
        }
    }

    pub(crate) fn config_editor_key(&mut self, key: Key, mods: Mods) -> bool {
        let Some(layout) = self.config_editor_layout() else { return false };
        self.config_editor.cancel_paste();
        if mods.ctrl && matches!(key, Key::PageUp | Key::PageDown) { return false; }
        if key == Key::Escape {
            return self.config_editor.focus.take().is_some();
        }
        if mods.ctrl && matches!(key, Key::Char('S') | Key::Enter) {
            self.save_raw_config();
            return true;
        }
        if key == Key::F(6) {
            self.config_editor.focus = Some(if self.config_editor.focus == Some(Side::Current) && layout.reference.is_some() { Side::Defaults } else { Side::Current });
            return true;
        }
        let Some(side) = self.config_editor.focus else {
            if matches!(key, Key::Enter | Key::Tab) { self.config_editor.focus = Some(Side::Current); }
            return key != Key::Backspace;
        };
        let view = layout.view(side).expect("visible editor");
        let writable = side == Side::Current && self.config_editor.writable(self.config.read_only());
        if mods.ctrl && key == Key::Char('V') {
            if writable {
                let editor = &mut self.config_editor;
                editor.paste_sequence = editor.paste_sequence.wrapping_add(1);
                editor.pending_paste = Some(editor.paste_sequence);
                self.effects.push(Effect::RequestConfigPaste(editor.paste_sequence));
            }
            return true;
        }
        let editor = self.config_editor.editor_mut(side);
        if mods.ctrl && matches!(key, Key::Char('C') | Key::Char('X')) {
            if !editor.selection().is_empty() {
                self.effects.push(Effect::SetClipboard(editor.selected_text().into()));
                if key == Key::Char('X') && writable && editor.insert("") { self.config_editor.changed(); }
            }
        } else {
            let changed = editor.key(key, mods, view.rows(), writable);
            editor.reveal_cursor(view.rows(), view.cols());
            if changed { self.config_editor.changed(); }
        }
        true
    }

    pub(crate) fn config_editor_click(&mut self, x: f32, y: f32) -> bool {
        let Some(layout) = self.config_editor_layout() else { return false };
        self.config_editor.cancel_paste();
        if layout.save.contains(x, y) {
            self.save_raw_config();
        } else if layout.defaults.contains(x, y) {
            self.config_editor.toggle_defaults();
        } else if layout.open.contains(x, y) {
            self.effects.push(Effect::OpenConfigFile);
        } else if layout.reveal.contains(x, y) {
            self.effects.push(Effect::RevealConfigFile);
        } else if layout.reload.is_some_and(|r| r.contains(x, y)) {
            self.config_editor.reset(true);
        } else if let Some(side) = layout.hit(x, y) {
            self.config_editor.focus = Some(side);
            self.config_editor.dragging = true;
            let editor = self.config_editor.editor_mut(side);
            let (row, col) = layout.view(side).unwrap().position(x, y, editor);
            editor.place_cursor(row, col, false);
        }
        true
    }

    pub(crate) fn config_editor_mouse_move(&mut self, x: f32, y: f32) -> bool {
        if !self.config_editor.dragging { return false; }
        let Some((side, view)) = self.config_editor.focus.and_then(|side| self.config_editor_layout()?.view(side).map(|view| (side, view))) else { return false };
        let editor = self.config_editor.editor_mut(side);
        if y < view.text.y { editor.scroll(-1, view.rows()); }
        if y >= view.text.bottom() { editor.scroll(1, view.rows()); }
        let (row, col) = view.position(x, y, editor);
        editor.place_cursor(row, col, true);
        self.dirty = true;
        true
    }

    pub(crate) fn config_editor_mouse_up(&mut self) -> bool {
        std::mem::take(&mut self.config_editor.dragging)
    }

    pub(crate) fn config_editor_wheel(&mut self, dy: f32) -> bool {
        let Some(layout) = self.config_editor_layout() else { return false };
        if let Some(side) = layout.hit(self.mouse.0, self.mouse.1).or(self.config_editor.focus) {
            self.config_editor.editor_mut(side).scroll((-dy * 3.0).round() as isize, layout.view(side).unwrap().rows());
            self.dirty = true;
        }
        true
    }
}

#[cfg(test)]
#[path = "../test/config_editor.rs"]
mod tests;
