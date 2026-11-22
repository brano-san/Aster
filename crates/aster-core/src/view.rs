use crate::app::{App, Drag, Effect, Metrics};
use crate::i18n::{t, tf};
use crate::input::HitArea;
use crate::overlay::{Overlay, OverlayKind, VISIBLE};
use crate::panes::PaneId;
use crate::theme::color;
use aster_gfx::render::{Color, Rect};
use aster_gfx::text::{FontFamily, FontStyle, Gfx, TextBackend};
use aster_vt::{CursorShape, Flags, Rgb, Size};

pub use crate::tabs::{bare_chrome, chrome, Chrome, TabSlot, TAB_H};
const PAD: f32 = 6.0;
const G_CLOSE: &str = "\u{E8BB}";
const G_PLUS: &str = "\u{E710}";
const G_MIN: &str = "\u{E921}";
const G_MAX: &str = "\u{E922}";
const G_RESTORE: &str = "\u{E923}";

#[derive(Clone, Copy)]
enum ProfileFieldControl {
    Text,
    Choice { editable: bool },
    Switch(bool),
}

#[derive(Clone, Copy, PartialEq)]
struct RunPaint {
    font: FontStyle,
    ink: Color,
    background: Rgb,
    flags: Flags,
    selected: bool,
    cursor: bool,
}

#[derive(Default)]
struct CellRun {
    text: String,
    origins: Vec<(usize, f32)>,
    paint: Option<RunPaint>,
    next_x: f32,
    baseline: f32,
}

impl CellRun {
    fn append<B: TextBackend>(&mut self, g: &mut Gfx<B>, cell: &aster_vt::Cell, x: f32, baseline: f32, width: f32, paint: RunPaint) {
        if !paint.font.ligatures && cell.zerowidth().is_none() {
            g.cell(x, baseline, cell.c, paint.font, paint.ink);
            return;
        }
        if self.paint != Some(paint) || self.baseline != baseline || self.next_x != x {
            self.flush(g);
        }
        self.paint = Some(paint);
        self.next_x = x + width;
        self.baseline = baseline;
        self.origins.push((self.text.len(), x));
        self.text.push(cell.c);
        if let Some(marks) = cell.zerowidth() {
            self.text.extend(marks);
        }
        if !paint.font.ligatures {
            self.flush(g);
        }
    }

    fn flush<B: TextBackend>(&mut self, g: &mut Gfx<B>) {
        if let Some(paint) = self.paint.take() {
            g.cell_run(&self.text, &self.origins, self.baseline, paint.font, paint.ink);
            self.text.clear();
            self.origins.clear();
        }
    }
}


pub fn pane_inner_with(r: Rect, s: f32, padding: f32) -> Rect {
    let p = (padding.clamp(0.0, 64.0) * s).round();
    r.inset(p, p)
}

pub fn pane_inner(r: Rect, s: f32) -> Rect {
    pane_inner_with(r, s, PAD)
}

fn rgb(c: Rgb) -> Color {
    color(c)
}

fn baseline<B: TextBackend>(g: &mut Gfx<B>, r: Rect, style: FontStyle) -> f32 {
    let m = g.metrics(style);
    (r.y + (r.h + m.ascent - m.descent) / 2.0).round()
}

fn wrapped_info<B: TextBackend>(g: &mut Gfx<B>, text: &str, style: FontStyle, width: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for mut line in text.lines() {
        while !line.is_empty() && g.measure(line, style) > width {
            let mut end = g.hit(line, style, width);
            if end > 0 && g.measure(&line[..end], style) > width {
                end = line[..end].char_indices().next_back().map_or(0, |(at, _)| at);
            }
            if end == 0 {
                end = line.chars().next().map_or(0, char::len_utf8);
            }
            let head = &line[..end];
            let split = head.rfind([' ', '/', '\\']).map_or(end, |at| at + 1);
            lines.push(line[..split].to_owned());
            line = &line[split..];
        }
        if !line.is_empty() {
            lines.push(line.to_owned());
        }
    }
    lines
}

fn centered<B: TextBackend>(g: &mut Gfx<B>, r: Rect, text: &str, style: FontStyle, c: Color) {
    let w = g.measure(text, style);
    let y = baseline(g, r, style);
    g.text((r.x + (r.w - w) / 2.0).round(), y, text, style, c, r.w);
}

fn tab_badge_style(s: f32) -> FontStyle {
    FontStyle::new(FontFamily::Ui, (8.5 * s).round(), true)
}

fn draw_tab_badge<B: TextBackend>(g: &mut Gfx<B>, r: Rect, label: &str, tint: u32, s: f32) {
    g.dl.round_rect(r, Color::rgb(tint), (3.0 * s).round());
    centered(g, r, label, tab_badge_style(s), Color::rgb(0xFFFFFF));
}

pub struct OverlayGeometry {
    pub frame: Rect,
    pub input: Rect,
    pub items: Vec<(usize, Rect)>,
    pub headers: Vec<(String, Rect)>,
    pub dividers: Vec<Rect>,
}

pub fn overlay_geometry(app: &App, o: &Overlay) -> OverlayGeometry {
    let s = app.ui();
    let content = app.chrome.content;
    let profiles = o.kind.is_profile_picker();
    let width = (if profiles { 520.0 } else { 560.0 } * s).min((content.w - 32.0 * s).max(200.0 * s)).min(content.w).round();
    let x = (content.x + (content.w - width) / 2.0).round();
    let y = content.y + (10.0 * s).round();
    let input_h = (if profiles { 44.0 } else { 40.0 } * s).round();
    let item_h = (if profiles { 34.0 } else { 30.0 } * s).round();
    let header_h = (30.0 * s).round();
    let pad = (6.0 * s).round();
    let n = o.shown.len().min(VISIBLE);
    let input = Rect::new(x, y, width, input_h);
    let mut cursor = y + input_h + pad;
    let mut items = Vec::new();
    let mut headers = Vec::new();
    let mut dividers = Vec::new();
    for k in 0..n {
        let position = o.scroll + k;
        if let Some(title) = o.header_before(position) {
            headers.push((title.to_owned(), Rect::new(x + pad, cursor, width - 2.0 * pad, header_h)));
            cursor += header_h;
        } else if k > 0 && o.items[o.shown[position].0].group.is_empty() && !o.items[o.shown[position - 1].0].group.is_empty() {
            let gap = (12.0 * s).round();
            dividers.push(Rect::new(x + pad, cursor + (gap / 2.0).round(), width - 2.0 * pad, (1.0 * s).round().max(1.0)));
            cursor += gap;
        }
        items.push((position, Rect::new(x + pad, cursor, width - 2.0 * pad, item_h)));
        cursor += item_h;
    }
    if n == 0 {
        cursor += item_h;
    }
    let frame = Rect::new(x, y, width, cursor + pad - y);
    OverlayGeometry { frame, input, items, headers, dividers }
}

pub fn overlay_hit(app: &App, o: &Overlay, x: f32, y: f32) -> Option<Option<usize>> {
    let geo = overlay_geometry(app, o);
    if !geo.frame.contains(x, y) {
        return None;
    }
    Some(geo.items.iter().find(|(_, r)| r.contains(x, y)).map(|(i, _)| *i))
}

impl App {
    fn metrics_for<B: TextBackend>(&self, g: &mut Gfx<B>) -> Metrics {
        let px = ((self.config.terminal.font_size + self.zoom).clamp(6.0, 48.0) * self.scale).round();
        let style = FontStyle::new(FontFamily::Mono, px, false);
        let fm = g.metrics(style);
        let adv = g.measure("M", style);
        let pad = self.config.terminal.line_padding * self.scale;
        let h = (fm.ascent + fm.descent + pad).round().max(1.0);
        let baseline = (fm.ascent + pad / 2.0).round();
        Metrics { w: adv.round().max(1.0), h, baseline, px }
    }

    pub fn frame<B: TextBackend>(&mut self, g: &mut Gfx<B>, maximized: bool) {
        self.prompt_pending_serial();
        g.set_mono_family(&self.config.terminal.font);
        g.set_fallback_family(&self.config.terminal.fallback_font);
        g.set_ui_family(&self.config.appearance.ui_font);
        let m = self.metrics_for(g);
        self.metrics = Some(m);
        let s = self.ui();
        self.chrome = self.tab_chrome();
        self.prepare_background_tabs(m);
        self.mark_seen();
        let progress = self.tab_progress(self.active);
        if progress != self.shown_progress {
            self.shown_progress = progress;
            self.effects.push(Effect::Progress(progress));
        }
        let theme = self.theme;
        g.begin(Color::rgba(0));
        if !self.tab_bar_hidden() {
            g.dl.rect(self.chrome.tab_bar, theme.chrome);
            if self.chrome.caption != self.chrome.tab_bar {
                g.dl.rect(self.chrome.caption, theme.chrome);
            }
            self.draw_tab_bar(g, maximized);
        }
        let content = self.chrome.content;
        g.dl.rect(content, theme.background);
        self.draw_profile_sidebar(g);
        let gap = (1.0 * s).round().max(1.0);
        let layout = match self.tabs.get(self.active) {
            Some(tab) => match tab.zoomed {
                Some(z) => crate::panes::Layout { panes: vec![(z, content)], splitters: Vec::new() },
                None => tab.root.layout(content, gap),
            },
            None => crate::panes::Layout::default(),
        };
        self.layout = layout.clone();
        let focus = self.focused_pane();
        let split = layout.panes.len() > 1;
        let broadcast = self.broadcast_targets();
        for (id, rect) in &layout.panes {
            self.draw_pane(g, *id, *rect, focus == Some(*id), m);
            if split && focus != Some(*id) {
                g.dl.rect(*rect, theme.dim);
            }
            if let Some(prompt) = self.password_prompt(*id).cloned() {
                let text = if prompt.offer {
                    Some(t("Save this password for the next prompt here? Ctrl+Shift+Enter saves it; any other key dismisses"))
                } else if prompt.can_fill() {
                    Some(t("A saved password matches this prompt. Ctrl+Shift+Enter types it"))
                } else {
                    None
                };
                if let Some(text) = text {
                    self.draw_hint(g, *rect, text);
                }
            }
            if broadcast.contains(id) {
                let text = if self.broadcast == Some(crate::app::Broadcast::AllTabs) { t("Typing goes to every tab. Click here to stop.") } else { t("Typing goes to every pane of this tab. Click here to stop.") };
                self.draw_hint(g, *rect, text);
            }
            if let Some((state, typed)) = self.offers.get(id).cloned() {
                self.draw_offer(g, *rect, state, &typed);
            }
            if self.bell_flash(*id) {
                g.dl.rect(*rect, theme.foreground.alpha(0x2C));
            }
            self.draw_pane_header(g, *id, *rect, focus == Some(*id));
        }
        for sp in &layout.splitters {
            g.dl.rect(sp.rect, theme.divider);
        }
        self.draw_pane_rearrangement(g);
        if let Some(dialog) = self.vault_prompt.as_ref().filter(|dialog| dialog.kind == crate::vault_settings::DialogKind::Startup) {
            g.dl.rect(content, theme.surface);
            self.draw_vault_dialog(g, content, dialog, false);
            self.mark_drawn();
            return;
        }
        if self.tabs.is_empty() {
            self.draw_start_page(g);
        }
        let content_layers = g.dl.instances.len();
        self.draw_search(g);
        self.draw_settings(g);
        self.draw_files(g);
        if let Some(o) = self.overlay.clone() {
            self.draw_overlay(g, &o);
        }
        self.draw_update_notice(g);
        self.draw_paste(g, m);
        self.draw_close(g);
        self.draw_tab_toast(g);
        if let Some(dialog) = &self.vault_prompt {
            self.draw_vault_dialog(g, content, dialog, false);
        }
        g.dl.confine(content_layers, content);
        self.draw_menu(g);
        self.draw_mru(g);
        self.mark_drawn();
    }

