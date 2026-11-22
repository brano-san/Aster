use crate::app::{App, Effect};
use crate::config::CustomScheme;
use crate::i18n::{t, tf};
use crate::input::{Key, Mods};
use aster_gfx::render::Rect;
use aster_vt::{schemes, Palette, Rgb};

const FIELD_COUNT: usize = 23;

fn hex(c: Rgb) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

impl CustomScheme {
    pub(crate) fn palette(&self) -> Option<Palette> {
        let colors: Vec<_> = self.colors.iter().filter_map(|s| schemes::parse_hex(s)).collect();
        let cursor = self.cursor.as_deref().and_then(schemes::parse_hex);
        let mut palette = schemes::from_colors(schemes::parse_hex(&self.foreground)?, schemes::parse_hex(&self.background)?, cursor, &colors)?;
        palette.cursor_text = self.cursor_text.as_deref().and_then(schemes::parse_hex);
        palette.selection = self.selection.as_deref().and_then(schemes::parse_hex).unwrap_or(palette.selection);
        palette.selection_text = self.selection_text.as_deref().and_then(schemes::parse_hex);
        Some(palette)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SchemeIssue {
    MissingName,
    DuplicateName,
    Color(usize),
    MissingOriginal,
}

impl SchemeIssue {
    pub(crate) fn field(self) -> usize {
        if let Self::Color(field) = self { field } else { 0 }
    }

    pub(crate) fn message(self) -> String {
        match self {
            Self::MissingName => t("Enter a color scheme name").to_owned(),
            Self::DuplicateName => t("A color scheme with this name already exists").to_owned(),
            Self::Color(field) => tf("Enter a color in #RRGGBB format for {field}", &[("field", &SchemeEditor::label(field))]),
            Self::MissingOriginal => t("This color scheme no longer exists").to_owned(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SchemeEditor {
    pub original: Option<String>,
    pub fields: [String; FIELD_COUNT],
    pub focus: usize,
    pub replace: bool,
    pub error: Option<SchemeIssue>,
    pub scroll: f32,
    palette: Palette,
}

impl SchemeEditor {
    pub(crate) fn new(original: Option<String>, name: String, palette: Palette) -> Self {
        let fields = std::array::from_fn(|index| match index {
            0 => name.clone(),
            1 => hex(palette.foreground),
            2 => hex(palette.background),
            3 => hex(palette.cursor),
            4 => hex(palette.cursor_text.unwrap_or(palette.background)),
            5 => hex(palette.selection),
            6 => hex(palette.selection_text.unwrap_or(palette.foreground)),
            i => hex(palette.ansi[i - 7]),
        });
        Self { original, fields, focus: 0, replace: true, error: None, scroll: 0.0, palette }
    }

    pub(crate) fn label(field: usize) -> String {
        match field {
            0 => t("Name"),
            1 => t("Foreground"),
            2 => t("Background"),
            3 => t("Cursor"),
            4 => t("Cursor text"),
            5 => t("Selection"),
            6 => t("Selection text"),
            i => return tf("ANSI {index}", &[("index", &(i - 7).to_string())]),
        }.to_owned()
    }

    pub(crate) fn color(&self, field: usize) -> Option<Rgb> {
        let text = self.fields[field].trim();
        (text.len() == 7).then(|| schemes::parse_hex(text)).flatten()
    }

    pub(crate) fn preview(&self) -> Palette {
        let mut palette = self.palette;
        palette.foreground = self.color(1).unwrap_or(palette.foreground);
        palette.background = self.color(2).unwrap_or(palette.background);
        palette.cursor = self.color(3).unwrap_or(palette.cursor);
        palette.cursor_text = self.color(4).or(palette.cursor_text);
        palette.selection = self.color(5).unwrap_or(palette.selection);
        palette.selection_text = self.color(6).or(palette.selection_text);
        for (i, color) in palette.ansi.iter_mut().enumerate() {
            *color = self.color(i + 7).unwrap_or(*color);
        }
        palette
    }

    fn validate(&self, name_taken: impl FnOnce(&str) -> bool) -> Result<CustomScheme, SchemeIssue> {
        let name = self.fields[0].trim();
        if name.is_empty() {
            return Err(SchemeIssue::MissingName);
        }
        if name_taken(name) {
            return Err(SchemeIssue::DuplicateName);
        }
        let colors: Vec<_> = (1..FIELD_COUNT).map(|i| self.color(i).map(hex).ok_or(SchemeIssue::Color(i))).collect::<Result<_, _>>()?;
        Ok(CustomScheme {
            name: name.to_owned(), foreground: colors[0].clone(), background: colors[1].clone(),
            cursor: Some(colors[2].clone()), cursor_text: Some(colors[3].clone()),
            selection: Some(colors[4].clone()), selection_text: Some(colors[5].clone()),
            colors: colors[6..].to_vec(),
        })
    }

    fn focus(&mut self, field: usize) {
        self.focus = field;
        self.replace = true;
    }

    pub(crate) fn insert(&mut self, text: &str) {
        if std::mem::take(&mut self.replace) {
            self.fields[self.focus].clear();
        }
        self.fields[self.focus].push_str(text);
        self.error = None;
    }
}

pub(crate) struct EditorGeometry {
    pub frame: Rect,
    pub title: Rect,
    pub body: Rect,
    pub fields: [Rect; FIELD_COUNT],
    pub preview: Rect,
    pub message: Rect,
    pub cancel: Rect,
    pub save: Rect,
    pub max_scroll: f32,
}

pub(crate) fn geometry(area: Rect, scale: f32, scroll: f32) -> EditorGeometry {
    let px = |v: f32| (v * scale).round();
    let w = px(776.0).min(area.w - px(24.0)).max(1.0);
    let h = px(602.0).min(area.h - px(16.0)).max(1.0);
    let frame = Rect::new(area.x + ((area.w - w) * 0.5).round(), area.y + ((area.h - h) * 0.5).round(), w, h);
    let inner = frame.inset(px(16.0), 0.0);
    let title = Rect::new(inner.x, frame.y + px(8.0), inner.w, px(36.0));
    let button_y = frame.bottom() - px(42.0);
    let button_w = px(90.0).min(inner.w * 0.43);
    let save = Rect::new(inner.right() - button_w, button_y, button_w, px(30.0));
    let cancel = Rect::new(save.x - button_w - px(8.0), button_y, button_w, save.h);
    let message = Rect::new(inner.x, button_y - px(25.0), inner.w, px(22.0));
    let preview_h = (h * 0.18).min(px(88.0)).round();
    let preview = Rect::new(inner.x, message.y - preview_h - px(3.0), inner.w, preview_h);
    let body = Rect::new(inner.x, title.bottom(), inner.w, (preview.y - px(8.0) - title.bottom()).max(0.0));
    let row_h = px(49.0);
    let gap = px(12.0);
    let main_cols: usize = if body.w >= px(390.0) { 2 } else { 1 };
    let ansi_cols: usize = if body.w >= px(580.0) { 4 } else { 2 };
    let ansi_top = 1 + 6_usize.div_ceil(main_cols);
    let rows = ansi_top + 16_usize.div_ceil(ansi_cols);
    let max_scroll = (rows as f32 * row_h - body.h).max(0.0);
    let top = body.y - scroll.clamp(0.0, max_scroll);
    let fields = std::array::from_fn(|i| {
        let (cols, col, row) = match i {
            0 => (1, 0, 0),
            1..=6 => (main_cols, (i - 1) % main_cols, 1 + (i - 1) / main_cols),
            _ => (ansi_cols, (i - 7) % ansi_cols, ansi_top + (i - 7) / ansi_cols),
        };
        let field_w = ((body.w - (cols - 1) as f32 * gap) / cols as f32).floor();
        Rect::new(body.x + col as f32 * (field_w + gap), top + row as f32 * row_h, field_w.max(1.0), row_h)
    });
    EditorGeometry { frame, title, body, fields, preview, message, cancel, save, max_scroll }
}

impl App {
    fn scheme_name_taken(&self, name: &str, ignore: Option<usize>) -> bool {
        self.config.color_schemes.iter().enumerate().any(|(i, scheme)| Some(i) != ignore && scheme.name.eq_ignore_ascii_case(name))
            || self.available_schemes().any(|(scheme, custom)| !custom && scheme.name.eq_ignore_ascii_case(name))
    }

    fn current_settings_scheme(&self) -> Option<&crate::settings::SchemeRow> {
        let page = self.settings.as_ref().filter(|p| p.colors_page() && p.deleting_scheme.is_none())?;
        let name = page.scheme_tab.name(&self.config.appearance);
        page.scheme_item(name).and_then(|i| page.scheme_at(i))
    }

    pub(crate) fn scheme_edit_click(&mut self, bar: crate::settings_colors::SchemeBar, x: f32, y: f32) -> bool {
        let Some(row) = self.current_settings_scheme() else { return false };
        let edit = if row.custom { bar.cancel } else { bar.delete };
        if !edit.contains(x, y) {
            return false;
        }
        self.open_scheme_editor()
    }

    pub(crate) fn open_scheme_editor(&mut self) -> bool {
        let Some(row) = self.current_settings_scheme() else { return false };
        let Some(palette) = self.scheme(&row.name) else { return false };
        let original = row.custom.then(|| row.name.clone());
        let name = if row.custom {
            row.name.clone()
        } else {
            let copy = tf("{name} (copy)", &[("name", &row.name)]);
            if self.scheme_name_taken(&copy, None) {
                (2..).map(|i| format!("{copy} {i}")).find(|name| !self.scheme_name_taken(name, None)).unwrap()
            } else {
                copy
            }
        };
        self.settings.as_mut().unwrap().scheme_editor = Some(SchemeEditor::new(original, name, palette));
        true
    }

    fn save_scheme_editor(&mut self) {
        if self.config.read_only() { return self.warn_read_only_config(); }
        let Some(page) = self.settings.as_ref() else { return };
        let Some(editor) = page.scheme_editor.as_ref() else { return };
        let tab = page.scheme_tab;
        let original = editor.original.as_ref().and_then(|name| self.config.color_schemes.iter().position(|scheme| scheme.name.eq_ignore_ascii_case(name)));
        if editor.original.is_some() && original.is_none() {
            return self.scheme_editor_error(SchemeIssue::MissingOriginal);
        }
        let scheme = match editor.validate(|name| self.scheme_name_taken(name, original)) {
            Ok(scheme) => scheme,
            Err(issue) => return self.scheme_editor_error(issue),
        };
        let mut config = self.config.clone();
        if let Some(i) = original {
            let old = &self.config.color_schemes[i].name;
            if *old != scheme.name {
                let old = old.clone();
                rename_scheme_references(&mut config, &old, &scheme.name);
                for pane in self.panes.values_mut() {
                    rename_profile_scheme(&mut pane.profile.scheme, &old, &scheme.name);
                    rename_profile_scheme(&mut pane.source_profile.scheme, &old, &scheme.name);
                }
            }
            config.color_schemes[i] = scheme;
        } else {
            *tab.name_mut(&mut config.appearance) = scheme.name.clone();
            config.color_schemes.push(scheme);
        }
        if let Some(page) = self.settings.as_mut() {
            page.scheme_editor = None;
            page.query.clear();
            page.refilter();
        }
        if config != self.config {
            self.set_config(config);
            self.push_save_config();
        }
        self.select_scheme_tab(tab);
    }

    pub(crate) fn scheme_editor_key(&mut self, key: Key, mods: Mods) {
        let Some(geo) = self.settings_geometry() else { return };
        let scale = self.ui();
        let Some(page) = self.settings.as_mut() else { return };
        match key {
            Key::Escape => page.scheme_editor = None,
            Key::Enter if mods.ctrl => self.save_scheme_editor(),
            Key::Char('V') if mods.ctrl => self.push(Effect::RequestPaste),
            _ => {
                let Some(editor) = page.scheme_editor.as_mut() else { return };
                match key {
                    Key::Tab | Key::Enter => {
                        let step = if mods.shift { FIELD_COUNT - 1 } else { 1 };
                        editor.focus((editor.focus + step) % FIELD_COUNT);
                        reveal(editor, geo.page, scale);
                    }
                    Key::Char('A') if mods.ctrl => editor.replace = true,
                    Key::Backspace | Key::Delete => {
                        if std::mem::take(&mut editor.replace) || mods.ctrl || key == Key::Delete {
                            editor.fields[editor.focus].clear();
                        } else {
                            editor.fields[editor.focus].pop();
                        }
                        editor.error = None;
                    }
                    _ => {}
                }
            }
        }
    }

    pub(crate) fn scheme_editor_click(&mut self, area: Rect, x: f32, y: f32) -> bool {
        let scale = self.ui();
        let Some(page) = self.settings.as_mut() else { return false };
        let Some(editor) = page.scheme_editor.as_mut() else { return false };
        let geo = geometry(area, scale, editor.scroll);
        if geo.cancel.contains(x, y) {
            page.scheme_editor = None;
        } else if geo.save.contains(x, y) {
            self.save_scheme_editor();
        } else if geo.body.contains(x, y) {
            if let Some(i) = geo.fields.iter().position(|r| r.contains(x, y)) {
                editor.focus(i);
                reveal(editor, area, scale);
            }
        }
        true
    }

    pub(crate) fn scheme_editor_wheel(&mut self, area: Rect, dy: f32) -> bool {
        let scale = self.ui();
        let Some(editor) = self.settings.as_mut().and_then(|p| p.scheme_editor.as_mut()) else { return false };
        let geo = geometry(area, scale, editor.scroll);
        editor.scroll = (editor.scroll - dy * 60.0 * scale).clamp(0.0, geo.max_scroll);
        self.dirty = true;
        true
    }

    pub(crate) fn scheme_editor_error(&mut self, issue: SchemeIssue) {
        let Some(geo) = self.settings_geometry() else { return };
        let scale = self.ui();
        let Some(editor) = self.settings.as_mut().and_then(|p| p.scheme_editor.as_mut()) else { return };
        editor.error = Some(issue);
        editor.focus(issue.field());
        reveal(editor, geo.page, scale);
    }
}

fn rename_profile_scheme(value: &mut Option<String>, old: &str, new: &str) {
    if value.as_deref().is_some_and(|name| name.eq_ignore_ascii_case(old)) {
        *value = Some(new.to_owned());
    }
}

fn rename_scheme_references(config: &mut crate::config::Config, old: &str, new: &str) {
    for tab in crate::settings_colors::SchemeTab::ALL {
        if tab.name(&config.appearance).eq_ignore_ascii_case(old) {
            *tab.name_mut(&mut config.appearance) = new.to_owned();
        }
    }
    for profile in &mut config.profiles {
        rename_profile_scheme(&mut profile.scheme, old, new);
    }
    for defaults in config.profile_defaults.values_mut().chain(config.groups.iter_mut().flat_map(|group| group.defaults.values_mut())) {
        rename_profile_scheme(&mut defaults.scheme, old, new);
    }
}

fn reveal(editor: &mut SchemeEditor, area: Rect, scale: f32) {
    let geo = geometry(area, scale, editor.scroll);
    let rect = geo.fields[editor.focus];
    let delta = if rect.y < geo.body.y { rect.y - geo.body.y } else { (rect.bottom() - geo.body.bottom()).max(0.0) };
    editor.scroll = (editor.scroll.clamp(0.0, geo.max_scroll) + delta).clamp(0.0, geo.max_scroll);
}

#[cfg(test)]
#[path = "../test/scheme_editor.rs"]
mod tests;
