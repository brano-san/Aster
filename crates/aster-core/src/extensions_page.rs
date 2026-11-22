use crate::app::{App, Effect, TOAST};
use crate::extensions::{CatalogEntry, Reply, Request, Status, https_url};
use crate::i18n::{t, tf};
use crate::input::{Key, Mods};
use crate::settings::{EXTENSIONS, PAGES, SettingsPage};
use aster_gfx::render::Rect;

#[derive(Clone, Debug, Default, PartialEq)]
enum Catalog {
    #[default]
    Empty,
    Loading,
    Loaded(Vec<CatalogEntry>),
    Failed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Tab(bool),
    Field,
    Submit,
    Root,
    Row(usize, usize),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Page {
    pub available: bool,
    pub editing: bool,
    pub input: String,
    pub focus: Option<Target>,
    pub busy: bool,
    pub feedback: Option<String>,
    replace: bool,
    scroll: f32,
    confirm: Option<String>,
    catalog: Catalog,
}

enum Command {
    Toggle(String),
    Confirm(String),
    Cancel,
    Request(Request),
}

pub(crate) struct Button {
    pub label: String,
    pub enabled: bool,
    command: Command,
}

impl Button {
    fn new(label: &str, enabled: bool, command: Command) -> Self {
        Self { label: label.into(), enabled, command }
    }
}

pub(crate) struct Row {
    pub title: String,
    pub metadata: String,
    pub description: String,
    pub status: String,
    pub failed: bool,
    pub buttons: Vec<Button>,
}

pub(crate) struct Layout {
    pub tabs: [Rect; 2],
    pub field: Rect,
    pub submit: Rect,
    pub root: Option<Rect>,
    pub feedback: Rect,
    pub list: Rect,
    pub rows: Vec<(Rect, Vec<Rect>)>,
    pub scrollbar: Option<Rect>,
    maximum: f32,
}

impl SettingsPage {
    pub(crate) fn extensions_page(&self) -> bool {
        PAGES.get(self.page) == Some(&EXTENSIONS)
    }
}

impl Page {
    pub fn layout(&self, area: Rect, s: f32, rows: &[Row]) -> Layout {
        let gap = 8.0 * s;
        let h = 31.0 * s;
        let tab_w = (140.0 * s).min((area.w - gap) * 0.5);
        let tabs = [Rect::new(area.x, area.y, tab_w, h), Rect::new(area.x + tab_w + gap, area.y, tab_w, h)];
        let y = area.y + h + 12.0 * s;
        let root_w = if self.available { 0.0 } else { (205.0 * s).min(area.w * 0.5) };
        let compact = !self.available && area.w < 630.0 * s;
        let submit_w = ((if self.available { 112.0 } else { 158.0 }) * s).min(area.w * 0.5);
        let root = (!self.available).then(|| Rect::new(area.right() - root_w,
            if compact { y + 21.0 * s + h + gap } else { y + 21.0 * s }, root_w, h));
        let right = if compact { area.right() } else { root.map_or(area.right(), |rect| rect.x - gap) };
        let submit = Rect::new(right - submit_w, y + 21.0 * s, submit_w, h);
        let field = Rect::new(area.x, y, (submit.x - gap - area.x).max(0.0), 21.0 * s + h);
        let info_y = if compact { root.map_or(field.bottom(), |rect| rect.bottom()) } else { field.bottom() };
        let feedback = Rect::new(area.x, info_y + gap, area.w, if self.feedback.is_some() || self.busy { 24.0 * s } else { 0.0 });
        let list_y = feedback.bottom() + gap;
        let list = Rect::new(area.x, list_y, area.w, (area.bottom() - list_y).max(0.0));
        let pitch = 134.0 * s;
        let maximum = (rows.len() as f32 * pitch - list.h).max(0.0);
        let scroll = self.scroll.clamp(0.0, maximum);
        let scrollbar = (maximum > 0.0 && list.h > 0.0).then(|| {
            let h = (list.h * list.h / (list.h + maximum)).max(12.0 * s).min(list.h);
            Rect::new(list.right() - 3.0 * s, list.y + (list.h - h) * scroll / maximum, 3.0 * s, h)
        });
        let cards = rows.iter().enumerate().map(|(i, row)| {
            let rect = Rect::new(list.x, list.y + i as f32 * pitch - scroll, list.w - scrollbar.map_or(0.0, |_| gap), pitch - gap);
            let count = row.buttons.len().max(1) as f32;
            let button_w = (155.0 * s).min((rect.w - (count + 1.0) * gap) / count).max(0.0);
            let buttons = (0..row.buttons.len()).map(|j| {
                Rect::new(rect.x + gap + j as f32 * (button_w + gap), rect.bottom() - gap - h, button_w, h)
            }).collect();
            (rect, buttons)
        }).collect();
        Layout { tabs, field, submit, root, feedback, list, rows: cards, scrollbar, maximum }
    }