    fn draw_start_page<B: TextBackend>(&self, g: &mut Gfx<B>) {
        use crate::keymap::{format_binding, Action};
        use crate::start::Entry;

        let page = self.start_page();
        let s = self.ui();
        let ui = FontStyle::new(FontFamily::Ui, (14.0 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let first = page.row(0);
        let last = page.row(page.entries.len() - 1);
        let panel = Rect::new(first.x, first.y, first.w, last.bottom() - first.y).inset(-5.0 * s, -5.0 * s);
        g.dl.set_clip(Some(self.chrome.content));
        g.dl.round_rect(panel, self.theme.surface, 8.0 * s);
        g.dl.outline(panel, self.theme.overlay_border, 8.0 * s, s.max(1.0));
        for (i, entry) in page.entries.iter().enumerate() {
            let row = page.row(i);
            g.dl.set_clip(Some(row));
            if i == page.selected {
                g.dl.round_rect(row, self.theme.accent.alpha(0x38), 5.0 * s);
            } else if row.contains(self.mouse.0, self.mouse.1) && self.overlay.is_none() {
                g.dl.round_rect(row, self.theme.button_hover, 5.0 * s);
            }
            let side = (22.0 * s).round();
            let mark = Rect::new(row.x + 10.0 * s, row.y + ((row.h - side) / 2.0).round(), side, side);
            let (label, action, symbol) = match entry {
                Entry::NewTab => (t("New tab"), Action::NewTab, "+"),
                Entry::AllProfiles => (t("All profiles…"), Action::ProfileSelector, "…"),
                Entry::Settings => (t("Settings"), Action::Settings, "\u{2699}"),
                Entry::Recent(n) => {
                    let p = &self.profiles[*n];
                    let (badge, tint) = crate::picker::badge(&crate::profile::effective(p, &self.config));
                    g.dl.round_rect(mark, Color::rgb(tint), 5.0 * s);
                    let px = if badge.chars().count() > 2 { 8.5 } else { 10.0 };
                    centered(g, mark, &badge, FontStyle::new(FontFamily::Ui, (px * s).round(), true), Color::rgb(0xFFFFFF));
                    (p.name.as_str(), Action::Tab(i as u8), "")
                }
            };
            if !symbol.is_empty() {
                centered(g, mark, symbol, ui, self.theme.text_dim);
            }
            let x = mark.right() + 10.0 * s;
            let mut right = row.right() - 10.0 * s;
            if let Some(hint) = self.keymap.bindings_of(&action).next().map(format_binding) {
                let w = g.measure(&hint, small);
                if right - x - w >= 90.0 * s {
                    let y = baseline(g, row, small);
                    g.text(right - w, y, &hint, small, self.theme.text_dim, w + 1.0);
                    right -= w + 10.0 * s;
                }
            }
            let y = baseline(g, row, ui);
            g.text(x, y, label, ui, self.theme.text, (right - x).max(0.0));
        }
        g.dl.set_clip(None);
    }

    fn draw_profile_sidebar<B: TextBackend>(&mut self, g: &mut Gfx<B>) {
        use crate::profile_sidebar::Focus;
        use crate::profiles_page::TreeKind;
        let Some(geo) = self.sidebar_geometry() else { return };
        let s = self.ui();
        let theme = self.theme;
        let (mx, my) = self.mouse;
        let line = (1.0 * s).round().max(1.0);
        let ui = FontStyle::new(FontFamily::Ui, (12.0 * s).round(), false);
        let bold = FontStyle::new(FontFamily::Ui, ui.px(), true);
        let radius = (5.0 * s).round();
        let visible = geo.visible();
        let max_scroll = self.profile_sidebar.rows.len().saturating_sub(visible);
        self.profile_sidebar.scroll = self.profile_sidebar.scroll.min(max_scroll);
        let browser = &self.profile_sidebar;
        let focused = browser.focus == Focus::Filter;
        g.dl.set_clip(Some(geo.rect));
        g.dl.rect(geo.rect, theme.surface);
        g.dl.round_rect(geo.filter, theme.overlay, radius);
        g.dl.outline(geo.filter, if focused { theme.accent } else { theme.overlay_border }, radius, line);
        let x = geo.filter.x + (8.0 * s).round();
        let y = baseline(g, geo.filter, ui);
        let width = (geo.filter.right() - x - 8.0 * s).max(0.0);
        let composing = focused && !self.preedit.is_empty();
        let empty = browser.query.is_empty() && !composing;
        let w = if empty {
            g.text(x, y, t("Filter profiles"), ui, theme.text_dim, width);
            0.0
        } else {
            let w = g.text(x, y, &browser.query, ui, theme.text, width);
            if composing {
                g.text(x + w, y, &self.preedit.text, ui, theme.text, (width - w).max(0.0));
                let cursor = self.preedit.cursor.unwrap_or(self.preedit.text.len());
                w + g.measure(&self.preedit.text[..cursor], ui)
            } else {
                w
            }
        };
        let caret = focused.then(|| Rect::new(x + w.min(width), geo.filter.y + (8.0 * s).round(), line, (14.0 * s).round()));
        if let Some(caret) = caret {
            g.dl.rect(caret, theme.accent);
        }
        let resizing = matches!(self.drag, Drag::SidebarResize { .. });
        let divider = Rect::new(geo.rect.right() - line, geo.rect.y, line, geo.rect.h);
        g.dl.rect(divider, if resizing || geo.grip.contains(mx, my) { theme.accent } else { theme.divider });
        for (position, row) in browser.rows.iter().skip(browser.scroll).take(visible).enumerate() {
            let r = geo.row(position);
            let selected = browser.selected == browser.scroll + position && browser.focus != Focus::None;
            if selected || r.contains(mx, my) {
                g.dl.round_rect(r, if selected { theme.accent.alpha(0x30) } else { theme.button_hover }, radius);
            }
            let x = r.x + 8.0 * s + row.indent(s).min((r.w - 44.0 * s).max(0.0));
            let end = r.right() - 7.0 * s;
            match &row.kind {
                TreeKind::Header(header) => {
                    let open = !browser.query.trim().is_empty() || !browser.collapsed.contains(header);
                    g.dl.chevron((x + 4.0 * s, r.y + r.h * 0.5), 7.0 * s, if open { 90.0 } else { 0.0 }, 1.5 * s, theme.text_dim);
                    let mut x = x + 16.0 * s;
                    if let Some(group) = header.group_index(&self.config.groups).map(|i| &self.config.groups[i]) {
                        x = self.draw_profile_group_mark(g, r, x, group, bold);
                    }
                    let y = baseline(g, r, bold);
                    g.text(x, y, header.label(&self.config.groups), bold, theme.text, (end - x).max(0.0));
                }
                TreeKind::Profile(i) => {
                    let profile = &browser.profiles[*i];
                    let badge = Rect::new(x, r.y + ((r.h - 16.0 * s) / 2.0).round(), (22.0 * s).round(), (16.0 * s).round());
                    draw_tab_badge(g, badge, &profile.badge, profile.tint, s);
                    let x = badge.right() + (7.0 * s).round();
                    let y = baseline(g, r, ui);
                    g.text(x, y, &profile.name, ui, theme.text, (end - x).max(0.0));
                }
                _ => {}
            }
        }
        if browser.rows.is_empty() && visible > 0 {
            let r = geo.row(0);
            let y = baseline(g, r, ui);
            g.text(x, y, t("Nothing matches the filter"), ui, theme.text_dim, width);
        }
        if max_scroll > 0 && visible > 0 {
            let height = (geo.list.h * visible as f32 / browser.rows.len() as f32).max(12.0 * s);
            let y = geo.list.y + (geo.list.h - height) * browser.scroll as f32 / max_scroll as f32;
            g.dl.round_rect(Rect::new(geo.rect.right() - 4.0 * s, y, 2.0 * s, height), theme.text_dim.alpha(0x80), s);
        }
        g.dl.set_clip(None);
        if let Some(caret) = caret {
            self.set_ime_area(caret);
        }
    }

    fn draw_pane_rearrangement<B: TextBackend>(&self, g: &mut Gfx<B>) {
        let s = self.ui();
        let theme = self.theme;
        let line = (1.0 * s).round().max(1.0);
        if let Some(state) = self.rearrange.as_ref().filter(|_| self.rearranging()) {
            let count = self.layout.panes.len();
            let ui = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), true);
            for (i, (pane, rect)) in self.layout.panes.iter().enumerate() {
                let rect = self.pane_terminal_rect(*pane, *rect);
                let label = crate::rearrange::pane_label(i, count);
                let w = (g.measure(&label, ui) + 16.0 * s).round();
                let badge = Rect::new(rect.x + 10.0 * s, rect.y + 10.0 * s, w, (24.0 * s).round());
                let selected = state.first == Some(*pane);
                g.dl.set_clip(Some(rect));
                g.dl.round_rect(badge, if selected { theme.accent } else { theme.overlay }, (5.0 * s).round());
                g.dl.outline(badge, theme.accent, (5.0 * s).round(), line);
                centered(g, badge, &label, ui, if selected { Color::rgb(0xFFFFFF) } else { theme.text });
            }
            g.dl.set_clip(None);
        }
        if let Some((rect, active)) = self.pane_drag_hover() {
            self.draw_drop_zones(g, rect, active, true);
        } else if let Some((rect, active)) = self.tab_drag_hover() {
            self.draw_drop_zones(g, rect, active, false);
        }
    }

    fn draw_drop_zones<B: TextBackend>(&self, g: &mut Gfx<B>, rect: Rect, active: crate::rearrange::DropZone, center_swap: bool) {
        use crate::panes::Side;
        use crate::rearrange::DropZone;
        let s = self.ui();
        let theme = self.theme;
        let line = (1.0 * s).round().max(1.0);
        let ui = FontStyle::new(FontFamily::Ui, (18.0 * s).round(), false);
        g.dl.set_clip(Some(rect));
        for (zone, area) in crate::rearrange::drop_zones(rect) {
            let r = area.inset(line, line);
            let chosen = zone == active;
            g.dl.round_rect(r, if chosen { theme.accent.alpha(0x78) } else { theme.overlay.alpha(0xC8) }, (4.0 * s).round());
            g.dl.outline(r, if chosen { theme.accent } else { theme.overlay_border }, (4.0 * s).round(), line);
            let ink = if chosen { Color::rgb(0xFFFFFF) } else { theme.text_dim };
            if r.w < 18.0 * s || r.h < 18.0 * s {
                continue;
            }
            let angle = match zone {
                DropZone::Side(Side::Left) => 180.0,
                DropZone::Side(Side::Right) => 0.0,
                DropZone::Side(Side::Top) => 270.0,
                DropZone::Side(Side::Bottom) => 90.0,
                DropZone::Center => {
                    centered(g, r, if center_swap { "↔" } else { "+" }, ui, ink);
                    continue;
                }
            };
            g.dl.chevron((r.x + r.w / 2.0, r.y + r.h / 2.0), 12.0 * s, angle, 1.5 * s, ink);
        }
        g.dl.set_clip(None);
    }

    fn draw_tab_bar<B: TextBackend>(&mut self, g: &mut Gfx<B>, maximized: bool) {
        let s = self.ui();
        let theme = self.theme;
        let c = self.chrome.clone();
        let ui = FontStyle::new(FontFamily::Ui, (12.5 * s).round(), false);
        let icons = FontStyle::new(FontFamily::Icons, (10.0 * s).round(), false);
        let (mx, my) = self.mouse;
        let radius = (8.0 * s).round();
        for (i, slot) in c.tabs.iter().enumerate() {
            let r = slot.rect;
            if r.w <= 0.0 || r.h <= 0.0 {
                continue;
            }
            g.dl.set_clip(Some(r));
            let active = i == self.active;
            let hovered = slot.rect.contains(mx, my);
            let dragging = matches!(self.drag, Drag::Tab { index, moved: true, .. } if index == i);
            if active || dragging {
                let shape = match self.config.appearance.tab_bar_position {
                    crate::config::TabBarPosition::Top => Rect::new(r.x, r.y, r.w, r.h + radius),
                    crate::config::TabBarPosition::Bottom => Rect::new(r.x, r.y - radius, r.w, r.h + radius),
                    _ => r.inset(0.0, (2.0 * s).round()),
                };
                g.dl.round_rect(shape, theme.tab_active, radius);
            } else if hovered {
                g.dl.round_rect(Rect::new(r.x + 2.0 * s, r.y + 2.0 * s, r.w - 4.0 * s, r.h - 6.0 * s), theme.tab_hover, (6.0 * s).round());
            } else if !c.vertical && i + 1 != self.active && i + 1 < c.tabs.len() {
                let lh = (16.0 * s).round();
                g.dl.rect(Rect::new(r.right() - 1.0, r.y + ((r.h - lh) / 2.0).round(), (1.0 * s).max(1.0), lh), theme.divider);
            }
            if let Some(ink) = self.tab_color(i) {
                g.dl.round_rect(Rect::new(r.x + (10.0 * s).round(), r.y + (3.0 * s).round(), r.w - (20.0 * s).round(), (3.0 * s).round().max(2.0)), ink, (1.5 * s).round());
            }
            let show_close = slot.close.w > 0.0 && (active || hovered);
            let text_x = r.x + (12.0 * s).round();
            let text_end = if show_close { slot.close.x - 4.0 * s } else { r.right() - 12.0 * s };
            let y = baseline(g, r, ui);
            match self.rename.as_ref().filter(|n| n.tab == i) {
                Some(edit) => {
                    let field = Rect::new(text_x - (5.0 * s).round(), r.y + (5.0 * s).round(), (text_end - text_x + 8.0 * s).max(0.0).round(), r.h - (10.0 * s).round());
                    g.dl.round_rect(field, theme.surface, (4.0 * s).round());
                    g.dl.outline(field, theme.accent, (4.0 * s).round(), (1.0 * s).round().max(1.0));
                    let w = g.text(text_x, y, &edit.text, ui, theme.text, (text_end - text_x).max(0.0));
                    let caret_h = (14.0 * s).round();
                    g.dl.rect(Rect::new(text_x + w + 1.0, field.y + ((field.h - caret_h) / 2.0).round(), (1.0 * s).round().max(1.0), caret_h), theme.accent);
                }
                None if self.tabs[i].pinned => self.draw_pinned_tab(g, i, r, ui),
                None => {
                    let title = self.tab_label(i);
                    let mut x = text_x;
                    if self.tab_activity(i) {
                        let d = (6.0 * s).round();
                        g.dl.circle(Rect::new(x, r.y + ((r.h - d) / 2.0).round(), d, d), theme.accent);
                        x += d + (6.0 * s).round();
                    }
                    if let Some((label, tint)) = self.tab_badge(i) {
                        let style = tab_badge_style(s);
                        let w = (g.measure(&label, style) + 6.0 * s).round();
                        if x + w + 18.0 * s <= text_end {
                            let badge = Rect::new(x, r.y + ((r.h - 16.0 * s) / 2.0).round(), w, (16.0 * s).round());
                            draw_tab_badge(g, badge, &label, tint, s);
                            x = badge.right() + (5.0 * s).round();
                        }
                    }
                    g.text(x, y, &title, ui, if active { theme.text } else { theme.text_dim }, (text_end - x).max(0.0));
                }
            }
            if let Some(progress) = self.tab_progress(i) {
                use aster_vt::ProgressState;
                let track = Rect::new(r.x + (10.0 * s).round(), r.bottom() - (4.0 * s).round(), r.w - (20.0 * s).round(), (2.0 * s).round().max(1.0));
                let (fraction, ink) = match progress.state {
                    ProgressState::Normal => (f32::from(progress.percent) / 100.0, theme.accent),
                    ProgressState::Error => (1.0, theme.close_hover),
                    ProgressState::Paused => (f32::from(progress.percent) / 100.0, theme.text_dim),
                    ProgressState::Indeterminate => (1.0, theme.accent.alpha(0x80)),
                };
                g.dl.rect(track, theme.divider);
                g.dl.rect(Rect::new(track.x, track.y, (track.w * fraction).round(), track.h), ink);
            }
            if show_close {
                if slot.close.contains(mx, my) {
                    g.dl.round_rect(slot.close, theme.button_hover, (4.0 * s).round());
                }
                centered(g, slot.close, G_CLOSE, FontStyle::new(FontFamily::Icons, (8.0 * s).round(), false), if active { theme.text } else { theme.text_dim });
            }
        }
        g.dl.set_clip(Some(c.tab_bar));
        if c.plus.contains(mx, my) {
            g.dl.round_rect(c.plus, theme.button_hover, (6.0 * s).round());
        }
        centered(g, c.plus, G_PLUS, icons, theme.text_dim);
        if c.tab_menu.w > 0.0 && c.tab_menu.h > 0.0 {
            if c.tab_menu.contains(mx, my) {
                let fill = if self.drag == Drag::TabMenu { theme.button_press } else { theme.button_hover };
                g.dl.round_rect(c.tab_menu, fill, (6.0 * s).round());
            }
            g.dl.chevron((c.tab_menu.x + c.tab_menu.w / 2.0, c.tab_menu.y + c.tab_menu.h / 2.0), 8.0 * s, 90.0, 1.5 * s, theme.text_dim);
        }
        g.dl.set_clip(Some(c.caption));
        for (r, glyph, area) in [(c.min, G_MIN, HitArea::Minimize), (c.max, if maximized { G_RESTORE } else { G_MAX }, HitArea::Maximize), (c.close, G_CLOSE, HitArea::Close)] {
            let hot = self.hover == area;
            let pressed = self.drag == Drag::Button(area);
            let mut ink = theme.text;
            if area == HitArea::Close && (hot || pressed) {
                g.dl.rect(r, theme.close_hover);
                ink = Color::rgb(0xFFFFFF);
            } else if pressed {
                g.dl.rect(r, theme.button_press);
            } else if hot {
                g.dl.rect(r, theme.button_hover);
            }
            centered(g, r, glyph, icons, ink);
        }
        g.dl.set_clip(None);
    }

    fn draw_pinned_tab<B: TextBackend>(&self, g: &mut Gfx<B>, index: usize, r: Rect, ui: FontStyle) {
        let s = self.ui();
        let number = self.tab_label(index);
        let ink = if self.active == index { self.theme.text } else { self.theme.text_dim };
        let mut dot = (r.right() - 8.0 * s, r.y + 7.0 * s);
        if let Some((label, tint)) = self.tab_badge(index) {
            let number_w = g.measure(&number, ui);
            let gap = if number.is_empty() { 0.0 } else { (3.0 * s).round() };
            let badge_w = (g.measure(&label, tab_badge_style(s)) + 6.0 * s).round().min((r.w - number_w - gap).max(0.0));
            let x = (r.x + (r.w - number_w - gap - badge_w) / 2.0).round();
            let badge = Rect::new(x + number_w + gap, r.y + ((r.h - 16.0 * s) / 2.0).round(), badge_w, (16.0 * s).round());
            let y = baseline(g, r, ui);
            g.text(x, y, &number, ui, ink, number_w + 1.0);
            draw_tab_badge(g, badge, &label, tint, s);
            dot = (badge.right(), badge.y);
        } else {
            let text = if number.is_empty() { self.tab_title(index) } else { number };
            let y = baseline(g, r, ui);
            let width = (r.w - 8.0 * s).max(0.0);
            let w = g.measure(&text, ui).min(width);
            g.text((r.x + (r.w - w) / 2.0).round(), y, &text, ui, ink, width);
        }
        if self.tab_activity(index) {
            let diameter = (6.0 * s).round();
            g.dl.circle(Rect::new(dot.0 - diameter / 2.0, dot.1 - diameter / 2.0, diameter, diameter), self.theme.accent);
        }
    }

