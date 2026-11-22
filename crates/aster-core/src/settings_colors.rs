use crate::app::App;
use crate::config::{Appearance, Config};
use crate::input::{Key, Mods};
use crate::settings::SchemeRow;
use aster_gfx::render::Rect;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SchemeTab {
    #[default]
    Dark,
    Light,
}

impl SchemeTab {
    pub(crate) const ALL: [Self; 2] = [Self::Dark, Self::Light];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Dark => crate::i18n::t("Dark"),
            Self::Light => crate::i18n::t("Light"),
        }
    }

    pub(crate) fn name(self, appearance: &Appearance) -> &str {
        match self {
            Self::Dark => &appearance.color_scheme,
            Self::Light => &appearance.light_color_scheme,
        }
    }

    pub(crate) fn name_mut(self, appearance: &mut Appearance) -> &mut String {
        match self {
            Self::Dark => &mut appearance.color_scheme,
            Self::Light => &mut appearance.light_color_scheme,
        }
    }

    pub(crate) fn matches_setting(key: &str) -> bool {
        matches!(key, "appearance.color-scheme" | "appearance.light-color-scheme")
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SchemeBar {
    pub tabs: [Rect; 2],
    pub current: Rect,
    pub delete: Rect,
    pub cancel: Rect,
}

impl SchemeBar {
    pub(crate) fn new(area: Rect, scale: f32) -> Self {
        let gap = (6.0 * scale).round();
        let h = (30.0 * scale).round();
        let tab_w = (115.0 * scale).round().min((area.w - gap) / 2.0);
        let tabs = [Rect::new(area.x, area.y, tab_w, h), Rect::new(area.x + tab_w + gap, area.y, tab_w, h)];
        let w = tab_w.min((area.w * 0.27).round());
        let y = area.y + h + gap;
        let delete = Rect::new(area.right() - w, y, w, h);
        let cancel = Rect::new(delete.x - w - gap, y, w, h);
        let current = Rect::new(area.x + gap, y, (cancel.x - area.x - 2.0 * gap).max(0.0), h);
        Self { tabs, current, delete, cancel }
    }
}

fn replace_profile_scheme(value: &mut Option<String>, old: &str, new: Option<&str>) {
    if value.as_deref().is_some_and(|name| name.eq_ignore_ascii_case(old)) {
        *value = new.map(str::to_owned);
    }
}

fn replace_config_scheme(config: &mut Config, old: &str, new: Option<&str>) {
    let defaults = Appearance::default();
    for tab in SchemeTab::ALL {
        if tab.name(&config.appearance).eq_ignore_ascii_case(old) {
            *tab.name_mut(&mut config.appearance) = new.unwrap_or_else(|| tab.name(&defaults)).to_owned();
        }
    }
    for profile in &mut config.profiles {
        replace_profile_scheme(&mut profile.scheme, old, new);
    }
    for defaults in config.profile_defaults.values_mut().chain(config.groups.iter_mut().flat_map(|group| group.defaults.values_mut())) {
        replace_profile_scheme(&mut defaults.scheme, old, new);
    }
}

impl App {
    pub(crate) fn scheme_rows(&self) -> Vec<SchemeRow> {
        let rgb = |c: aster_vt::Rgb| u32::from_be_bytes([0, c.r, c.g, c.b]);
        let mut rows: Vec<_> = self.available_schemes().map(|(scheme, custom)| SchemeRow {
            name: scheme.name,
            custom,
            foreground: rgb(scheme.palette.foreground),
            background: rgb(scheme.palette.background),
            colors: scheme.palette.ansi.map(rgb),
        }).collect();
        rows.sort_by_cached_key(|r| (!r.custom, r.name.to_lowercase()));
        rows
    }

    pub(crate) fn sync_scheme_rows(&mut self) {
        if self.settings.is_none() && self.settings_stash.is_none() {
            return;
        }
        let rows = self.scheme_rows();
        for page in self.settings.iter_mut().chain(self.settings_stash.iter_mut()) {
            page.schemes.clone_from(&rows);
            page.refilter();
        }
    }

    pub(crate) fn select_scheme_tab(&mut self, tab: SchemeTab) {
        let name = tab.name(&self.config.appearance);
        let Some(page) = self.settings.as_mut() else { return };
        page.scheme_tab = tab;
        page.deleting_scheme = None;
        if let Some(at) = page.scheme_item(name).and_then(|item| page.shown.iter().position(|&i| i == item)) {
            page.selected = at;
        }
        self.reveal_selected();
    }

    pub(crate) fn apply_settings_scheme(&mut self, name: String) {
        if self.config.read_only() { return self.warn_read_only_config(); }
        let tab = self.settings.as_ref().filter(|p| p.colors_page()).map_or_else(|| self.active_scheme_tab(), |p| p.scheme_tab);
        if tab.name(&self.config.appearance) != name {
            let mut config = self.config.clone();
            *tab.name_mut(&mut config.appearance) = name;
            self.set_config(config);
            self.push_save_config();
        }
    }

    pub(crate) fn scheme_bar_click(&mut self, bar: SchemeBar, x: f32, y: f32) -> bool {
        let Some(page) = self.settings.as_ref() else { return false };
        if page.deleting_scheme.is_some() {
            if bar.delete.contains(x, y) {
                self.delete_settings_scheme();
            } else if bar.cancel.contains(x, y) {
                self.settings.as_mut().unwrap().deleting_scheme = None;
            }
            return true;
        }
        if let Some(k) = bar.tabs.iter().position(|r| r.contains(x, y)) {
            self.select_scheme_tab(SchemeTab::ALL[k]);
            return true;
        }
        if bar.delete.contains(x, y) {
            self.request_scheme_deletion();
            return true;
        }
        false
    }

    pub(crate) fn scheme_bar_key(&mut self, key: Key, mods: Mods) -> bool {
        let Some(page) = self.settings.as_mut().filter(|p| p.colors_page() && !p.editing()) else { return false };
        if page.deleting_scheme.is_some() {
            match key {
                Key::Enter if mods == Mods::NONE => self.delete_settings_scheme(),
                Key::Escape => page.deleting_scheme = None,
                _ => {}
            }
            return true;
        }
        match key {
            Key::Left if mods.alt => self.select_scheme_tab(SchemeTab::Dark),
            Key::Right if mods.alt => self.select_scheme_tab(SchemeTab::Light),
            Key::Delete if mods == Mods::NONE => self.request_scheme_deletion(),
            _ => return false,
        }
        true
    }

    fn request_scheme_deletion(&mut self) {
        let Some(page) = self.settings.as_mut() else { return };
        let name = page.scheme_tab.name(&self.config.appearance);
        page.deleting_scheme = page.scheme_item(name).and_then(|i| page.scheme_at(i)).filter(|r| r.custom).map(|r| r.name.clone());
    }

    fn delete_settings_scheme(&mut self) {
        let Some(name) = self.settings.as_mut().and_then(|p| p.deleting_scheme.take()) else { return };
        if self.config.read_only() { return self.warn_read_only_config(); }
        let mut config = self.config.clone();
        let count = config.color_schemes.len();
        config.color_schemes.retain(|scheme| !scheme.name.eq_ignore_ascii_case(&name));
        if count == config.color_schemes.len() {
            return;
        }
        replace_config_scheme(&mut config, &name, None);
        for pane in self.panes.values_mut() {
            replace_profile_scheme(&mut pane.profile.scheme, &name, None);
        }
        self.set_config(config);
        self.push_save_config();
    }
}