    pub fn empty_message(&self, has_index: bool) -> String {
        if !self.available { return t("No extensions are installed.").into(); }
        if !has_index { return t("Enter an HTTPS index URL to browse available extensions.").into(); }
        match &self.catalog {
            Catalog::Loading => t("Loading extension catalog…").into(),
            Catalog::Failed(error) => tf("Could not load extension catalog: {error}", &[("error", error)]),
            Catalog::Loaded(_) => t("No extensions match.").into(),
            Catalog::Empty => String::new(),
        }
    }
}

impl App {
    pub(crate) fn extension_rows(&self, page: &SettingsPage) -> Vec<Row> {
        let state = &page.extensions;
        if state.available {
            let Catalog::Loaded(entries) = &state.catalog else { return Vec::new() };
            let query = page.query.trim().to_lowercase();
            return entries.iter().filter(|entry| [&entry.id, &entry.name, &entry.author, &entry.description].iter()
                .any(|text| text.to_lowercase().contains(&query))).map(|entry| {
                let installed = self.extensions.installed.iter().find(|installed| installed.manifest.id == entry.id);
                let (label, enabled) = match installed {
                    Some(installed) if crate::update::newer(&entry.version, &installed.manifest.version) => {
                        (tf("Update to {version}", &[("version", &entry.version)]), true)
                    }
                    Some(_) => (t("Installed").into(), false),
                    None => (t("Install").into(), true),
                };
                Row {
                    title: entry.name.clone(), metadata: metadata(&entry.version, &entry.author), description: entry.description.clone(), status: String::new(), failed: false,
                    buttons: vec![Button::new(&label, enabled && !state.busy, Command::Request(Request::InstallCatalog(entry.clone())))],
                }
            }).collect();
        }
        let mut rows: Vec<_> = self.extensions.installed.iter().map(|installed| {
            let manifest = &installed.manifest;
            let id = &manifest.id;
            let disabled = self.config.extensions.disabled.contains(id);
            let mut description = manifest.description.clone();
            let status = match &installed.status {
                _ if disabled => t("Stopped").into(),
                Status::Stopped => t("Stopped").into(),
                Status::Starting => t("Starting…").into(),
                Status::Running => t("Running").into(),
                Status::Failed(error) => tf("Failed: {error}", &[("error", error)]),
            };
            let buttons = if state.confirm.as_ref() == Some(id) {
                description = tf("Remove {name} and delete its folder?", &[("name", &manifest.name)]);
                vec![
                    Button::new(t("Remove"), !state.busy, Command::Request(Request::Remove(id.clone()))),
                    Button::new(t("Cancel"), true, Command::Cancel),
                ]
            } else {
                vec![
                    Button::new(if disabled { t("Enable") } else { t("Disable") }, !state.busy && !self.config.read_only(), Command::Toggle(id.clone())),
                    Button::new(t("Remove"), !state.busy, Command::Confirm(id.clone())),
                    Button::new(t("Open folder"), !state.busy, Command::Request(Request::OpenFolder(Some(id.clone())))),
                ]
            };
            Row { title: manifest.name.clone(), metadata: metadata(&manifest.version, &manifest.author), description, status,
                failed: !disabled && matches!(installed.status, Status::Failed(_)), buttons }
        }).collect();
        rows.extend(self.extensions.errors.iter().map(|error| Row {
            title: error.directory.file_name().unwrap_or(error.directory.as_os_str()).to_string_lossy().into_owned(),
            description: tf("Failed: {error}", &[("error", &error.error)]), metadata: String::new(), status: String::new(), failed: true, buttons: Vec::new(),
        }));
        rows
    }

    pub(crate) fn visit_extensions_page(&mut self) {
        if self.settings.as_ref().is_some_and(|p| p.extensions_page() && p.extensions.available) {
            self.extension_index_changed();
        }
    }

    pub(crate) fn extension_index_changed(&mut self) {
        let Some(page) = self.settings.as_mut().filter(|page| page.extensions_page() && page.extensions.available) else { return };
        let url = self.config.extensions.index_url.trim().to_owned();
        page.extensions.input.clone_from(&url);
        page.extensions.catalog = if url.is_empty() { Catalog::Empty } else { Catalog::Loading };
        page.extensions.scroll = 0.0;
        if !url.is_empty() {
            self.effects.push(Effect::Extension(Request::Catalog(url)));
        }
    }