    fn tab_badge(&self, index: usize) -> Option<(String, u32)> {
        if !self.config.appearance.tab_icon {
            return None;
        }
        self.panes.get(&self.tabs[index].focus).map(|pane| crate::picker::badge(&pane.profile))
    }

    fn draw_profile_group_mark<B: TextBackend>(&self, g: &mut Gfx<B>, rect: Rect, x: f32, group: &crate::profile::ProfileGroup, font: FontStyle) -> f32 {
        let s = self.ui();
        let tint = group.color.as_deref().and_then(aster_vt::schemes::parse_hex).map(color);
        if let Some(icon) = group.icon.as_deref().filter(|v| !v.is_empty()) {
            let y = baseline(g, rect, font);
            x + g.text(x, y, icon, font, tint.unwrap_or(self.theme.text_dim), 32.0 * s) + 7.0 * s
        } else if let Some(ink) = tint {
            g.dl.circle(Rect::new(x, rect.y + (rect.h - 9.0 * s) * 0.5, 9.0 * s, 9.0 * s), ink);
            x + 16.0 * s
        } else {
            x
        }
    }

    fn prepare_pane(&mut self, id: PaneId, size: Size, m: Metrics) {
        let Some(pane) = self.panes.get_mut(&id) else { return };
        if let Some(mut launch) = pane.pending.take_if(|launch| launch.baud_placeholder().is_none()) {
            launch.cols = size.cols as u16;
            launch.rows = size.rows as u16;
            {
                let mut t = pane.terminal.lock();
                t.resize(size);
                t.set_cell_pixels(m.w as u16, m.h as u16);
            }
            pane.size = size;
            if pane.profile.exit_decision(None) == crate::profile::ExitDecision::Reconnect {
                pane.retry_launch = Some(Box::new(launch.clone()));
            }
            self.effects.push(Effect::Spawn { pane: id, launch, terminal: pane.terminal.clone(), reconnecting: false });
            if !pane.queued.is_empty() {
                self.effects.push(Effect::Write { pane: id, bytes: std::mem::take(&mut pane.queued) });
            }
        } else if pane.size != size {
            {
                let mut t = pane.terminal.lock();
                t.resize(size);
                t.set_cell_pixels(m.w as u16, m.h as u16);
            }
            pane.size = size;
            if pane.exited.is_none() && pane.pending.is_none() {
                self.effects.push(Effect::Resize { pane: id, cols: size.cols as u16, rows: size.rows as u16 });
            }
        }
    }

    fn prepare_background_tabs(&mut self, m: Metrics) {
        let content = self.chrome.content;
        let s = self.ui();
        let gap = (1.0 * s).round().max(1.0);
        for id in std::mem::take(&mut self.background_launches) {
            let Some(pending) = self.panes.get(&id).and_then(|p| p.pending.as_ref()) else { continue };
            if pending.baud_placeholder().is_some() {
                self.background_launches.push(id);
                continue;
            }
            let Some(tab) = self.tabs.iter().find(|t| t.root.contains(id)) else { continue };
            if let Some((_, rect)) = tab.root.layout(content, gap).panes.into_iter().find(|(p, _)| *p == id) {
                let inner = pane_inner_with(self.pane_terminal_rect(id, rect), s, self.config.terminal.padding);
                let size = Size::new((inner.w / m.w).floor().max(2.0) as usize, (inner.h / m.h).floor().max(1.0) as usize);
                self.prepare_pane(id, size, m);
            }
        }
    }