    pub fn extension_reply(&mut self, reply: Reply) {
        let page = self.settings.as_mut().or(self.settings_stash.as_mut());
        match reply {
            Reply::Catalog { url, result } if self.config.extensions.index_url.trim() == url => {
                if let Some(page) = page {
                    page.extensions.catalog = match result {
                        Ok(entries) => Catalog::Loaded(entries),
                        Err(error) => Catalog::Failed(error),
                    };
                }
            }
            Reply::Catalog { .. } => {}
            Reply::Operation(result) => {
                let message = match result {
                    Ok(()) => t("Extensions updated.").into(),
                    Err(error) => tf("Extension operation failed: {error}", &[("error", &error)]),
                };
                if let Some(page) = page {
                    page.extensions.busy = false;
                    page.extensions.feedback = Some(message);
                } else {
                    self.toast(message, TOAST);
                }
            }
        }
        self.dirty = true;
    }

    fn activate_extension_control(&mut self, target: Target) {
        let command = if let Target::Row(row, button) = target {
            self.settings.as_ref().and_then(|page| self.extension_rows(page).into_iter().nth(row))
                .and_then(|row| row.buttons.into_iter().nth(button)).filter(|button| button.enabled).map(|button| button.command)
        } else { None };
        let Some(page) = self.settings.as_mut() else { return };
        let state = &mut page.extensions;
        state.focus = Some(target);
        state.editing = target == Target::Field;
        state.replace = false;
        self.dirty = true;
        match target {
            Target::Tab(available) if available != state.available => {
                state.available = available;
                state.input.clear();
                state.scroll = 0.0;
                state.confirm = None;
                page.query.clear();
                self.visit_extensions_page();
            }
            Target::Submit if !state.busy => {
                let input = state.input.trim().to_owned();
                if state.available {
                    if self.config.read_only() { return self.warn_read_only_config(); }
                    if !input.is_empty() && !https_url(&input) {
                        state.feedback = Some(t("Enter an HTTPS address.").into());
                        return;
                    }
                    state.feedback = None;
                    if self.config.extensions.index_url != input {
                        let mut config = self.config.clone();
                        config.extensions.index_url = input;
                        self.set_config(config);
                        self.push_save_config();
                    } else {
                        self.extension_index_changed();
                    }
                } else if !input.is_empty() {
                    state.busy = true;
                    state.feedback = None;
                    self.effects.push(Effect::Extension(Request::InstallFile(input.trim_matches('"').into())));
                }
            }
            Target::Root if !state.busy => self.effects.push(Effect::Extension(Request::OpenFolder(None))),
            Target::Row(_, _) => match command {
                Some(Command::Confirm(id)) => {
                    state.confirm = Some(id);
                    if let Target::Row(row, _) = target { state.focus = Some(Target::Row(row, 1)); }
                }
                Some(Command::Cancel) => state.confirm = None,
                Some(Command::Request(request)) => {
                    if matches!(request, Request::InstallCatalog(_) | Request::Remove(_)) {
                        state.busy = true;
                        state.feedback = None;
                        state.confirm = None;
                    }
                    self.effects.push(Effect::Extension(request));
                }
                Some(Command::Toggle(id)) => {
                    let mut config = self.config.clone();
                    if config.extensions.disabled.contains(&id) { config.extensions.disabled.retain(|value| *value != id); }
                    else { config.extensions.disabled.push(id); }
                    self.set_config(config);
                    self.push_save_config();
                }
                None => {}
            },
            _ => {}
        }
    }

    pub(crate) fn extensions_text(&mut self, text: &str) -> bool {
        let Some(page) = self.settings.as_mut().filter(|page| page.extensions_page()) else { return false };
        if page.extensions.editing || !page.extensions.available {
            if std::mem::take(&mut page.extensions.replace) { page.extensions.input.clear(); }
            page.extensions.input.push_str(text);
            page.extensions.editing = true;
            page.extensions.focus = Some(Target::Field);
        } else {
            page.query.push_str(text);
            page.extensions.scroll = 0.0;
            page.extensions.focus = None;
        }
        self.dirty = true;
        true
    }

    pub(crate) fn extensions_key(&mut self, key: Key, mods: Mods) -> bool {
        let Some(page) = self.settings.as_mut().filter(|page| page.extensions_page()) else { return false };
        if mods.ctrl && matches!(key, Key::PageUp | Key::PageDown) { return false; }
        let state = &mut page.extensions;
        if key == Key::Tab || !state.editing && matches!(key, Key::Up | Key::Down) {
            self.move_extension_focus(mods.shift || key == Key::Up);
            return true;
        }
        match key {
            Key::Left | Key::Right if mods.ctrl || !state.editing => self.activate_extension_control(Target::Tab(key == Key::Right)),
            Key::PageUp | Key::PageDown => {
                if let Some(geo) = self.settings_geometry() {
                    let delta = geo.list.h * if key == Key::PageUp { -0.8 } else { 0.8 };
                    self.extensions_scroll(geo.list, delta);
                }
            }
            Key::Escape if state.confirm.take().is_some() => {}
            Key::Escape if state.editing || state.focus.is_some() || !page.query.is_empty() => {
                state.editing = false;
                state.focus = None;
                page.query.clear();
                state.scroll = 0.0;
            }
            Key::Escape => return false,
            Key::Enter if state.editing => self.activate_extension_control(Target::Submit),
            Key::Enter => {
                if let Some(target) = state.focus { self.activate_extension_control(target); }
            }
            Key::Backspace if state.editing => {
                if mods.ctrl || std::mem::take(&mut state.replace) { state.input.clear(); }
                else { state.input.pop(); }
            }
            Key::Backspace => {
                if mods.ctrl { page.query.clear(); } else { page.query.pop(); }
                state.scroll = 0.0;
                state.focus = None;
            }
            Key::Char('A') if mods.ctrl && state.editing => state.replace = true,
            Key::Char('V') if mods.ctrl => self.effects.push(Effect::RequestPaste),
            _ => {}
        }
        self.dirty = true;
        true
    }

    fn move_extension_focus(&mut self, reverse: bool) {
        let Some(page) = self.settings.as_ref() else { return };
        let rows = self.extension_rows(page);
        let mut targets = vec![Target::Tab(false), Target::Tab(true), Target::Field, Target::Submit];
        if !page.extensions.available { targets.push(Target::Root); }
        for (i, row) in rows.iter().enumerate() {
            targets.extend(row.buttons.iter().enumerate().filter(|(_, button)| button.enabled).map(|(j, _)| Target::Row(i, j)));
        }
        let current = targets.iter().position(|target| Some(*target) == page.extensions.focus);
        let index = current.map_or(if reverse { targets.len() - 1 } else { 0 }, |index| {
            if reverse { (index + targets.len() - 1) % targets.len() } else { (index + 1) % targets.len() }
        });
        let target = targets[index];
        let scale = self.ui();
        let geo = self.settings_geometry();
        let page = self.settings.as_mut().unwrap();
        let state = &mut page.extensions;
        state.focus = Some(target);
        state.editing = target == Target::Field;
        state.replace = false;
        if let (Target::Row(index, _), Some(geo)) = (target, geo) {
            let layout = state.layout(geo.list, scale, &rows);
            let rect = layout.rows[index].0;
            if rect.y < layout.list.y { state.scroll -= layout.list.y - rect.y; }
            if rect.bottom() > layout.list.bottom() { state.scroll += rect.bottom() - layout.list.bottom(); }
            state.scroll = state.scroll.clamp(0.0, layout.maximum);
        }
        self.dirty = true;
    }

    pub(crate) fn extensions_click(&mut self, area: Rect, x: f32, y: f32) -> bool {
        let Some(page) = self.settings.as_ref().filter(|page| page.extensions_page()) else { return false };
        let rows = self.extension_rows(page);
        let layout = page.extensions.layout(area, self.ui(), &rows);
        let mut target = [(Target::Tab(false), layout.tabs[0]), (Target::Tab(true), layout.tabs[1]), (Target::Field, layout.field), (Target::Submit, layout.submit)]
            .into_iter().find_map(|(target, rect)| rect.contains(x, y).then_some(target));
        if target.is_none() && layout.root.is_some_and(|rect| rect.contains(x, y)) { target = Some(Target::Root); }
        if target.is_none() && layout.list.contains(x, y) {
            target = layout.rows.iter().enumerate().find_map(|(i, (_, buttons))| {
                buttons.iter().position(|rect| rect.contains(x, y)).map(|j| Target::Row(i, j))
            });
        }
        if let Some(target) = target { self.activate_extension_control(target); }
        else { self.extension_search_focus(); }
        true
    }

    pub(crate) fn extension_search_focus(&mut self) {
        if let Some(page) = self.settings.as_mut().filter(|page| page.extensions_page()) {
            page.extensions.focus = None;
            page.extensions.editing = false;
            page.extensions.replace = false;
        }
    }

    pub(crate) fn extensions_scroll(&mut self, area: Rect, delta: f32) -> bool {
        let Some(page) = self.settings.as_ref().filter(|page| page.extensions_page()) else { return false };
        let maximum = page.extensions.layout(area, self.ui(), &self.extension_rows(page)).maximum;
        let state = &mut self.settings.as_mut().unwrap().extensions;
        state.scroll = (state.scroll + delta).clamp(0.0, maximum);
        self.dirty = true;
        true
    }
}

fn metadata(version: &str, author: &str) -> String {
    if author.is_empty() { version.to_owned() } else { format!("{version} · {author}") }
}