    fn draw_pane<B: TextBackend>(&mut self, g: &mut Gfx<B>, id: PaneId, rect: Rect, focused: bool, m: Metrics) {
        let s = self.ui();
        let rect = self.pane_terminal_rect(id, rect);
        let inner = pane_inner_with(rect, s, self.config.terminal.padding);
        let size = Size::new((inner.w / m.w).floor().max(2.0) as usize, (inner.h / m.h).floor().max(1.0) as usize);
        self.prepare_pane(id, size, m);
        let window_focused = self.terminal_has_focus();
        let blink_on = self.blink_on || !self.cursor_blink_enabled();
        let palette = self.pane_palette(id);
        let Some(pane) = self.panes.get_mut(&id) else { return };
        let theme = self.theme;
        let terminal = pane.terminal.clone();
        let mut t = terminal.lock();
        let searching = self.search.as_ref().is_some_and(|s| s.pane == id);
        let (matches, marks) = if searching { (t.visible_matches(), t.search_marks()) } else { (Vec::new(), Vec::new()) };
        if searching {
            if let (Some(bar), Some(status)) = (self.search.as_mut(), t.search_status()) {
                bar.current = status.current;
                bar.count = status.count;
                bar.missing = !bar.query.is_empty() && status.count == 0;
            }
        }
        let overrides = *t.colors();
        let default_bg = palette.background(&overrides);
        if default_bg != self.palette.background {
            g.dl.rect(rect, rgb(default_bg));
        }
        g.dl.set_clip(Some(rect));
        let (ox, oy) = (inner.x, inner.y);
        let cell_rect = |line: usize, col: usize, cols: usize| Rect::new(ox + col as f32 * m.w, oy + line as f32 * m.h, m.w * cols as f32, m.h);
        let selection_bg = palette.selection;
        t.visible(|v| {
            if v.cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                return;
            }
            let (_, bg) = palette.cell(v.cell, &overrides);
            let bg = if v.selected { selection_bg } else { bg };
            if bg != default_bg {
                let span = if v.cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
                g.dl.rect(cell_rect(v.line, v.col, span), rgb(bg));
            }
        });
        let dpi = self.scale;
        for mt in &matches {
            let r = cell_rect(mt.line, mt.start, mt.end.saturating_sub(mt.start).max(1));
            if mt.current {
                g.dl.rect(r, rgb(selection_bg));
                g.dl.outline(r, Color::rgb(0xF0A030), 0.0, (1.0 * dpi).round().max(1.0));
            } else {
                g.dl.rect(r, Color::rgb(0xE5C07B).alpha(0x58));
            }
        }
        let cursor = t.cursor();
        let show_cursor = cursor.shape != CursorShape::Hidden;
        let cursor_color = overrides[aster_vt::NamedColor::Cursor].unwrap_or(palette.cursor);
        let cur_cols = if cursor.wide { 2 } else { 1 };
        let cr = cell_rect(cursor.line, cursor.col, cur_cols);
        let active_cursor = focused && window_focused;
        let draw_cursor = show_cursor && (!active_cursor || (blink_on && self.preedit.is_empty()));
        let thin = (1.0 * dpi).round().max(1.0);
        let block = draw_cursor && active_cursor && matches!(cursor.shape, CursorShape::Block);
        if draw_cursor {
            match (active_cursor, cursor.shape) {
                (true, CursorShape::Block) => g.dl.rect(cr, rgb(cursor_color)),
                (true, CursorShape::Beam) => g.dl.rect(Rect::new(cr.x, cr.y, (2.0 * dpi).round().max(1.0), cr.h), rgb(cursor_color)),
                (true, CursorShape::Underline) => g.dl.rect(Rect::new(cr.x, cr.bottom() - (2.0 * dpi).round().max(1.0), cr.w, (2.0 * dpi).round().max(1.0)), rgb(cursor_color)),
                _ => g.dl.outline(cr, rgb(cursor_color), 0.0, thin),
            }
        }
        let px = m.px;
        let cursor_text = palette.cursor_text.unwrap_or(default_bg);
        let selection_text = palette.selection_text;
        let min_contrast = self.config.terminal.minimum_contrast_ratio;
        let under_y = (m.baseline + (m.h - m.baseline) * 0.35).round().min(m.h - thin);
        let ligatures = self.config.terminal.ligatures;
        let mut run = CellRun::default();
        let mut decorations = Vec::new();
        t.visible(|v| {
            let flags = v.cell.flags;
            if flags.contains(Flags::WIDE_CHAR_SPACER) {
                return;
            }
            let (mut fg, bg) = palette.cell(v.cell, &overrides);
            if v.selected {
                if let Some(st) = selection_text {
                    fg = st;
                }
            }
            let at_cursor = block && v.line == cursor.line && v.col == cursor.col;
            if at_cursor {
                fg = cursor_text;
            }
            let x = ox + v.col as f32 * m.w;
            let y = oy + v.line as f32 * m.h;
            let c = v.cell.c;
            if (c != ' ' || v.cell.zerowidth().is_some()) && c != '\t' && !flags.contains(Flags::HIDDEN) {
                let mut backdrop = if v.selected { selection_bg } else { bg };
                if min_contrast > 1.0 && !matches.is_empty() {
                    let i = matches.partition_point(|mt| (mt.line, mt.end) <= (v.line, v.col));
                    if let Some(mt) = matches.get(i).filter(|mt| mt.line == v.line && mt.start <= v.col) {
                        backdrop = if mt.current { selection_bg } else { crate::theme::mix(backdrop, Rgb { r: 0xe5, g: 0xc0, b: 0x7b }, 0x58 as f32 / 255.0) };
                    }
                }
                let backdrop = if at_cursor { cursor_color } else { backdrop };
                let ink = rgb(crate::theme::ensure_contrast(fg, backdrop, min_contrast));
                if ligatures && aster_gfx::boxdraw::is_procedural(c) {
                    run.flush(g);
                }
                if !g.cell_box(x, y, c, m.w as u32, m.h as u32, ink) {
                    let mut font = FontStyle::new(FontFamily::Mono, px, flags.contains(Flags::BOLD)).italic(flags.contains(Flags::ITALIC));
                    font.ligatures = ligatures;
                    let paint = RunPaint { font, ink, background: backdrop, flags: flags - (Flags::WIDE_CHAR | Flags::WRAPLINE), selected: v.selected, cursor: at_cursor };
                    let width = if flags.contains(Flags::WIDE_CHAR) { 2.0 * m.w } else { m.w };
                    run.append(g, v.cell, x, y + m.baseline, width, paint);
                }
            }
            if flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT) {
                let span = if flags.contains(Flags::WIDE_CHAR) { 2.0 } else { 1.0 };
                let w = m.w * span;
                let ucolor = v.cell.underline_color().map(|c| palette.resolve(c, &overrides)).unwrap_or(fg);
                decorations.push((x, y, w, flags, ucolor, fg));
            }
        });
        run.flush(g);
        let offset = t.display_offset();
        let history = t.history();
        drop(t);
        for (x, y, w, flags, ucolor, fg) in decorations {
            let uy = y + under_y;
            if flags.contains(Flags::UNDERCURL) {
                let band = (3.0 * dpi).round().max(3.0);
                g.dl.undercurl(Rect::new(x, uy - band / 2.0, w, band), rgb(ucolor), thin);
            } else if flags.contains(Flags::DOUBLE_UNDERLINE) {
                g.dl.rect(Rect::new(x, uy - thin, w, thin), rgb(ucolor));
                g.dl.rect(Rect::new(x, uy + thin, w, thin), rgb(ucolor));
            } else if flags.contains(Flags::DOTTED_UNDERLINE) || flags.contains(Flags::DASHED_UNDERLINE) {
                let dash = if flags.contains(Flags::DOTTED_UNDERLINE) { thin } else { (3.0 * dpi).round() };
                let mut dx = 0.0;
                while dx < w {
                    g.dl.rect(Rect::new(x + dx, uy, dash.min(w - dx), thin), rgb(ucolor));
                    dx += dash * 2.0;
                }
            } else if flags.contains(Flags::UNDERLINE) {
                g.dl.rect(Rect::new(x, uy, w, thin), rgb(ucolor));
            }
            if flags.contains(Flags::STRIKEOUT) {
                g.dl.rect(Rect::new(x, y + (m.baseline * 0.62).round(), w, thin), rgb(fg));
            }
        }
        if let Some(link) = self.hover_link.as_ref().filter(|l| l.pane == id) {
            let x = ox + link.start as f32 * m.w;
            let w = (link.end.saturating_sub(link.start)) as f32 * m.w;
            g.dl.rect(Rect::new(x, oy + link.line as f32 * m.h + under_y, w, thin), theme.accent);
        }
        if focused && self.pane_rename.is_none() && !self.sidebar_focused() && !self.preedit.is_empty() {
            let style = FontStyle::new(FontFamily::Mono, px, false);
            let w = g.measure(&self.preedit.text, style);
            let r = Rect::new(cr.x, cr.y, w.max(m.w), m.h);
            g.dl.rect(r, theme.surface);
            g.text(r.x, r.y + m.baseline, &self.preedit.text, style, theme.foreground, f32::MAX);
            g.dl.rect(Rect::new(r.x, r.y + under_y, w, thin), theme.foreground);
        }
        if offset > 0 && history > 0 {
            let total = (history + size.rows) as f32;
            let track = rect.inset(0.0, (4.0 * s).round());
            let th = (track.h * size.rows as f32 / total).max(24.0 * s);
            let ty = track.y + (track.h - th) * (1.0 - offset as f32 / history as f32);
            let bw = (4.0 * s).round();
            g.dl.round_rect(Rect::new(track.right() - bw - 2.0 * s, ty, bw, th), theme.text_dim.alpha(0x90), bw / 2.0);
        }
        if !marks.is_empty() {
            let track = rect.inset(0.0, (4.0 * s).round());
            let (mw, mh) = ((6.0 * s).round(), (2.0 * s).round().max(2.0));
            let mut last = f32::MIN;
            let mut current = None;
            for (frac, is_current) in &marks {
                let y = (track.y + frac * (track.h - mh)).round();
                if *is_current {
                    current = Some(y);
                } else if y > last {
                    g.dl.rect(Rect::new(track.right() - mw, y, mw, mh), Color::rgb(0xE5C07B).alpha(0xC0));
                    last = y;
                }
            }
            if let Some(y) = current {
                g.dl.rect(Rect::new(track.right() - mw, y - 1.0, mw, mh + 2.0), Color::rgb(0xF0A030));
            }
        }
        g.dl.set_clip(None);
        if focused && self.pane_rename.is_none() && !self.sidebar_focused() {
            self.set_ime_area(cr);
        }
    }

    fn draw_pane_header<B: TextBackend>(&mut self, g: &mut Gfx<B>, id: PaneId, rect: Rect, focused: bool) {
        let header = self.pane_header(id, rect);
        if header.h <= 0.0 || header.w <= 0.0 {
            return;
        }
        let Some(pane) = self.panes.get(&id) else { return };
        let s = self.ui();
        let theme = self.theme;
        let ui = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let line = (1.0 * s).round().max(1.0);
        let field = header.inset((5.0 * s).round(), (2.0 * s).round());
        let x = field.x + (4.0 * s).round();
        let y = baseline(g, header, ui);
        g.dl.set_clip(Some(header));
        g.dl.rect(header, theme.surface);
        g.dl.rect(Rect::new(header.x, header.bottom() - line, header.w, line), theme.divider);
        let edit = self.pane_rename.as_ref().filter(|edit| edit.pane == id);
        if let Some(edit) = edit {
            let radius = (4.0 * s).round();
            g.dl.round_rect(field, theme.tab_active, radius);
            g.dl.outline(field, theme.accent, radius, line);
            let width = (field.right() - x - 4.0 * s).max(0.0);
            let empty = edit.text.is_empty() && self.preedit.is_empty();
            let w = if empty {
                g.text(x, y, t("Pane name"), ui, theme.text_dim, width);
                0.0
            } else {
                let w = g.text(x, y, &edit.text, ui, theme.text, width);
                g.text(x + w, y, &self.preedit.text, ui, theme.text, (width - w).max(0.0));
                let cursor = self.preedit.cursor.unwrap_or(self.preedit.text.len());
                w + g.measure(&self.preedit.text[..cursor], ui)
            };
            let caret = Rect::new(x + w.min(width), field.y + 2.0 * line, line, (field.h - 4.0 * line).max(0.0));
            g.dl.rect(caret, theme.accent);
            self.set_ime_area(caret);
        } else {
            let title = pane.name.as_deref().or(pane.title.as_deref()).unwrap_or(&pane.profile.name);
            g.text(x, y, title, ui, if focused { theme.text } else { theme.text_dim }, (header.right() - x - 8.0 * s).max(0.0));
        }
        g.dl.set_clip(None);
    }

    fn draw_search<B: TextBackend>(&mut self, g: &mut Gfx<B>) {
        use crate::search::SearchHit;
        let Some(geo) = self.search_geometry() else { return };
        let Some(bar) = self.search.clone() else { return };
        let s = self.ui();
        let theme = self.theme;
        let radius = (6.0 * s).round();
        g.dl.shadow(Rect::new(geo.frame.x, geo.frame.y + 3.0 * s, geo.frame.w, geo.frame.h), theme.shadow, radius, (10.0 * s).round());
        g.dl.round_rect(geo.frame, theme.overlay, radius);
        let border = if bar.error || bar.missing { Color::rgb(0xD9534F) } else { theme.overlay_border };
        g.dl.outline(geo.frame, border, radius, (1.0 * s).round().max(1.0));
        let ui = FontStyle::new(FontFamily::Ui, (12.5 * s).round(), false);
        let y = baseline(g, geo.field, ui);
        if bar.query.is_empty() {
            g.text(geo.field.x, y, t("Search"), ui, theme.text_dim, geo.field.w);
        }
        let counter = (!bar.query.is_empty() && !bar.error).then(|| match bar.current {
            Some(c) => format!("{c}/{}", bar.count),
            None => format!("{}", bar.count),
        });
        let small = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let cw = counter.as_ref().map_or(0.0, |c| g.measure(c, small));
        if let Some(c) = &counter {
            let cy = baseline(g, geo.field, small);
            g.text(geo.field.right() - cw, cy, c, small, if bar.missing { Color::rgb(0xD9534F) } else { theme.text_dim }, cw + 1.0);
        }
        let w = g.text(geo.field.x, y, &bar.query, ui, theme.text, (geo.field.w - cw - 8.0 * s).max(0.0));
        let caret_h = (16.0 * s).round();
        g.dl.rect(Rect::new(geo.field.x + w + 1.0, geo.field.y + ((geo.field.h - caret_h) / 2.0).round(), (1.0 * s).round().max(1.0), caret_h), theme.accent);
        let options = self.config.terminal.search_options;
        let icons = FontStyle::new(FontFamily::Icons, (10.0 * s).round(), false);
        let (mx, my) = self.mouse;
        for (hit, r) in geo.buttons {
            let on = match hit {
                SearchHit::Case => options.case_sensitive,
                SearchHit::Regex => options.regex,
                SearchHit::Word => options.whole_word,
                _ => false,
            };
            if on {
                g.dl.round_rect(r, theme.accent.alpha(0x50), (4.0 * s).round());
            } else if r.contains(mx, my) {
                g.dl.round_rect(r, theme.button_hover, (4.0 * s).round());
            }
            let ink = if on { theme.text } else { theme.text_dim };
            match hit {
                SearchHit::Case => centered(g, r, "Aa", ui, ink),
                SearchHit::Regex => centered(g, r, ".*", ui, ink),
                SearchHit::Word => centered(g, r, "W", ui, ink),
                SearchHit::Older => centered(g, r, "\u{E70E}", icons, ink),
                SearchHit::Newer => centered(g, r, "\u{E70D}", icons, ink),
                SearchHit::Close => centered(g, r, G_CLOSE, FontStyle::new(FontFamily::Icons, (8.0 * s).round(), false), ink),
                SearchHit::Field => {}
            }
        }
    }

    fn draw_hint<B: TextBackend>(&mut self, g: &mut Gfx<B>, pane: Rect, text: &str) {
        let s = self.ui();
        let theme = self.theme;
        let h = hint_height(s);
        let r = Rect::new(pane.x, pane.bottom() - h, pane.w, h);
        g.dl.rect(r, theme.overlay);
        g.dl.rect(Rect::new(r.x, r.y, r.w, (1.0 * s).round().max(1.0)), theme.accent);
        let ui = FontStyle::new(FontFamily::Ui, (12.5 * s).round(), false);
        let inner = r.inset((14.0 * s).round(), 0.0);
        let y = baseline(g, inner, ui);
        g.text(inner.x, y, text, ui, theme.text, inner.w);
    }

    fn draw_offer<B: TextBackend>(&mut self, g: &mut Gfx<B>, pane: Rect, state: crate::app::PaneTransfer, typed: &str) {
        use crate::app::PaneTransfer;
        match state {
            PaneTransfer::Incoming => return self.draw_hint(g, pane, t("The remote side wants to send files. Enter receives them into Downloads, Esc refuses.")),
            PaneTransfer::Running => return self.draw_hint(g, pane, t("Transfer in progress. Esc cancels.")),
            PaneTransfer::AwaitingFiles | PaneTransfer::None => {}
        }
        let s = self.ui();
        let theme = self.theme;
        let h = (64.0 * s).round();
        let r = Rect::new(pane.x, pane.bottom() - h, pane.w, h);
        g.dl.rect(r, theme.overlay);
        g.dl.rect(Rect::new(r.x, r.y, r.w, (1.0 * s).round().max(1.0)), theme.accent);
        let ui = FontStyle::new(FontFamily::Ui, (12.5 * s).round(), false);
        let pad = (14.0 * s).round();
        let first = Rect::new(r.x + pad, r.y + (6.0 * s).round(), r.w - 2.0 * pad, (24.0 * s).round());
        let y = baseline(g, first, ui);
        g.text(first.x, y, t("Send files: drop them here, or type a path and press Enter. Esc cancels."), ui, theme.text_dim, first.w);
        let field = Rect::new(r.x + pad, first.bottom() + (2.0 * s).round(), r.w - 2.0 * pad, (26.0 * s).round());
        g.dl.round_rect(field, theme.surface, (4.0 * s).round());
        g.dl.outline(field, theme.accent, (4.0 * s).round(), (1.0 * s).round().max(1.0));
        let inner = field.inset((8.0 * s).round(), 0.0);
        let fy = baseline(g, inner, ui);
        let w = g.text(inner.x, fy, typed, ui, theme.text, inner.w);
        let caret_h = (14.0 * s).round();
        g.dl.rect(Rect::new(inner.x + w + 1.0, field.y + ((field.h - caret_h) / 2.0).round(), (1.0 * s).round().max(1.0), caret_h), theme.accent);
    }

    fn draw_paste<B: TextBackend>(&mut self, g: &mut Gfx<B>, m: Metrics) {
        use crate::paste::{BUTTONS, PREVIEW_LINES};
        let Some(geo) = self.paste_geometry() else { return };
        let Some(confirm) = self.paste_confirm.clone() else { return };
        let s = self.ui();
        let theme = self.theme;
        let radius = (8.0 * s).round();
        let line = (1.0 * s).round().max(1.0);
        g.dl.rect(self.chrome.content, theme.shadow.alpha(0x40));
        g.dl.shadow(Rect::new(geo.frame.x, geo.frame.y + 6.0 * s, geo.frame.w, geo.frame.h), theme.shadow, radius, (18.0 * s).round());
        g.dl.round_rect(geo.frame, theme.overlay, radius);
        g.dl.outline(geo.frame, theme.overlay_border, radius, line);
        let lines = confirm.lines();
        let title = FontStyle::new(FontFamily::Ui, (14.0 * s).round(), true);
        let ty = baseline(g, geo.title, title);
        g.text(geo.title.x, ty, &tf("Paste {count} lines?", &[("count", &lines.len().to_string())]), title, theme.text, geo.title.w);
        g.dl.round_rect(geo.preview, theme.surface, (5.0 * s).round());
        g.dl.outline(geo.preview, theme.overlay_border, (5.0 * s).round(), line);
        let mono = FontStyle::new(FontFamily::Mono, m.px, false);
        let inner = geo.preview.inset((10.0 * s).round(), (8.0 * s).round());
        for (k, text) in lines.iter().take(PREVIEW_LINES).enumerate() {
            let y = inner.y + k as f32 * m.h + m.baseline;
            g.text(inner.x, y.round(), &text.replace('\t', "    "), mono, theme.text, inner.w);
        }
        if lines.len() > PREVIEW_LINES {
            let y = inner.y + PREVIEW_LINES as f32 * m.h + m.baseline;
            let ui = FontStyle::new(FontFamily::Ui, (12.0 * s).round(), false);
            g.text(inner.x, y.round(), &tf("and {count} more", &[("count", &(lines.len() - PREVIEW_LINES).to_string())]), ui, theme.text_dim, inner.w);
        }
        let ui = FontStyle::new(FontFamily::Ui, (12.5 * s).round(), false);
        let (mx, my) = self.mouse;
        for (i, r) in geo.buttons.iter().enumerate() {
            let chosen = confirm.button == i;
            if chosen {
                g.dl.round_rect(*r, theme.accent, (5.0 * s).round());
            } else {
                g.dl.round_rect(*r, if r.contains(mx, my) { theme.button_hover } else { theme.tab_active }, (5.0 * s).round());
                g.dl.outline(*r, theme.overlay_border, (5.0 * s).round(), line);
            }
            centered(g, *r, t(BUTTONS[i]), ui, if chosen { Color::rgb(0xFFFFFF) } else { theme.text });
        }
    }

    fn draw_close<B: TextBackend>(&mut self, g: &mut Gfx<B>) {
        use crate::close::{CloseTarget, BUTTONS, LISTED};
        let Some(geo) = self.close_geometry() else { return };
        let Some(confirm) = self.close_confirm.clone() else { return };
        let s = self.ui();
        let theme = self.theme;
        let radius = (8.0 * s).round();
        let line = (1.0 * s).round().max(1.0);
        g.dl.rect(self.chrome.content, theme.shadow.alpha(0x40));
        g.dl.shadow(Rect::new(geo.frame.x, geo.frame.y + 6.0 * s, geo.frame.w, geo.frame.h), theme.shadow, radius, (18.0 * s).round());
        g.dl.round_rect(geo.frame, theme.overlay, radius);
        g.dl.outline(geo.frame, theme.overlay_border, radius, line);
        let title = FontStyle::new(FontFamily::Ui, (14.0 * s).round(), true);
        let heading = match confirm.target {
            CloseTarget::Pane(_) => t("Close the pane? These programs are still running:"),
            CloseTarget::PaneProfile(..) => t("Change the profile and stop the current session?"),
            CloseTarget::Tabs(_) if confirm.programs.is_empty() => t("Close the pinned tab?"),
            CloseTarget::Tabs(ref reps) if reps.len() > 1 => t("Close the tabs? These programs are still running:"),
            CloseTarget::Tabs(_) => t("Close the tab? These programs are still running:"),
            CloseTarget::Window => t("Close the window? These programs are still running:"),
        };
        let ty = baseline(g, geo.title, title);
        g.text(geo.title.x, ty, heading, title, theme.text, geo.title.w);
        let ui = FontStyle::new(FontFamily::Ui, (12.5 * s).round(), false);
        let row = (20.0 * s).round();
        for (k, name) in confirm.programs.iter().take(LISTED).enumerate() {
            let r = Rect::new(geo.list.x, geo.list.y + k as f32 * row, geo.list.w, row);
            let y = baseline(g, r, ui);
            g.text(r.x + (8.0 * s).round(), y, name, ui, theme.text_dim, r.w);
        }
        if confirm.programs.len() > LISTED {
            let r = Rect::new(geo.list.x, geo.list.y + LISTED as f32 * row, geo.list.w, row);
            let y = baseline(g, r, ui);
            g.text(r.x + (8.0 * s).round(), y, &tf("and {count} more", &[("count", &(confirm.programs.len() - LISTED).to_string())]), ui, theme.text_dim, r.w);
        }
        let (mx, my) = self.mouse;
        for (i, r) in geo.buttons.iter().enumerate() {
            let chosen = confirm.button == i;
            if chosen {
                g.dl.round_rect(*r, theme.accent, (5.0 * s).round());
            } else {
                g.dl.round_rect(*r, if r.contains(mx, my) { theme.button_hover } else { theme.tab_active }, (5.0 * s).round());
                g.dl.outline(*r, theme.overlay_border, (5.0 * s).round(), line);
            }
            let label = if i == 0 && matches!(confirm.target, CloseTarget::PaneProfile(..)) { t("Change profile") } else { t(BUTTONS[i]) };
            centered(g, *r, label, ui, if chosen { Color::rgb(0xFFFFFF) } else { theme.text });
        }
    }

    fn draw_tab_toast<B: TextBackend>(&mut self, g: &mut Gfx<B>) {
        let Some((text, _)) = self.tab_toast.clone() else { return };
        let s = self.ui();
        let theme = self.theme;
        let ui = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), false);
        let content = self.chrome.content;
        let w = (g.measure(&text, ui) + 28.0 * s).min((content.w - 24.0 * s).max(0.0)).round();
        let h = (32.0 * s).round();
        let r = Rect::new((content.x + (content.w - w) / 2.0).round(), content.y + (14.0 * s).round(), w, h);
        g.dl.shadow(Rect::new(r.x, r.y + 3.0 * s, r.w, r.h), theme.shadow, (8.0 * s).round(), (12.0 * s).round());
        g.dl.round_rect(r, theme.overlay, (8.0 * s).round());
        g.dl.outline(r, theme.overlay_border, (8.0 * s).round(), (1.0 * s).round().max(1.0));
        centered(g, r, &text, ui, theme.text);
    }

    fn draw_update_notice<B: TextBackend>(&mut self, g: &mut Gfx<B>) {
        let s = self.ui();
        let title = FontStyle::new(FontFamily::Ui, (14.0 * s).round(), true);
        let ui = FontStyle::new(FontFamily::Ui, (12.5 * s).round(), false);
        let content = self.chrome.content;
        let Some(notice) = self.update_notice.as_mut() else { return };
        notice.button_widths = notice.buttons().iter().map(|b| (g.measure(b.label(notice.notes_open), ui) + 28.0 * s).ceil()).collect();
        let inner = (crate::update::notice_geometry(content, s, notice).frame.w - 28.0 * s).max(1.0);
        notice.status_lines = wrapped_info(g, &notice.status(), ui, inner);
        if notice.notes_shown() {
            let width = (inner - 20.0 * s).max(1.0);
            let mut wrapped = Vec::new();
            for line in crate::update::note_lines(&notice.release.notes) {
                if line.is_empty() {
                    wrapped.push(String::new());
                } else {
                    wrapped.extend(wrapped_info(g, &line, ui, width));
                }
            }
            if wrapped != notice.wrapped {
                notice.wrapped = wrapped;
                notice.notes_scroll = 0;
            }
        }
        let Some(geo) = self.update_geometry() else { return };
        let Some(notice) = self.update_notice.clone() else { return };
        let theme = self.theme;
        let radius = (8.0 * s).round();
        let line = (1.0 * s).round().max(1.0);
        g.dl.shadow(Rect::new(geo.frame.x, geo.frame.y + 4.0 * s, geo.frame.w, geo.frame.h), theme.shadow, radius, (16.0 * s).round());
        g.dl.round_rect(geo.frame, theme.overlay, radius);
        g.dl.outline(geo.frame, theme.overlay_border, radius, line);
        g.dl.round_rect(Rect::new(geo.frame.x, geo.frame.y + radius, (3.0 * s).round(), (geo.frame.h - 2.0 * radius).max(line)), theme.accent, (1.5 * s).round());
        let ty = baseline(g, geo.title, title);
        g.text(geo.title.x, ty, &notice.title(), title, theme.text, geo.title.w);
        let failed = matches!(notice.stage, crate::update::Stage::Failed | crate::update::Stage::ReadOnly);
        let status_ink = if failed { Color::rgb(if theme.dark { 0xFF9490 } else { 0xA52320 }) } else { theme.text_dim };
        for (k, text) in notice.status_lines.iter().take(((geo.status.h / geo.line_h) as usize).max(1)).enumerate() {
            let r = Rect::new(geo.status.x, geo.status.y + k as f32 * geo.line_h, geo.status.w, geo.line_h);
            let y = baseline(g, r, ui);
            g.text(r.x, y, text, ui, status_ink, r.w);
        }
        if let (Some(bar), Some(fraction)) = (geo.bar, notice.fraction()) {
            g.dl.round_rect(bar, theme.surface, bar.h / 2.0);
            if fraction > 0.0 {
                g.dl.round_rect(Rect::new(bar.x, bar.y, (bar.w * fraction).max(bar.h).round(), bar.h), theme.accent, bar.h / 2.0);
            }
        }
        if let Some(notes) = geo.notes {
            g.dl.round_rect(notes, theme.surface, (5.0 * s).round());
            g.dl.outline(notes, theme.overlay_border, (5.0 * s).round(), line);
            let inner = notes.inset((10.0 * s).round(), (8.0 * s).round());
            g.dl.set_clip(Some(notes));
            for (k, text) in notice.visible_notes().iter().enumerate() {
                let r = Rect::new(inner.x, inner.y + k as f32 * geo.line_h, inner.w, geo.line_h);
                let y = baseline(g, r, ui);
                g.text(r.x, y, text, ui, theme.text, r.w);
            }
            g.dl.set_clip(None);
            if notice.wrapped.len() > crate::update::NOTES_VISIBLE {
                let total = notice.wrapped.len() as f32;
                let track = notes.inset(0.0, (6.0 * s).round());
                let thumb_h = (track.h * crate::update::NOTES_VISIBLE as f32 / total).max(16.0 * s);
                let thumb_y = track.y + (track.h - thumb_h) * notice.notes_scroll as f32 / (total - crate::update::NOTES_VISIBLE as f32).max(1.0);
                let w = (3.0 * s).round();
                g.dl.round_rect(Rect::new(notes.right() - w - (4.0 * s).round(), thumb_y.round(), w, thumb_h.round()), theme.overlay_border, w / 2.0);
            }
        }
        let (mx, my) = self.mouse;
        for (i, (button, r)) in geo.buttons.iter().enumerate() {
            let primary = i == 0;
            if primary {
                g.dl.round_rect(*r, theme.accent, (5.0 * s).round());
            } else {
                g.dl.round_rect(*r, if r.contains(mx, my) { theme.button_hover } else { theme.tab_active }, (5.0 * s).round());
                g.dl.outline(*r, theme.overlay_border, (5.0 * s).round(), line);
            }
            centered(g, *r, button.label(notice.notes_open), ui, if primary { Color::rgb(0xFFFFFF) } else { theme.text });
        }
    }

    fn draw_mru<B: TextBackend>(&self, g: &mut Gfx<B>) {
        let Some((order, selected)) = self.mru.selection() else { return };
        let s = self.ui();
        let (w, h) = self.size;
        let row_h = (30.0 * s).round().max(1.0);
        let pad = (6.0 * s).round();
        let count = order.len().min(8).min(((h - 4.0 * pad) / row_h) as usize);
        if count == 0 || w < 6.0 * pad {
            return;
        }
        let first = selected.saturating_sub(count / 2).min(order.len() - count);
        let width = (320.0 * s).round().min(w - 2.0 * pad);
        let height = count as f32 * row_h + 2.0 * pad;
        let frame = Rect::new(((w - width) / 2.0).round(), ((h - height) / 2.0).round(), width, height);
        let radius = (8.0 * s).round();
        let theme = self.theme;
        g.dl.shadow(Rect::new(frame.x, frame.y + 4.0 * s, width, height), theme.shadow, radius, (14.0 * s).round());
        g.dl.round_rect(frame, theme.overlay, radius);
        g.dl.outline(frame, theme.overlay_border, radius, (1.0 * s).round().max(1.0));
        let ui = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), false);
        for (i, key) in order[first..first + count].iter().enumerate() {
            let Some(tab) = self.tabs.iter().position(|tab| tab.key == *key) else { continue };
            let row = Rect::new(frame.x + pad, frame.y + pad + row_h * i as f32, width - 2.0 * pad, row_h);
            if first + i == selected {
                g.dl.round_rect(row, theme.accent.alpha(0x38), (4.0 * s).round());
            }
            let y = baseline(g, row, ui);
            g.text(row.x + pad, y, &self.tab_title(tab), ui, theme.text, row.w - 2.0 * pad);
        }
    }

    fn draw_menu<B: TextBackend>(&mut self, g: &mut Gfx<B>) {
        use crate::menu::Entry;
        let Some(geo) = self.menu_geometry() else { return };
        let Some(menu) = self.menu.clone() else { return };
        let s = self.ui();
        let theme = self.theme;
        let radius = (7.0 * s).round();
        g.dl.shadow(Rect::new(geo.frame.x, geo.frame.y + 4.0 * s, geo.frame.w, geo.frame.h), theme.shadow, radius, (14.0 * s).round());
        g.dl.round_rect(geo.frame, theme.overlay, radius);
        g.dl.outline(geo.frame, theme.overlay_border, radius, (1.0 * s).round().max(1.0));
        for r in &geo.separators {
            g.dl.rect(*r, theme.overlay_border);
        }
        let ui = FontStyle::new(FontFamily::Ui, (12.5 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (11.0 * s).round(), false);
        for (i, r) in &geo.rows {
            let Some(Entry::Item { label, action, enabled }) = menu.entries.get(*i) else { continue };
            if menu.selected == Some(*i) {
                g.dl.round_rect(*r, theme.accent.alpha(0x38), (4.0 * s).round());
            }
            let ink = if *enabled { theme.text } else { theme.text_dim.alpha(0x80) };
            let hint = if menu.profile.is_some() { String::new() } else { self.keymap.bindings_of(action).next().map(crate::keymap::format_binding).unwrap_or_default() };
            let hint_w = if hint.is_empty() { 0.0 } else { g.measure(&hint, small) };
            let pad = (12.0 * s).round();
            let y = baseline(g, *r, ui);
            g.text(r.x + pad, y, t(label), ui, ink, (r.w - hint_w - 3.0 * pad).max(0.0));
            if hint_w > 0.0 {
                let hy = baseline(g, *r, small);
                g.text(r.right() - pad - hint_w, hy, &hint, small, theme.text_dim, hint_w + 1.0);
            }
        }
    }

    fn draw_files<B: TextBackend>(&mut self, g: &mut Gfx<B>) {
        use crate::files::{date, human_size, Prompt, TransferState};
        if self.files.as_ref().is_some_and(|f| !self.panes.contains_key(&f.pane)) {
            self.files = None;
        }
        let Some(geo) = self.files_geometry() else { return };
        let Some(page) = self.files.clone() else { return };
        let transfers: Vec<_> = self.transfers_of(page.pane).into_iter().cloned().collect();
        let s = self.ui();
        let theme = self.theme;
        let line = (1.0 * s).round().max(1.0);
        let radius = (6.0 * s).round();
        let (mx, my) = self.mouse;
        g.dl.rect(self.chrome.content, theme.surface);
        let title = FontStyle::new(FontFamily::Ui, (17.0 * s).round(), true);
        let ui = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let ty = baseline(g, geo.title, title);
        g.text(geo.title.x, ty, &tf("Files on {host}", &[("host", &page.host)]), title, theme.text, geo.title.w);
        if geo.close.contains(mx, my) {
            g.dl.round_rect(geo.close, theme.button_hover, radius);
        }
        centered(g, geo.close, G_CLOSE, FontStyle::new(FontFamily::Icons, (10.0 * s).round(), false), theme.text);

        let caret_h = (16.0 * s).round();
        let field = |g: &mut Gfx<B>, r: Rect, text: &str, placeholder: &str, focused: bool| {
            g.dl.round_rect(r, theme.overlay, radius);
            g.dl.outline(r, if focused { theme.accent } else { theme.overlay_border }, radius, line);
            let inner = r.inset((10.0 * s).round(), 0.0);
            let y = baseline(g, inner, ui);
            if text.is_empty() {
                g.text(inner.x, y, placeholder, ui, theme.text_dim, inner.w);
            }
            let w = g.text(inner.x, y, text, ui, theme.text, inner.w);
            if focused {
                g.dl.rect(Rect::new(inner.x + w + 1.0, r.y + ((r.h - caret_h) / 2.0).round(), line, caret_h), theme.accent);
            }
        };
        let (path_text, path_hint, editing) = match &page.prompt {
            Some(Prompt::NewFolder(t)) => (t.clone(), crate::i18n::t("New folder name").to_owned(), true),
            Some(Prompt::Rename { text, .. }) => (text.clone(), t("New name").to_owned(), true),
            Some(Prompt::GoTo(t)) => (t.clone(), crate::i18n::t("Go to folder").to_owned(), true),
            Some(Prompt::Upload(t)) => (t.clone(), crate::i18n::t("Local file to upload").to_owned(), true),
            Some(Prompt::Delete { name, dir: true }) => (String::new(), tf("Delete the folder {name} and everything in it? Enter to delete, Esc to keep", &[("name", name)]), false),
            Some(Prompt::Delete { name, .. }) => (String::new(), tf("Delete {name}? Enter to delete, Esc to keep", &[("name", name)]), false),
            Some(Prompt::Chmod { text, dir, .. }) => (text.clone(), if *dir { t("Permissions in octal, applied to everything inside").to_owned() } else { t("Permissions in octal, for example 644").to_owned() }, true),
            None => (page.path.clone(), t("Loading").to_owned(), false),
        };
        field(g, geo.path, &path_text, &path_hint, editing);
        if let Some(Prompt::Delete { .. }) = page.prompt {
            g.dl.outline(geo.path, Color::rgb(0xD9534F), radius, line);
        }
        field(g, geo.filter, &page.query, t("Filter"), page.prompt.is_none());

        g.dl.set_clip(Some(geo.list));
        let name_x = geo.list.x + (40.0 * s).round();
        let date_w = (100.0 * s).round();
        let size_w = (90.0 * s).round();
        for (row, r) in &geo.rows {
            if r.bottom() < geo.list.y || r.y > geo.list.bottom() {
                continue;
            }
            if *row == page.selected {
                g.dl.round_rect(*r, theme.accent.alpha(0x30), radius);
            } else if r.contains(mx, my) {
                g.dl.round_rect(*r, theme.button_hover, radius);
            }
            let y = baseline(g, *r, ui);
            let icon = Rect::new(r.x + (14.0 * s).round(), r.y + ((r.h - 14.0 * s) / 2.0).round(), (16.0 * s).round(), (14.0 * s).round());
            if page.is_up(*row) {
                g.dl.round_rect(icon, theme.text_dim.alpha(0x70), (2.0 * s).round());
                g.text(name_x, y, crate::files::UP, ui, theme.text, r.w);
                continue;
            }
            let Some(entry) = page.entry(*row) else { continue };
            if entry.dir {
                g.dl.round_rect(icon, theme.accent, (2.0 * s).round());
            } else {
                let paper = Rect::new(icon.x + 2.0 * s, icon.y - 1.0 * s, icon.w - 4.0 * s, icon.h + 2.0 * s);
                g.dl.outline(paper, theme.text_dim, (2.0 * s).round(), line);
            }
            let right = r.right() - (12.0 * s).round();
            let name_w = (right - date_w - size_w - name_x - 110.0 * s).max(0.0);
            let name = if entry.link { format!("{} \u{2192}", entry.name) } else { entry.name.clone() };
            g.text(name_x, y, &name, ui, theme.text, name_w);
            if !entry.dir {
                let size = human_size(entry.size);
                let w = g.measure(&size, small);
                let sy = baseline(g, *r, small);
                g.text(right - date_w - w - 12.0 * s, sy, &size, small, theme.text_dim, size_w);
            }
            if let Some(mode) = entry.mode {
                let p = crate::files::permissions(mode);
                let py = baseline(g, *r, small);
                let pw = g.measure(&p, small);
                g.text(right - date_w - size_w - pw - 24.0 * s, py, &p, small, theme.text_dim, pw + 1.0);
            }
            if let Some(m) = entry.modified {
                let d = date(m);
                let w = g.measure(&d, small);
                let dy = baseline(g, *r, small);
                g.text(right - w, dy, &d, small, theme.text_dim, date_w);
            }
        }
        g.dl.set_clip(None);
        let status = if page.loading && page.entries.is_empty() {
            Some((t("Loading").to_owned(), theme.text_dim))
        } else if let Some(e) = &page.error {
            Some((e.clone(), Color::rgb(0xD9534F)))
        } else if page.rows() == 0 {
            Some((if page.query.is_empty() { t("This folder is empty").to_owned() } else { t("Nothing matches the filter").to_owned() }, theme.text_dim))
        } else {
            None
        };
        if let Some((text, color)) = status {
            centered(g, Rect::new(geo.list.x, geo.list.y, geo.list.w, (60.0 * s).round()), &text, ui, color);
        }

        for (id, r) in &geo.transfers {
            let Some(t) = transfers.iter().find(|t| t.id == *id) else { continue };
            if r.contains(mx, my) {
                g.dl.round_rect(*r, theme.button_hover, radius);
            }
            let pad = (12.0 * s).round();
            let bar = Rect::new(r.x + pad, r.bottom() - (6.0 * s).round(), r.w - 2.0 * pad, (3.0 * s).round().max(2.0));
            let fraction = if t.total > 0 { (t.done as f32 / t.total as f32).clamp(0.0, 1.0) } else { 0.0 };
            let (state, color) = match &t.state {
                TransferState::Running => (tf("{done} of {total}  ·  click to cancel", &[("done", &human_size(t.done)), ("total", &human_size(t.total))]), theme.text_dim),
                TransferState::Editing { saves } => (tf("editing locally  ·  saved {saves} times  ·  click to stop", &[("saves", &saves.to_string())]), theme.accent),
                TransferState::Done(m) if !t.upload => (tf("{message}  ·  click to show", &[("message", m)]), theme.text_dim),
                TransferState::Done(m) => (m.clone(), theme.text_dim),
                TransferState::Failed(e) => (tf("{error}  ·  click to dismiss", &[("error", e)]), Color::rgb(0xD9534F)),
            };
            g.dl.round_rect(bar, theme.overlay_border, bar.h / 2.0);
            if fraction > 0.0 {
                g.dl.round_rect(Rect::new(bar.x, bar.y, (bar.w * fraction).max(bar.h), bar.h), if matches!(t.state, TransferState::Failed(_)) { Color::rgb(0xD9534F) } else { theme.accent }, bar.h / 2.0);
            }
            let y = r.y + (17.0 * s).round();
            let arrow = if t.edit { "\u{270E}" } else if t.upload { "\u{2191}" } else { "\u{2193}" };
            let label = format!("{arrow} {}", t.name);
            let lw = g.text(r.x + pad, y, &label, ui, theme.text, r.w * 0.45);
            g.text(r.x + pad + lw + (14.0 * s).round(), y, &state, small, color, (r.w - lw - 3.0 * pad).max(0.0));
        }
        let hint = t("Enter open  ·  Ctrl+D download  ·  F4 edit  ·  F2 rename  ·  F7 new folder  ·  F9 permissions  ·  Del delete  ·  Ctrl+U or drop to upload");
        let hy = baseline(g, geo.hint, small);
        g.text(geo.hint.x, hy, hint, small, theme.text_dim, geo.hint.w);
    }

    fn draw_profile_tree_row<B: TextBackend>(&self, g: &mut Gfx<B>, page: &crate::settings::SettingsPage, tree: &crate::profiles_page::TreeRow, rect: Rect, selected: bool) {
        use crate::profiles_page::{kind_label, layout::row_action, TreeKind};
        let theme = self.theme;
        let s = self.ui();
        let ui = FontStyle::new(FontFamily::Ui, (13.5 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let bold = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), true);
        let (mx, my) = self.mouse;
        let line = s.round().max(1.0);
        let action = row_action(rect, s);
        let mut x = rect.x + (14.0 * s).round() + tree.indent(s).min(rect.w * 0.25);
        let hint = selected.then_some(page.notice.as_deref()).flatten();
        let action_button = |g: &mut Gfx<B>, label: &str| {
            g.dl.round_rect(action, if action.contains(mx, my) { theme.button_hover } else { theme.overlay }, 5.0 * s);
            g.dl.outline(action, theme.overlay_border, 5.0 * s, line);
            centered(g, action, label, small, theme.text);
        };
        match &tree.kind {
            TreeKind::Profile(k) => {
                let profile = &page.profiles[*k];
                let size = (26.0 * s).round();
                let mark = Rect::new(x, rect.y + (rect.h - size) * 0.5, size, size).snap();
                g.dl.round_rect(mark, Color::rgb(profile.tint), (5.0 * s).round());
                let font = FontStyle::new(FontFamily::Ui, (if profile.badge.chars().count() > 2 { 9.0 } else { 11.0 } * s).round(), true);
                let background = Rgb { r: (profile.tint >> 16) as u8, g: (profile.tint >> 8) as u8, b: profile.tint as u8 };
                let ink = crate::theme::ensure_contrast(Rgb { r: 255, g: 255, b: 255 }, background, 4.5);
                let badge_width = g.measure(&profile.badge, font).min(mark.w);
                let by = baseline(g, mark, font);
                g.text(mark.x + (mark.w - badge_width) * 0.5, by, &profile.badge, font, color(ink), mark.w);
                x = mark.right() + (10.0 * s).round();
                let mut right = action.x - 8.0 * s;
                for (enabled, text, accent) in [(profile.default, t("Default"), true), (profile.hidden, t("Hidden"), false), (!profile.custom, t("Built-in"), false)] {
                    if !enabled {
                        continue;
                    }
                    let w = g.measure(text, small) + 12.0 * s;
                    if right - w < x + 62.0 * s {
                        continue;
                    }
                    let tag = Rect::new(right - w, rect.y + (rect.h - 20.0 * s) * 0.5, w, 20.0 * s).snap();
                    g.dl.round_rect(tag, if accent { theme.accent.alpha(0x30) } else { theme.overlay }, 4.0 * s);
                    centered(g, tag, text, small, if accent { theme.text } else { theme.text_dim });
                    right = tag.x - 5.0 * s;
                }
                let label = Rect::new(x, rect.y + 3.0 * s, (right - x).max(0.0), rect.h * 0.51);
                let y = baseline(g, label, ui);
                g.text(x, y, &profile.name, ui, theme.text, label.w);
                let detail = Rect::new(x, label.bottom() - s, (action.x - x - 8.0 * s).max(0.0), rect.bottom() - label.bottom() - 3.0 * s);
                let y = baseline(g, detail, small);
                g.text(x, y, hint.unwrap_or(&profile.detail), small, if hint.is_some() { theme.accent } else { theme.text_dim }, detail.w);
                action_button(g, "▶");
            }
            TreeKind::Header(header) => {
                let open = page.searching() || !page.collapsed.contains(header);
                g.dl.chevron((x + 4.0 * s, rect.y + rect.h * 0.5), 8.0 * s, if open { 90.0 } else { 0.0 }, 1.5 * s, theme.text_dim);
                x += 20.0 * s;
                let group = header.group_index(&page.groups).map(|i| &page.groups[i]);
                if let Some(group) = group {
                    x = self.draw_profile_group_mark(g, rect, x, group, bold);
                    action_button(g, t("Edit"));
                }
                let end = if group.is_some() { action.x } else { rect.right() } - 10.0 * s;
                let y = baseline(g, rect, bold);
                let w = g.text(x, y, header.label(&page.groups), bold, theme.text, (end - x).max(0.0));
                if let Some(notice) = hint {
                    let x = x + w + 10.0 * s;
                    let y = baseline(g, rect, small);
                    g.text(x, y, notice, small, theme.accent, (end - x).max(0.0));
                }
            }
            TreeKind::DefaultsHeader => {
                let y = baseline(g, rect, bold);
                g.text(x, y, t("Defaults for each type"), bold, theme.accent, rect.right() - x);
            }
            TreeKind::NewProfile | TreeKind::NewGroup => {
                let plus = Rect::new(x, rect.y, 16.0 * s, rect.h);
                centered(g, plus, G_PLUS, FontStyle::new(FontFamily::Icons, (11.0 * s).round(), false), theme.accent);
                x = plus.right() + 7.0 * s;
                let y = baseline(g, rect, ui);
                let label = if matches!(tree.kind, TreeKind::NewProfile) { t("New profile") } else { t("New group") };
                g.text(x, y, label, ui, theme.text, rect.right() - x);
            }
            TreeKind::Defaults(kind) => {
                let y = baseline(g, rect, ui);
                let w = g.text(x, y, kind_label(kind), ui, theme.text, rect.w);
                if let Some(notice) = hint {
                    let nx = x + w + 10.0 * s;
                    let y = baseline(g, rect, small);
                    g.text(nx, y, notice, small, theme.accent, (action.x - nx).max(0.0));
                }
                action_button(g, t("Edit"));
            }
        }
    }

    fn draw_editor_field<B: TextBackend>(&self, g: &mut Gfx<B>, slot: Rect, field: (&str, &str), control: ProfileFieldControl, focused: bool) {
        let s = self.ui();
        let theme = self.theme;
        let ui = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let label = Rect::new(slot.x, slot.y, slot.w, 19.0 * s);
        let y = baseline(g, label, small);
        g.text(label.x, y, field.0, small, if focused { theme.text } else { theme.text_dim }, label.w);
        let r = Rect::new(slot.x, label.bottom() + 2.0 * s, slot.w, 31.0 * s).snap();
        g.dl.round_rect(r, theme.surface, 5.0 * s);
        g.dl.outline(r, if focused { theme.accent } else { theme.overlay_border }, 5.0 * s, if focused { (1.5 * s).round() } else { s.round() }.max(1.0));
        let mut inner = r.inset(9.0 * s, 2.0 * s);
        if field.0 == t("Color") {
            let size = (15.0 * s).round();
            let chip = Rect::new(inner.x, r.y + (r.h - size) * 0.5, size, size).snap();
            if let Some(tint) = aster_vt::schemes::parse_hex(field.1) {
                g.dl.round_rect(chip, color(tint), 3.0 * s);
            }
            g.dl.outline(chip, theme.overlay_border, 3.0 * s, s.round().max(1.0));
            inner.x += size + 7.0 * s;
            inner.w = (inner.w - size - 7.0 * s).max(0.0);
        }
        if let ProfileFieldControl::Switch(on) = control {
            let h = (18.0 * s).round();
            let switch = Rect::new(inner.right() - 32.0 * s, r.y + (r.h - h) * 0.5, 32.0 * s, h).snap();
            g.dl.round_rect(switch, if on { theme.accent } else { theme.overlay_border }, h * 0.5);
            let knob = switch.h - 4.0 * s;
            let kx = if on { switch.right() - knob - 2.0 * s } else { switch.x + 2.0 * s };
            g.dl.circle(Rect::new(kx, switch.y + (switch.h - knob) * 0.5, knob, knob), if on { Color::rgb(0xFFFFFF) } else { theme.text_dim });
            inner.w = (switch.x - 8.0 * s - inner.x).max(0.0);
        } else if matches!(control, ProfileFieldControl::Choice { .. }) {
            let (mx, my) = self.mouse;
            for (k, angle) in [(2, 180.0), (1, 0.0)] {
                let arrow = Rect::new(r.right() - k as f32 * 24.0 * s, r.y, 24.0 * s, r.h);
                if arrow.contains(mx, my) {
                    g.dl.round_rect(arrow.inset(2.0 * s, 3.0 * s), theme.button_hover, 4.0 * s);
                }
                g.dl.chevron((arrow.x + arrow.w * 0.5, arrow.y + arrow.h * 0.5), 7.0 * s, angle, (1.4 * s).max(1.0), theme.text_dim);
            }
            inner.w = (r.right() - 50.0 * s - inner.x).max(0.0);
        }
        let y = baseline(g, inner, ui);
        if focused && matches!(control, ProfileFieldControl::Text | ProfileFieldControl::Choice { editable: true }) {
            let first = g.dl.instances.len();
            let w = g.measure(field.1, ui);
            let caret = s.round().max(1.0);
            let x = inner.x.min(inner.right() - w - caret - 1.0);
            g.text(x, y, field.1, ui, theme.text, w + 1.0);
            g.dl.rect(Rect::new(x + w + 1.0, r.y + (r.h - 16.0 * s) * 0.5, caret, 16.0 * s), theme.accent);
            g.dl.confine(first, inner);
        } else {
            g.text(inner.x, y, field.1, ui, theme.text, inner.w);
        }
    }

    fn draw_profile_editor<B: TextBackend>(&self, g: &mut Gfx<B>, area: Rect, page: &crate::settings::SettingsPage) {
        use crate::profiles_page::{kind_label, layout::editor_layout, KINDS};
        let s = self.ui();
        let theme = self.theme;
        let ui = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let section = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), true);
        let title = FontStyle::new(FontFamily::Ui, (17.0 * s).round(), true);
        let line = s.round().max(1.0);
        let (mx, my) = self.mouse;
        let (layout, heading, error) = if let Some(editor) = &page.editor {
            (editor_layout(area, s, editor.tabs.len(), editor.tabs[editor.tab].fields.len(), editor.focus), editor.title(), editor.error.as_deref())
        } else if let Some(group) = &page.group_editor {
            (editor_layout(area, s, 0, 8, group.focus), if group.original.is_some() { t("Edit group") } else { t("New group") }.to_owned(), (!group.error.is_empty()).then_some(group.error.as_str()))
        } else {
            return;
        };
        let frame = layout.frame;
        let radius = (8.0 * s).round();
        g.dl.rect(area, theme.dim);
        g.dl.shadow(Rect::new(frame.x, frame.y + 6.0 * s, frame.w, frame.h), theme.shadow, radius, (18.0 * s).round());
        g.dl.round_rect(frame, theme.overlay, radius);
        g.dl.outline(frame, theme.overlay_border, radius, line);
        let y = baseline(g, layout.title, title);
        g.text(layout.title.x, y, &heading, title, theme.text, layout.title.w);
        if let Some(editor) = &page.editor {
            for (i, tab) in layout.tabs.iter().enumerate() {
                let active = i == editor.tab;
                g.dl.round_rect(*tab, if active { theme.accent.alpha(0x30) } else if tab.contains(mx, my) { theme.button_hover } else { theme.surface }, 5.0 * s);
                if active {
                    g.dl.outline(*tab, theme.accent, 5.0 * s, line);
                }
                centered(g, *tab, &editor.tabs[i].label, ui, if active { theme.text } else { theme.text_dim });
            }
            for &(i, slot) in &layout.fields {
                let field = &editor.tabs[editor.tab].fields[i];
                let control = match field.switch() {
                    Some(value) => ProfileFieldControl::Switch(value),
                    None if !field.choices.is_empty() => ProfileFieldControl::Choice { editable: field.editable },
                    None => ProfileFieldControl::Text,
                };
                self.draw_editor_field(g, slot, (&field.label, field.display()), control, i == editor.focus);
            }
        } else if let Some(group) = &page.group_editor {
            if let Some(r) = layout.group_defaults {
                let y = baseline(g, r, section);
                g.text(r.x, y, t("Defaults for this group"), section, theme.text, r.w);
            }
            let labels = [t("Name"), t("Parent group"), t("Color"), t("Icon")];
            for &(i, slot) in &layout.fields {
                let focused = i == group.focus;
                if i < 4 {
                    let value = if i == 1 { group.parent_label(&page.groups) } else { &group.fields[i] };
                    let control = if i == 1 { ProfileFieldControl::Choice { editable: false } } else { ProfileFieldControl::Text };
                    self.draw_editor_field(g, slot, (labels[i], value), control, focused);
                } else {
                    let r = slot.inset(0.0, 7.0 * s);
                    g.dl.round_rect(r, if r.contains(mx, my) { theme.button_hover } else { theme.surface }, 5.0 * s);
                    g.dl.outline(r, if focused { theme.accent } else { theme.overlay_border }, 5.0 * s, line);
                    let inner = r.inset(10.0 * s, 0.0);
                    let y = baseline(g, inner, ui);
                    g.text(inner.x, y, kind_label(KINDS[i - 4]), ui, theme.text, inner.w - 20.0 * s);
                    g.dl.chevron((inner.right() - 4.0 * s, r.y + r.h * 0.5), 8.0 * s, 0.0, (1.4 * s).max(1.0), theme.text_dim);
                }
            }
        }
        if let Some(r) = layout.thumb {
            g.dl.round_rect(r, theme.overlay_border, r.w * 0.5);
        }
        let list_hint = page.editor.as_ref().and_then(|e| e.tabs[e.tab].fields[e.focus].list_hint());
        let message = page.notice.as_deref().or(error).or(list_hint).unwrap_or_else(|| if page.editor.is_some() { t("Ctrl+Tab switch tab  ·  Tab next field  ·  Ctrl+Enter save") } else { t("Tab next field  ·  Ctrl+Enter save") });
        let y = baseline(g, layout.message, small);
        g.text(layout.message.x, y, message, small, if error.is_some() { Color::rgb(if theme.dark { 0xFF9490 } else { 0xA52320 }) } else if page.notice.is_some() { theme.accent } else { theme.text_dim }, layout.message.w);
        for (r, label, primary) in [(layout.cancel, t("Cancel"), false), (layout.save, t("Save"), true)] {
            g.dl.round_rect(r, if r.contains(mx, my) { theme.button_hover } else if primary { theme.accent.alpha(0x38) } else { theme.surface }, 5.0 * s);
            g.dl.outline(r, if primary { theme.accent } else { theme.overlay_border }, 5.0 * s, line);
            centered(g, r, label, ui, theme.text);
        }
    }

    fn draw_vault_dialog<B: TextBackend>(&self, g: &mut Gfx<B>, area: Rect, dialog: &crate::vault_settings::VaultDialog, unlocked: bool) {
        if dialog.revealed || dialog.value.is_some() {
            g.with_transient_text(|g| self.draw_vault_dialog_content(g, area, dialog, unlocked));
        } else {
            self.draw_vault_dialog_content(g, area, dialog, unlocked);
        }
    }

    fn draw_vault_dialog_content<B: TextBackend>(&self, g: &mut Gfx<B>, area: Rect, dialog: &crate::vault_settings::VaultDialog, unlocked: bool) {
        let s = self.ui();
        let theme = self.theme;
        let (mx, my) = self.mouse;
        let layout = crate::vault_settings::vault_layout(area, s, dialog, unlocked);
        let frame = layout.frame;
        let radius = (8.0 * s).round();
        let line = s.round().max(1.0);
        let ui = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let title = FontStyle::new(FontFamily::Ui, (17.0 * s).round(), true);
        let y = baseline(g, layout.title, title);
        g.dl.rect(area, theme.dim);
        g.dl.shadow(Rect::new(frame.x, frame.y + 6.0 * s, frame.w, frame.h), theme.shadow, radius, (18.0 * s).round());
        g.dl.round_rect(frame, theme.overlay, radius);
        g.dl.outline(frame, theme.overlay_border, radius, line);
        g.dl.set_clip(Some(frame));
        g.text(layout.title.x, y, dialog.kind.title(), title, theme.text, layout.title.w);
        if let Some(entry) = dialog.kind.entry() {
            let r = layout.subtitle;
            let y = baseline(g, r, small);
            g.text(r.x, y, &entry.label, small, theme.text_dim, r.w);
        }
        let menu = dialog.menu(unlocked);
        for (i, &slot) in layout.rows.iter().enumerate().filter(|(_, slot)| slot.h > 0.0) {
            let focused = i == dialog.focus && !dialog.waiting;
            if let Some(label) = menu.get(i) {
                g.dl.round_rect(slot, if slot.contains(mx, my) { theme.button_hover } else { theme.surface }, 5.0 * s);
                g.dl.outline(slot, if focused { theme.accent } else { theme.overlay_border }, 5.0 * s, line);
                let inner = slot.inset(10.0 * s, 0.0);
                let y = baseline(g, inner, ui);
                g.text(inner.x, y, t(label), ui, theme.text, inner.w);
            } else {
                self.draw_editor_field(g, slot, (dialog.field_label(i), &dialog.display(i)), ProfileFieldControl::Text, focused);
            }
        }
        if let Some(value) = dialog.value.as_deref() {
            let preview = layout.preview;
            g.dl.round_rect(preview, theme.surface, 5.0 * s);
            g.dl.outline(preview, theme.overlay_border, 5.0 * s, line);
            let mut inner = preview.inset(9.0 * s, 5.0 * s);
            inner.w = (inner.w - 5.0 * s).max(0.0);
            let pitch = 19.0 * s;
            let lines = (inner.h / pitch).floor().max(0.0) as usize;
            let mono = FontStyle::new(FontFamily::Mono, (13.0 * s).round(), false);
            let mut wrapped = Vec::new();
            for text in value.split('\n').map(|text| text.trim_end_matches('\r')) {
                if text.is_empty() {
                    wrapped.push(String::new());
                } else {
                    wrapped.extend(wrapped_info(g, text, mono, inner.w));
                }
            }
            let maximum = wrapped.len().saturating_sub(lines);
            dialog.max_scroll.set(maximum);
            let scroll = dialog.scroll.min(maximum);
            g.dl.set_clip(Some(inner));
            for (i, text) in wrapped.iter().skip(scroll).take(lines).enumerate() {
                let r = Rect::new(inner.x, inner.y + i as f32 * pitch, inner.w, pitch);
                let y = baseline(g, r, mono);
                g.text(r.x, y, text, mono, theme.text, r.w);
            }
            g.dl.set_clip(Some(frame));
            if maximum > 0 && lines > 0 {
                let thumb_h = (inner.h * lines as f32 / wrapped.len() as f32).max(10.0 * s).min(inner.h);
                let top = inner.y + (inner.h - thumb_h) * scroll as f32 / maximum as f32;
                g.dl.round_rect(Rect::new(inner.right() + 4.0 * s, top, 3.0 * s, thumb_h), theme.overlay_border, 1.5 * s);
            }
        }
        let status = if dialog.waiting { Some(t("Please wait…")) } else { dialog.issue.map(|issue| issue.message()).or_else(|| dialog.kind.note()) };
        if let Some(status) = status {
            let r = layout.status;
            let y = baseline(g, r, small);
            let ink = if dialog.issue.is_some() { Color::rgb(if theme.dark { 0xFF9490 } else { 0xA52320 }) } else { theme.text_dim };
            g.text(r.x, y, status, small, ink, r.w);
        }
        let end = if dialog.primary().is_none() || dialog.value.is_some() || dialog.waiting && !matches!(dialog.kind, crate::vault_settings::DialogKind::Startup) { t("Close") } else { t("Cancel") };
        let reveal = (dialog.can_reveal() && !dialog.waiting).then(|| if dialog.revealed { t("Hide value") } else { t("Show value") });
        for (r, label, primary) in [(layout.cancel, Some(end), false), (layout.primary, dialog.primary(), true), (layout.reveal, reveal, false)] {
            let Some(label) = label else { continue };
            let enabled = !dialog.waiting || !primary;
            g.dl.round_rect(r, if enabled && r.contains(mx, my) { theme.button_hover } else if primary && enabled { theme.accent.alpha(0x38) } else { theme.surface }, 5.0 * s);
            g.dl.outline(r, if primary && enabled { theme.accent } else { theme.overlay_border }, 5.0 * s, line);
            centered(g, r, label, ui, if enabled { theme.text } else { theme.text_dim });
        }
        g.dl.set_clip(None);
    }

    fn draw_vault_icon<B: TextBackend>(&self, g: &mut Gfx<B>, x: f32, y: f32, kind: &str) {
        let s = self.ui();
        let ink = self.theme.accent;
        let line = s.round().max(1.0);
        let tile = Rect::new(x, y, 30.0 * s, 30.0 * s).snap();
        g.dl.round_rect(tile, ink.alpha(0x28), 5.0 * s);
        let r = tile.inset(7.0 * s, 7.0 * s);
        match kind {
            "key-file" => {
                g.dl.outline(r.inset(2.0 * s, 0.0), ink, 2.0 * s, line);
                for n in [6.0, 10.0] {
                    g.dl.rect(Rect::new(r.x + 5.0 * s, r.y + n * s, 6.0 * s, line), ink);
                }
            }
            "key-passphrase" => {
                g.dl.outline(Rect::new(r.x, r.y + 4.0 * s, 8.0 * s, 8.0 * s), ink, 4.0 * s, line);
                g.dl.rect(Rect::new(r.x + 8.0 * s, r.y + 8.0 * s, 8.0 * s, line), ink);
                g.dl.rect(Rect::new(r.right() - 3.0 * s, r.y + 8.0 * s, line, 4.0 * s), ink);
            }
            _ => {
                g.dl.outline(Rect::new(r.x + 4.0 * s, r.y, 8.0 * s, 9.0 * s), ink, 3.0 * s, line);
                g.dl.round_rect(Rect::new(r.x + 2.0 * s, r.y + 6.0 * s, 12.0 * s, 10.0 * s), ink, 2.0 * s);
            }
        }
    }

    fn draw_settings<B: TextBackend>(&mut self, g: &mut Gfx<B>) {
        use crate::settings::{part_at, Control, Part, RowKind, VaultRow, SETTINGS};
        let s = self.ui();
        let ui = FontStyle::new(FontFamily::Ui, (13.5 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let (_, _, width) = crate::settings::column_widths(self.chrome.content, s);
        let Some(page) = self.settings.as_ref() else { return };
        let info_rows = page.shown.iter().filter_map(|&index| {
            let setting = SETTINGS.get(index).filter(|setting| matches!(setting.control, Control::ReadOnly(_)))?;
            Some((index, wrapped_info(g, &self.setting_text(setting), small, (width - 28.0 * s).max(1.0))))
        }).collect();
        let Some(page) = self.settings.as_mut() else { return };
        page.info_rows = info_rows;
        page.shortcut_rows.clear();
        for (n, &index) in page.shown.iter().enumerate() {
            if let Some(action) = page.shortcut(index) {
                let mut bindings: Vec<_> = self.keymap.bindings_of(&action).map(crate::keymap::format_binding).collect();
                if page.capture.as_ref().is_some_and(|(a, _)| *a == action) {
                    bindings = vec![t("Press a key, click, scroll or release modifiers").to_owned()];
                } else if bindings.is_empty() {
                    bindings.push(t("None").to_owned());
                }
                let hint = if n == page.selected {
                    page.notice.clone().unwrap_or_else(|| t("Enter replace  ·  Insert add  ·  Del clear  ·  Shift+Del restore defaults").to_owned())
                } else {
                    action.id()
                };
                let layout = crate::settings::ShortcutLayout::new(width, s, bindings, &hint, |text| g.measure(text, small));
                page.shortcut_rows.insert(index, layout);
            }
            if let Some(Control::Segments(options)) = SETTINGS.get(index).map(|setting| setting.control) {
                for &(_, label) in options {
                    page.segment_widths.insert(label, (g.measure(t(label), ui) + 16.0 * s).ceil());
                }
            }
        }
        let Some(geo) = self.settings_geometry() else { return };
        let page = self.settings.as_ref().unwrap();
        let theme = self.theme;
        let line = (1.0 * s).round().max(1.0);
        let (mx, my) = self.mouse;
        g.dl.rect(geo.page, theme.surface);
        let title = FontStyle::new(FontFamily::Ui, (20.0 * s).round(), true);
        let section = FontStyle::new(FontFamily::Ui, (12.0 * s).round(), true);
        let ty = baseline(g, geo.title, title);
        g.text(geo.title.x, ty, t("Settings"), title, theme.text, geo.title.w);
        let radius = (6.0 * s).round();
        g.dl.round_rect(geo.field, theme.overlay, radius);
        g.dl.outline(geo.field, if page.editing() { theme.overlay_border } else { theme.accent }, radius, line);
        let field_text = geo.field.inset((10.0 * s).round(), 0.0);
        let fy = baseline(g, field_text, ui);
        if page.query.is_empty() {
            g.text(field_text.x, fy, if page.searching_vault() { t("Search secrets") } else { t("Search settings") }, ui, theme.text_dim, field_text.w);
        }
        let qw = g.text(field_text.x, fy, &page.query, ui, theme.text, field_text.w);
        let caret_h = (16.0 * s).round();
        if !page.editing() {
            g.dl.rect(Rect::new(field_text.x + qw + 1.0, geo.field.y + ((geo.field.h - caret_h) / 2.0).round(), line, caret_h), theme.accent);
        }
        if geo.close.contains(mx, my) {
            g.dl.round_rect(geo.close, theme.button_hover, radius);
        }
        centered(g, geo.close, G_CLOSE, FontStyle::new(FontFamily::Icons, (10.0 * s).round(), false), theme.text);
        for (k, r) in geo.pages.iter().enumerate() {
            let current = !page.searching() && page.page == k;
            if current {
                g.dl.round_rect(*r, theme.accent.alpha(0x30), radius);
                g.dl.round_rect(Rect::new(r.x, r.y + (8.0 * s).round(), (3.0 * s).round(), (r.h - (16.0 * s).round()).max(line)), theme.accent, (1.5 * s).round());
            } else if r.contains(mx, my) {
                g.dl.round_rect(*r, theme.button_hover, radius);
            }
            let inner = r.inset((14.0 * s).round(), 0.0);
            let y = baseline(g, inner, ui);
            g.text(inner.x, y, t(crate::settings::PAGES[k]), ui, if current { theme.text } else { theme.text_dim }, inner.w);
        }
        if page.shown.is_empty() {
            centered(g, Rect::new(geo.list.x, geo.list.y, geo.list.w, (80.0 * s).round()), if page.searching_vault() { t("No secrets match") } else { t("No settings match") }, ui, theme.text_dim);
            if let Some(dialog) = &page.vault_dialog {
                self.draw_vault_dialog(g, geo.page, dialog, page.vault.unlocked);
            }
            return;
        }
        g.dl.set_clip(Some(geo.list));
        let current_scheme = page.scheme_item(self.current_scheme());
        for row in &geo.rows {
            if row.rect.bottom() < geo.list.y || row.rect.y > geo.list.bottom() {
                continue;
            }
            match row.kind {
                RowKind::Section(name) => {
                    let r = Rect::new(row.rect.x + (14.0 * s).round(), row.rect.y + (10.0 * s).round(), row.rect.w, (28.0 * s).round());
                    let y = baseline(g, r, section);
                    g.text(r.x, y, t(name), section, theme.accent, r.w);
                    if name == crate::settings::SYSTEM && !page.searching() {
                        let version = tf("Aster {version}", &[("version", env!("CARGO_PKG_VERSION"))]);
                        g.text(r.x, y + (20.0 * s).round(), &version, ui, theme.text, r.w);
                    }
                }
                RowKind::Item(n) => {
                    let selected = n == page.selected;
                    let card = row.rect.inset(0.0, (2.0 * s).round());
                    if selected {
                        g.dl.round_rect(card, theme.accent.alpha(0x26), radius);
                    } else if card.contains(mx, my) {
                        g.dl.round_rect(card, theme.button_hover, radius);
                    }
                    if let Some(action) = page.shortcut(page.shown[n]) {
                        let tx = row.rect.x + (14.0 * s).round();
                        let layout = &page.shortcut_rows[&page.shown[n]];
                        let capturing = page.capture.as_ref().is_some_and(|(a, _)| *a == action);
                        g.text(tx, row.rect.y + (24.0 * s).round(), &action.label_for(page.shortcut_name(page.shown[n])), ui, theme.text, layout.label_width);
                        for (lines, local) in &layout.chips {
                            let c = Rect::new(row.rect.x + local.x, row.rect.y + local.y, local.w, local.h);
                            g.dl.round_rect(c, theme.overlay, radius);
                            g.dl.outline(c, if capturing { theme.accent } else { theme.overlay_border }, radius, line);
                            for (k, text) in lines.iter().enumerate() {
                                let line = Rect::new(c.x, c.y + (3.0 + 16.0 * k as f32) * s, c.w, 16.0 * s);
                                centered(g, line, text, small, if capturing { theme.text_dim } else { theme.text });
                            }
                        }
                        let ink = if selected && page.notice.is_some() { theme.accent } else { theme.text_dim };
                        for (k, hint) in layout.hint_lines.iter().enumerate() {
                            let r = Rect::new(tx, row.rect.y + layout.hint_top + k as f32 * 16.0 * s, row.rect.w - 28.0 * s, 16.0 * s);
                            let y = baseline(g, r, small);
                            g.text(r.x, y, hint, small, ink, r.w);
                        }
                        continue;
                    }
                    let item = page.shown[n];
                    if let Some(vault_row) = page.vault_at(item) {
                        let (label, hint) = vault_row.labels(&page.vault);
                        let mut tx = row.rect.x + (14.0 * s).round();
                        let c = row.control;
                        if let VaultRow::Entry(i) = vault_row {
                            self.draw_vault_icon(g, tx, row.rect.y + (row.rect.h - 30.0 * s) * 0.5, &page.vault.entries[i].kind);
                            tx += (40.0 * s).round();
                        }
                        let tw = (c.x - tx - 16.0 * s).max(0.0);
                        let notice = selected.then(|| page.vault_notice()).flatten();
                        let detail = notice.unwrap_or(hint);
                        let y = if detail.is_empty() { baseline(g, row.rect, ui) } else { row.rect.y + (25.0 * s).round() };
                        g.text(tx, y, label, ui, theme.text, tw);
                        if !detail.is_empty() {
                            g.text(tx, row.rect.y + (43.0 * s).round(), detail, small, if notice.is_some() { theme.accent } else { theme.text_dim }, tw);
                        }
                        let button = match vault_row {
                            VaultRow::Create => t("Set password"),
                            VaultRow::Unlock => t("Unlock"),
                            VaultRow::Manage | VaultRow::Entry(_) => t("Actions…"),
                            VaultRow::Import => t("Import"),
                        };
                        g.dl.round_rect(c, if c.contains(mx, my) { theme.button_hover } else { theme.overlay }, radius);
                        g.dl.outline(c, if selected { theme.accent } else { theme.overlay_border }, radius, line);
                        centered(g, c, button, ui, theme.text);
                        continue;
                    }
                    if let Some(scheme) = page.scheme_at(item) {
                        let c = row.control;
                        let tx = row.rect.x + (14.0 * s).round();
                        let tw = (c.x - tx - 16.0 * s).max(0.0);
                        let mut detail = if scheme.custom { t("Custom") } else { t("Built-in") }.to_owned();
                        if current_scheme == Some(item) {
                            detail.push_str(" · ");
                            detail.push_str(t("Current"));
                        }
                        g.text(tx, row.rect.y + (25.0 * s).round(), &scheme.name, ui, theme.text, tw);
                        g.text(tx, row.rect.y + (43.0 * s).round(), &detail, small, theme.text_dim, tw);
                        g.dl.round_rect(c, Color::rgb(scheme.background), radius);
                        g.dl.outline(c, theme.overlay_border, radius, line);
                        let pad = (6.0 * s).round();
                        let inner = c.inset(pad, pad);
                        let sample_y = baseline(g, inner, small);
                        let sample_w = g.text(inner.x, sample_y, t("Aa"), small, Color::rgb(scheme.foreground), inner.w * 0.25);
                        let grid_x = inner.x + sample_w.ceil() + pad;
                        let gap = 3.0 * s;
                        let cell_w = (inner.right() - grid_x - 7.0 * gap) / 8.0;
                        let cell_h = (inner.h - gap) / 2.0;
                        for (i, &color) in scheme.colors.iter().enumerate() {
                            let x = grid_x + (i % 8) as f32 * (cell_w + gap);
                            let y = inner.y + (i / 8) as f32 * (cell_h + gap);
                            g.dl.round_rect(Rect::new(x, y, cell_w, cell_h).snap(), Color::rgb(color), 2.0 * s);
                        }
                        continue;
                    }
                    if let Some(tree) = page.tree_at(item) {
                        self.draw_profile_tree_row(g, page, tree, row.rect, selected);
                        continue;
                    }
                    let setting = &SETTINGS[page.shown[n]];
                    let tx = row.rect.x + (14.0 * s).round();
                    if let Some(lines) = page.info_rows.get(&page.shown[n]) {
                        let tw = (row.rect.w - 28.0 * s).max(1.0);
                        g.text(tx, row.rect.y + (25.0 * s).round(), t(setting.label), ui, theme.text, tw);
                        for (k, text) in lines.iter().enumerate() {
                            g.text(tx, row.rect.y + (43.0 * s).round() + k as f32 * (16.0 * s).round(), text, small, theme.text_dim, tw);
                        }
                        continue;
                    }
                    let tw = (row.control.x - tx - 16.0 * s).max(0.0);
                    g.text(tx, row.rect.y + (25.0 * s).round(), t(setting.label), ui, theme.text, tw);
                    let notice = if selected && setting.key == "vault.remember-minutes" { page.vault_notice() } else if selected { page.notice.as_deref() } else { None };
                    let hint = notice.unwrap_or_else(|| t(setting.hint));
                    g.text(tx, row.rect.y + (43.0 * s).round(), hint, small, if notice.is_some() { theme.accent } else { theme.text_dim }, tw);
                    let c = row.control;
                    match setting.control {
                        Control::ReadOnly(_) => {}
                        Control::Segments(options) => {
                            let value = crate::settings::get(&self.config, setting.key);
                            let active = value.as_ref().and_then(|v| v.as_str());
                            g.dl.round_rect(c, theme.overlay, radius);
                            for (segment, &(key, label)) in page.segments(options, c, s).into_iter().zip(options) {
                                let chosen = active == Some(key);
                                if chosen || segment.contains(mx, my) {
                                    g.dl.round_rect(segment, if chosen { theme.accent } else { theme.button_hover }, radius);
                                }
                                let inner = segment.inset(3.0 * s, 0.0);
                                let text = t(label);
                                let width = g.measure(text, ui).min(inner.w);
                                let y = baseline(g, inner, ui);
                                g.text(inner.x + ((inner.w - width) / 2.0).round(), y, text, ui, if chosen { theme.surface } else { theme.text }, inner.w);
                            }
                            g.dl.outline(c, theme.overlay_border, radius, line);
                        }
                        Control::Toggle => {
                            let on = self.setting_on(setting);
                            g.dl.round_rect(c, if on { theme.accent } else { theme.overlay_border }, c.h / 2.0);
                            let knob = (c.h - 6.0 * s).round();
                            let kx = if on { c.right() - knob - 3.0 * s } else { c.x + 3.0 * s };
                            g.dl.circle(Rect::new(kx.round(), c.y + ((c.h - knob) / 2.0).round(), knob, knob), if on { Color::rgb(0xFFFFFF) } else { theme.text_dim });
                        }
                        Control::Run(action) => {
                            g.dl.round_rect(c, if c.contains(mx, my) { theme.button_hover } else { theme.overlay }, radius);
                            g.dl.outline(c, if selected { theme.accent } else { theme.overlay_border }, radius, line);
                            let label = match action {
                                crate::keymap::Action::ApplicationsMenu(true) => t("Add"),
                                crate::keymap::Action::ApplicationsMenu(false) => t("Remove"),
                                crate::keymap::Action::StartMenuShortcut => if self.start_menu_shortcut { t("Remove shortcut") } else { t("Create shortcut") },
                                crate::keymap::Action::EditEnvironment => t("Edit"),
                                _ => t("Open"),
                            };
                            centered(g, c, label, ui, theme.text);
                        }
                        control => {
                            let editing = selected && page.editing() && matches!(control, Control::Text | Control::StringList | Control::Font { .. } | Control::Number { .. });
                            g.dl.round_rect(c, theme.overlay, radius);
                            g.dl.outline(c, if editing { theme.accent } else { theme.overlay_border }, radius, line);
                            let stepper = !matches!(control, Control::Text | Control::StringList | Control::Font { .. });
                            let edge = if stepper { (30.0 * s).round() } else { (10.0 * s).round() };
                            let inner = Rect::new(c.x + edge, c.y, (c.w - 2.0 * edge).max(0.0), c.h);
                            if stepper {
                                let hover = c.contains(mx, my).then(|| part_at(control, c, s, mx));
                                let stroke = (1.4 * s).max(1.0);
                                let size = (9.0 * s).round();
                                let less = Rect::new(c.x, c.y, edge, c.h);
                                let more = Rect::new(c.right() - edge, c.y, edge, c.h);
                                for (r, part) in [(less, Part::Less), (more, Part::More)] {
                                    if hover == Some(part) {
                                        g.dl.round_rect(r.inset(2.0 * s, 2.0 * s), theme.button_hover, radius);
                                    }
                                    let center = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                                    let ink = theme.text_dim;
                                    if matches!(control, Control::Number { .. }) {
                                        let arm = (5.0 * s).round();
                                        g.dl.rect(Rect::new((center.0 - arm).round(), (center.1 - line / 2.0).round(), 2.0 * arm, line), ink);
                                        if part == Part::More {
                                            g.dl.rect(Rect::new((center.0 - line / 2.0).round(), (center.1 - arm).round(), line, 2.0 * arm), ink);
                                        }
                                    } else {
                                        g.dl.chevron(center, size, if part == Part::Less { 180.0 } else { 0.0 }, stroke, ink);
                                    }
                                }
                            }
                            let text = if editing { page.edit_text() } else { self.setting_text(setting) };
                            let y = baseline(g, inner, ui);
                            if editing {
                                if page.font_replace && !text.is_empty() {
                                    let w = g.measure(&text, ui).min(inner.w);
                                    g.dl.rect(Rect::new(inner.x, c.y + 4.0 * s, w, c.h - 8.0 * s), theme.selection);
                                }
                                let w = g.text(inner.x, y, &text, ui, theme.text, inner.w);
                                g.dl.rect(Rect::new(inner.x + w + 1.0, c.y + ((c.h - caret_h) / 2.0).round(), line, caret_h), theme.accent);
                            } else if stepper {
                                let w = g.measure(&text, ui).min(inner.w);
                                g.text((inner.x + (inner.w - w) / 2.0).round(), y, &text, ui, theme.text, inner.w);
                            } else {
                                g.text(inner.x, y, &text, ui, theme.text, inner.w);
                            }
                        }
                    }
                }
            }
        }
        for &(index, r) in &geo.suggestions {
            g.dl.rect(r, theme.surface);
            g.dl.rect(r, if index == page.font_index { theme.accent.alpha(0x42) } else if r.contains(mx, my) { theme.button_hover } else { theme.overlay });
            let inner = r.inset(10.0 * s, 0.0);
            let y = baseline(g, inner, ui);
            g.text(inner.x, y, &page.font_matches[index], ui, theme.text, inner.w);
        }
        if let (Some((_, first)), Some((_, last))) = (geo.suggestions.first(), geo.suggestions.last()) {
            g.dl.outline(Rect::new(first.x, first.y, first.w, last.bottom() - first.y), theme.overlay_border, 0.0, line);
        }
        g.dl.set_clip(None);
        if let Some(dialog) = &page.vault_dialog {
            self.draw_vault_dialog(g, geo.page, dialog, page.vault.unlocked);
            return;
        }
        if page.editor.is_some() || page.group_editor.is_some() {
            self.draw_profile_editor(g, geo.page, page);
            return;
        }
        if geo.height > geo.list.h {
            let track = geo.list;
            let thumb_h = (track.h * track.h / geo.height).max(24.0 * s);
            let max_scroll = geo.height - track.h;
            let thumb_y = track.y + (track.h - thumb_h) * (page.scroll / max_scroll).clamp(0.0, 1.0);
            let w = (4.0 * s).round();
            g.dl.round_rect(Rect::new(track.right() + (8.0 * s).round(), thumb_y.round(), w, thumb_h.round()), theme.overlay_border, w / 2.0);
        }
    }

    fn draw_overlay<B: TextBackend>(&mut self, g: &mut Gfx<B>, o: &Overlay) {
        let s = self.ui();
        let theme = self.theme;
        let geo = overlay_geometry(self, o);
        let radius = (8.0 * s).round();
        g.dl.shadow(Rect::new(geo.frame.x, geo.frame.y + 6.0 * s, geo.frame.w, geo.frame.h), theme.shadow, radius, (18.0 * s).round());
        g.dl.round_rect(geo.frame, theme.overlay, radius);
        g.dl.outline(geo.frame, theme.overlay_border, radius, (1.0 * s).round().max(1.0));
        let ui = FontStyle::new(FontFamily::Ui, (13.5 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let tx = geo.input.x + (14.0 * s).round();
        let y = baseline(g, geo.input, ui);
        if o.query.is_empty() {
            g.text(tx, y, o.placeholder(), ui, theme.text_dim, geo.input.w - 28.0 * s);
        }
        let w = g.text(tx, y, &o.query, ui, theme.text, geo.input.w - 28.0 * s);
        let caret_h = (18.0 * s).round();
        g.dl.rect(Rect::new(tx + w + 1.0, geo.input.y + ((geo.input.h - caret_h) / 2.0).round(), (1.0 * s).round().max(1.0), caret_h), theme.accent);
        g.dl.rect(Rect::new(geo.frame.x, geo.input.bottom(), geo.frame.w, (1.0 * s).round().max(1.0)), theme.overlay_border);
        if o.shown.is_empty() {
            let r = Rect::new(geo.frame.x, geo.input.bottom(), geo.frame.w, geo.frame.bottom() - geo.input.bottom());
            let message = if matches!(o.kind, OverlayKind::SerialSpeed(_)) { t("Enter a speed from 50 to 4000000 baud") } else { t("No matches") };
            centered(g, r, message, ui, theme.text_dim);
            return;
        }
        let (mx, my) = self.mouse;
        let bold = FontStyle::new(FontFamily::Ui, (12.0 * s).round(), true);
        for (title, r) in &geo.headers {
            let y = baseline(g, *r, bold);
            g.text(r.x + (10.0 * s).round(), y, title, bold, theme.text_dim, r.w);
        }
        for r in &geo.dividers {
            g.dl.rect(*r, theme.overlay_border);
        }
        let profiles = o.kind.is_profile_picker();
        for (i, r) in &geo.items {
            let (item, _) = &o.shown[*i];
            let pick = &o.items[*item];
            let chosen = *i == o.selected;
            if chosen {
                g.dl.round_rect(*r, theme.accent.alpha(0x38), (5.0 * s).round());
            } else if r.contains(mx, my) {
                g.dl.round_rect(*r, theme.button_hover, (5.0 * s).round());
            }
            let y = baseline(g, *r, ui);
            let mut x = r.x + (10.0 * s).round();
            if profiles {
                let side = (22.0 * s).round();
                let mark = Rect::new(x, r.y + ((r.h - side) / 2.0).round(), side, side);
                match pick.tint {
                    Some(rgb) => {
                        g.dl.round_rect(mark, Color::rgb(rgb), (5.0 * s).round());
                        let size = if pick.badge.chars().count() > 2 { 8.5 } else { 10.0 };
                        centered(g, mark, &pick.badge, FontStyle::new(FontFamily::Ui, (size * s).round(), true), Color::rgb(0xFFFFFF));
                    }
                    None => centered(g, mark, &pick.badge, FontStyle::new(FontFamily::Ui, (13.0 * s).round(), false), theme.text_dim),
                }
                x = mark.right() + (10.0 * s).round();
            }
            let mut right = r.right() - (10.0 * s).round();
            if profiles && chosen {
                let label = t("Enter →");
                let tag = FontStyle::new(FontFamily::Ui, (10.0 * s).round(), true);
                let tw = g.measure(label, tag) + (12.0 * s).round();
                let tag_rect = Rect::new(right - tw, r.y + ((r.h - 18.0 * s) / 2.0).round(), tw.round(), (18.0 * s).round());
                g.dl.round_rect(tag_rect, theme.surface, (4.0 * s).round());
                g.dl.outline(tag_rect, theme.overlay_border, (4.0 * s).round(), (1.0 * s).round().max(1.0));
                centered(g, tag_rect, label, tag, theme.text);
                right = tag_rect.x - (8.0 * s).round();
            }
            let label_w = g.measure(&pick.label, ui).min(right - x);
            g.text(x, y, &pick.label, ui, theme.text, right - x);
            if !pick.hint.is_empty() && pick.hint != pick.label {
                let hy = baseline(g, *r, small);
                if profiles {
                    let hx = x + label_w + (10.0 * s).round();
                    if right - hx > 24.0 * s {
                        g.text(hx, hy, &pick.hint, small, theme.text_dim, right - hx);
                    }
                } else {
                    let hint_w = g.measure(&pick.hint, small).min(right - x);
                    g.text(right - hint_w, hy, &pick.hint, small, theme.text_dim, hint_w + 1.0);
                }
            }
        }
    }
}

pub fn hint_height(scale: f32) -> f32 {
    (30.0 * scale).round()
}
