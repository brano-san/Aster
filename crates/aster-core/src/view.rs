use crate::app::{App, Drag, Effect, Metrics};
use crate::hover::{fill as hover_fill, opacity, TabHover};
use crate::i18n::{t, tf, tn};
use crate::input::HitArea;
use crate::overlay::{Overlay, OverlayKind, VISIBLE};
use crate::busy::{Busy, ProgressBar};
use crate::layers::Layer;
use crate::motion::{POP, TOAST_IN, TOAST_OUT, UNFOLD};
use crate::panes::PaneId;
use crate::tab_motion::{StripFrame, TabGhost};
use crate::theme::color;
use aster_gfx::render::{Color, Rect};
use aster_gfx::text::{FontFamily, FontStyle, Gfx, TextBackend};
use aster_vt::{CursorShape, Flags, Rgb, Size};
use std::time::Instant;

pub use crate::tabs::{bare_chrome, chrome, Chrome, TabSlot, TAB_H};
const PAD: f32 = 6.0;
const G_CLOSE: &str = "\u{E8BB}";
const G_PLUS: &str = "\u{E710}";
const G_SETTINGS: &str = "\u{E713}";
const G_MIN: &str = "\u{E921}";
const G_MAX: &str = "\u{E922}";
const G_RESTORE: &str = "\u{E923}";

#[derive(Clone, Copy)]
enum ProfileFieldControl {
    Text,
    ReadOnly,
    Choice { editable: bool },
    Picker,
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

fn spinner(dl: &mut aster_gfx::render::DrawList, area: Rect, phase: f32, scale: f32, ink: Color) {
    let (x, y) = ((area.x + area.w / 2.0).round(), (area.y + area.h / 2.0).round());
    let radius = area.w.min(area.h).min(14.0 * scale) * 0.35;
    let start = std::f32::consts::TAU * (phase - 0.25);
    let points: [(f32, f32); 17] = std::array::from_fn(|i| {
        let (sin, cos) = (start + i as f32 / 16.0 * 1.5 * std::f32::consts::PI).sin_cos();
        (x + radius * cos, y + radius * sin)
    });
    dl.polyline(&points, (1.6 * scale).max(1.0), ink);
}

fn draw_busy_spinner(dl: &mut aster_gfx::render::DrawList, busy: &mut Busy, area: Rect, scale: f32, ink: Color) {
    if area.w.min(area.h) >= 8.0 * scale {
        spinner(dl, area, busy.phase(), scale, ink);
    }
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

fn color_field<B: TextBackend>(g: &mut Gfx<B>, rect: Rect, text: &str, theme: crate::theme::Theme, s: f32) -> Rect {
    let size = (16.0 * s).round().min(rect.w);
    let chip = Rect::new(rect.x, rect.y + (rect.h - size) / 2.0, size, size);
    if let Some(color) = crate::settings::color_value(text) {
        g.dl.round_rect(chip, color, 3.0 * s);
    }
    g.dl.outline(chip, theme.overlay_border, 3.0 * s, 1.0);
    let offset = (size + 8.0 * s).min(rect.w);
    Rect::new(rect.x + offset, rect.y, rect.w - offset, rect.h)
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
    pub list: Rect,
    pub items: Vec<(usize, Rect)>,
    pub headers: Vec<(String, Rect)>,
    pub dividers: Vec<Rect>,
}

pub fn overlay_geometry(app: &App, o: &Overlay) -> OverlayGeometry {
    if o.kind.scrolls_smoothly() {
        let layout = crate::overlay_list::OverlayList::new(app.chrome.content, app.ui(), o);
        return layout.geometry(layout.target(o));
    }
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
    let list = Rect::new(x, input.bottom(), width, (frame.bottom() - input.bottom()).max(0.0));
    OverlayGeometry { frame, input, list, items, headers, dividers }
}

pub fn overlay_hit(app: &App, o: &Overlay, x: f32, y: f32) -> Option<Option<usize>> {
    let geo = overlay_geometry(app, o);
    if !geo.frame.contains(x, y) {
        return None;
    }
    Some(overlay_row_at(&geo, (x, y)).filter(|&(i, r)| {
        let item = &o.items[o.shown[i].0];
        !matches!(o.kind, OverlayKind::SshPorts(_)) || item.badge.is_empty() || crate::ssh_toolbar::forward_action_rect(r, app.ui()).contains(x, y)
    }).map(|(i, _)| i))
}

fn overlay_row_at(geo: &OverlayGeometry, (x, y): (f32, f32)) -> Option<(usize, Rect)> {
    if !geo.list.contains(x, y) { return None; }
    geo.items.iter().find(|(_, r)| r.contains(x, y)).copied()
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
        self.frame_at(g, maximized, self.clock());
    }

    pub(crate) fn frame_at<B: TextBackend>(&mut self, g: &mut Gfx<B>, maximized: bool, now: std::time::Instant) {
        self.motion = false;
        self.prompt_pending_serial();
        let terminal = &self.config.terminal;
        g.set_mono_font(&terminal.font, [terminal.font_weight, terminal.font_weight_bold]);
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
        self.layers.begin(g.atlas_generation(), now, self.animations());
        self.busy.begin(now, self.animations());
        g.retain_instances(self.layers.retained());
        g.set_background_image(self.background.bitmap.as_ref().map(|bitmap| (self.background.revision, bitmap.size, bitmap.rgba.as_slice())));
        if self.config.appearance.background_image_opacity > 0.0 && self.focused_pane().is_some_and(|pane| self.panes.contains_key(&pane)) {
            g.prepare_background_image();
        }
        if !self.tab_bar_hidden() {
            g.dl.rect(self.chrome.tab_bar, theme.chrome);
            if self.chrome.caption != self.chrome.tab_bar {
                g.dl.rect(self.chrome.caption, theme.chrome);
            }
            self.draw_tab_bar(g, maximized, now);
        } else {
            self.hover_state.tabs = Default::default();
            self.hover_state.window = Default::default();
            self.tab_strip = Default::default();
        }
        let content = self.chrome.content;
        g.dl.rect(content, theme.background);
        let sidebar = self.sidebar_geometry().map(|geo| (geo.rect.w, geo.rect));
        let slide_sidebar = self.animations() && !matches!(self.drag, Drag::SidebarResize { .. });
        let sidebar_width = self.panel_motion.sidebar.update(sidebar, slide_sidebar, now, &mut self.motion);
        let sidebar_gap = if sidebar_width < self.chrome.profile_sidebar.w { self.chrome.profile_sidebar.w } else { 0.0 };
        let closing_sidebar_right = if sidebar.is_none() && sidebar_width > 0.0 {
            content.x + self.panel_motion.sidebar.content.map_or(0.0, |(full, _)| full)
        } else {
            content.x
        };
        if sidebar_gap > 0.0 {
            g.dl.rect(self.chrome.profile_sidebar, theme.background);
        }
        let gap = (1.0 * s).round().max(1.0);
        let layout = match self.tabs.get(self.active) {
            Some(tab) => match tab.zoomed {
                Some(z) => crate::panes::Layout { panes: vec![(z, content)], splitters: Vec::new() },
                None => tab.root.layout(content, gap),
            },
            None => crate::panes::Layout::default(),
        };
        self.layout = layout.clone();
        let tab = self.tabs.get(self.active);
        let animate_panes = self.animations() && !matches!(self.drag, Drag::Splitter { .. } | Drag::SidebarResize { .. });
        self.motion |= self.pane_motion.settle(tab.map(|t| t.key), content, &layout, tab.and_then(|t| t.zoomed), animate_panes, now);
        let focus = self.focused_pane();
        let split = layout.panes.len() > 1;
        let broadcast = self.broadcast_targets();
        let show_pane_busy = self.settings.is_none() && self.files.is_none()
            && self.vault_prompt.as_ref().is_none_or(|dialog| dialog.kind != crate::vault_settings::DialogKind::Startup);
        let blink = self.cursor_blink(now);
        self.panel_motion.headers.retain(|id, _| self.panes.contains_key(id));
        for (id, rect) in &layout.panes {
            let visual = self.pane_motion.pane(*id, now);
            let pane_from = g.dl.instances.len();
            let gap = if rect.x == content.x { sidebar_gap } else { 0.0 };
            self.draw_pane(g, *id, (*rect, visual), focus == Some(*id), (m, gap, closing_sidebar_right), blink);
            if split && focus != Some(*id) {
                g.dl.rect(visual, theme.dim);
            }
            let password = self.password_prompt(*id).and_then(|prompt| {
                if prompt.offer {
                    Some(t("Save this password for the next prompt here? Ctrl+Shift+Enter saves it; any other key dismisses"))
                } else if prompt.can_fill() {
                    Some(t("A saved password matches this prompt. Ctrl+Shift+Enter types it"))
                } else {
                    None
                }
            });
            self.draw_popup_hint(g, *rect, Layer::Password(*id), password);
            let broadcast_hint = broadcast.contains(id).then(|| {
                if self.broadcast == Some(crate::app::Broadcast::AllTabs) { t("Typing goes to every tab. Click here to stop.") } else { t("Typing goes to every pane of this tab. Click here to stop.") }
            });
            self.draw_popup_hint(g, *rect, Layer::Broadcast(*id), broadcast_hint);
            let layer = Layer::Transfer(*id);
            let transfer = self.offers.get(id);
            let from = self.layers.start(&mut g.dl, layer, self.layers.opened(layer, transfer.is_some()), POP);
            if let Some((state, typed)) = transfer {
                let running = *state == crate::app::PaneTransfer::Running && show_pane_busy;
                let frame = self.draw_offer(g, *rect, *state, typed, running);
                if running {
                    let area = Rect::new(frame.x + (12.0 * s).round(), frame.y, (16.0 * s).round(), frame.h);
                    draw_busy_spinner(&mut g.dl, &mut self.busy, area, s, theme.accent);
                }
                self.layers.pop(&mut g.dl, layer, from, frame, 0, POP);
            }
            let flash = self.bell_alpha(*id, now);
            if flash > 0.0 {
                self.motion = true;
                g.dl.rect(*rect, theme.foreground.alpha((44.0 * flash).round() as u8));
            }
            self.draw_pane_header(g, *id, *rect, focus == Some(*id), show_pane_busy, now);
            let layer = Layer::SshAuth(*id);
            let panel = self.ssh_auth.get(id);
            let from = self.layers.start(&mut g.dl, layer, panel.map(|panel| panel.opened_at), POP);
            if let Some(panel) = panel {
                let pane = self.pane_terminal_rect(*id, *rect);
                let geo = panel.geometry(pane, s);
                let phase = (panel.pending && show_pane_busy && geo.entries.first().is_some_and(|(_, row)| row.w >= 24.0 * s))
                    .then(|| self.busy.phase());
                let frame = self.draw_ssh_auth(g, pane, panel, geo, focus == Some(*id), phase);
                self.layers.pop(&mut g.dl, layer, from, frame, 1, POP);
            }
            if visual != *rect {
                g.dl.confine(pane_from, visual);
            }
        }
        for sp in &layout.splitters {
            g.dl.rect(self.pane_motion.splitter(&sp.path, now), theme.divider);
        }
        self.draw_pane_rearrangement(g, now);
        let mut controls = std::mem::take(&mut self.control_motion);
        let mut control_frame = controls.begin(now, self.animations());
        let sidebar_above_settings = sidebar.is_none() && self.settings.as_ref().is_some_and(|page| {
            page.editor.is_none() && page.group_editor.is_none() && page.scheme_editor.is_none() && page.vault_dialog.is_none()
        });
        if !sidebar_above_settings {
            self.draw_sliding_sidebar(g, sidebar_width, now, &mut control_frame);
        }
        if self.vault_prompt.as_ref().is_some_and(|dialog| dialog.kind == crate::vault_settings::DialogKind::Startup) {
            g.dl.rect(content, theme.surface);
            self.draw_window_vault(g, content);
            self.motion |= self.layers.finish() | self.busy.finish() | control_frame.finish();
            self.control_motion = controls;
            self.mark_drawn(now);
            return;
        }
        if self.tabs.is_empty() {
            self.draw_start_page(g);
        }
        let content_layers = g.dl.instances.len();
        self.draw_sliding_search(g, now);
        self.draw_settings(g, now, &mut control_frame);
        if sidebar_above_settings {
            self.draw_sliding_sidebar(g, sidebar_width, now, &mut control_frame);
        }
        self.motion |= control_frame.finish();
        self.control_motion = controls;
        self.draw_sliding_files(g, now);
        self.draw_overlay(g, now);
        if let Some(form) = &self.ssh_forward_form {
            if let Some(caret) = self.draw_ssh_forward_form(g, content, form) {
                self.set_ime_area(caret);
            }
        }
        self.draw_update_notice(g);
        self.draw_paste(g, m);
        self.draw_close(g);
        self.draw_tab_toast(g, now);
        self.draw_window_vault(g, content);
        g.dl.confine(content_layers, content);
        self.draw_menu(g, now);
        self.draw_mru(g);
        self.motion |= self.layers.finish() | self.busy.finish();
        self.mark_drawn(now);
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

    fn draw_sliding_sidebar<B: TextBackend>(&mut self, g: &mut Gfx<B>, width: f32, now: Instant, controls: &mut crate::control_motion::Frame<'_>) {
        let Some((full, mut rect)) = self.panel_motion.sidebar.content else {
            self.hover_state.profiles = Default::default();
            return;
        };
        let open = self.sidebar_geometry().is_some();
        if !open {
            rect.x = self.chrome.content.x;
            rect.y = self.chrome.content.y;
            rect.h = self.chrome.content.h;
        }
        let from = g.dl.instances.len();
        let geo = self.sidebar_geometry_at(rect);
        let max = self.profile_sidebar.rows.len().saturating_sub(geo.visible()) as f32;
        self.profile_sidebar.scroll = self.profile_sidebar.scroll.min(max);
        let animate = self.animations();
        let scroll = self.list_motion.sidebar.sample(self.profile_sidebar.scroll, max, animate, now, &mut self.motion);
        let (mx, my) = self.mouse;
        let hot = if open { geo.hit_row(mx, my, self.profile_sidebar.scroll).filter(|&i| i < self.profile_sidebar.rows.len()) } else { None };
        self.motion |= self.hover_state.profiles.aim(hot, animate, now);
        self.draw_profile_sidebar(g, geo, (open, scroll), now, controls);
        if width <= 0.0 {
            g.dl.instances.truncate(from);
        } else if width < full {
            g.dl.shift(from, width - full, 0.0);
            g.dl.confine(from, Rect::new(rect.x, rect.y, width, rect.h));
        }
    }

    fn draw_profile_sidebar<B: TextBackend>(&mut self, g: &mut Gfx<B>, geo: crate::profile_sidebar::Geometry, (open, scroll): (bool, f32), now: Instant, controls: &mut crate::control_motion::Frame<'_>) {
        use crate::profile_sidebar::Focus;
        use crate::profiles_page::TreeKind;
        let s = self.ui();
        let theme = self.theme;
        let (mx, my) = if open { self.mouse } else { (-1.0, -1.0) };
        let line = (1.0 * s).round().max(1.0);
        let ui = FontStyle::new(FontFamily::Ui, (12.0 * s).round(), false);
        let bold = FontStyle::new(FontFamily::Ui, ui.px(), true);
        let radius = (5.0 * s).round();
        let visible = geo.visible();
        let max_scroll = self.profile_sidebar.rows.len().saturating_sub(visible);
        let browser = &self.profile_sidebar;
        let focused = open && browser.focus == Focus::Filter;
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
        let list = geo.row_view();
        g.dl.set_clip(Some(list));
        let first = scroll.floor() as usize;
        for (position, row) in browser.rows.iter().enumerate().skip(first).take(visible + 1) {
            let r = geo.row(position, scroll);
            if r.y >= list.bottom() { break; }
            let selected = open && browser.selected == position && browser.focus != Focus::None;
            if selected {
                g.dl.round_rect(r, theme.accent.alpha(0x30), radius);
            } else if let Some(fill) = hover_fill(&self.hover_state.profiles, position, now, theme.button_hover) {
                g.dl.round_rect(r, fill, radius);
            }
            let x = r.x + 8.0 * s + row.indent(s).min((r.w - 44.0 * s).max(0.0));
            let end = r.right() - 7.0 * s;
            match &row.kind {
                TreeKind::Header(header) => {
                    let open = !browser.query.trim().is_empty() || !browser.collapsed.contains(header);
                    let angle = controls.angle(crate::control_motion::Control::Group(true, header.clone()), open);
                    g.dl.chevron((x + 4.0 * s, r.y + r.h * 0.5), 7.0 * s, angle, 1.5 * s, theme.text_dim);
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
            let r = geo.row(0, scroll);
            let y = baseline(g, r, ui);
            g.text(x, y, t("Nothing matches the filter"), ui, theme.text_dim, width);
        }
        g.dl.set_clip(Some(geo.rect));
        if max_scroll > 0 && visible > 0 {
            let height = (geo.list.h * visible as f32 / browser.rows.len() as f32).max(12.0 * s);
            let y = (geo.list.y + (geo.list.h - height) * scroll / max_scroll as f32).round();
            g.dl.round_rect(Rect::new(geo.rect.right() - 4.0 * s, y, 2.0 * s, height), theme.text_dim.alpha(0x80), s);
        }
        g.dl.set_clip(None);
        if let Some(caret) = caret {
            self.set_ime_area(caret);
        }
    }

    fn draw_pane_rearrangement<B: TextBackend>(&mut self, g: &mut Gfx<B>, now: Instant) {
        let s = self.ui();
        let theme = self.theme;
        let line = (1.0 * s).round().max(1.0);
        let rearranging = self.rearranging();
        let drop = self.pane_drag_hover().map(|(rect, zone)| (rect, zone, true))
            .or_else(|| self.tab_drag_hover().map(|(rect, zone)| (rect, zone, false)));
        let animate = self.animations();
        let (labels_alpha, drop_alpha, moving) = self.rearrange_motion.settle(rearranging, drop.map(|(rect, _, _)| rect), animate, now);
        self.motion |= moving;
        if let Some(state) = self.rearrange.as_ref().filter(|_| rearranging && labels_alpha > 0.0) {
            let from = g.dl.instances.len();
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
                centered(g, badge, &label, ui, if selected { theme.on_accent } else { theme.text });
            }
            g.dl.set_clip(None);
            if labels_alpha < 1.0 {
                g.dl.fade(from, labels_alpha);
            }
        }
        if let Some((rect, active, center_swap)) = drop.filter(|_| drop_alpha > 0.0) {
            let from = g.dl.instances.len();
            self.draw_drop_zones(g, rect, active, center_swap);
            if drop_alpha < 1.0 {
                g.dl.fade(from, drop_alpha);
            }
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

    fn draw_tab_bar<B: TextBackend>(&mut self, g: &mut Gfx<B>, maximized: bool, now: Instant) {
        let s = self.ui();
        let theme = self.theme;
        let c = self.chrome.clone();
        let ui = FontStyle::new(FontFamily::Ui, (12.5 * s).round(), false);
        let icons = FontStyle::new(FontFamily::Icons, (10.0 * s).round(), false);
        let (mx, my) = self.mouse;
        let radius = (8.0 * s).round();
        let hot = if c.plus.contains(mx, my) {
            Some(TabHover::Plus)
        } else if c.tab_menu.contains(mx, my) {
            Some(TabHover::Menu)
        } else if c.settings.contains(mx, my) {
            Some(TabHover::Settings)
        } else {
            c.tabs.iter().zip(&self.tabs).find(|(slot, _)| slot.rect.contains(mx, my)).map(|(slot, tab)| {
                if slot.close.contains(mx, my) { TabHover::Close(tab.key) } else { TabHover::Tab(tab.key) }
            })
        };
        let window_hot = self.hit_test(mx, my);
        let window_hot = matches!(window_hot, HitArea::Minimize | HitArea::Maximize | HitArea::Close).then_some(window_hot);
        let animate = self.animations();
        self.motion |= self.hover_state.tabs.aim(hot, animate, now);
        self.motion |= self.hover_state.window.aim(window_hot, animate, now);
        let bar = if c.tab_bar == c.caption { Rect::new(c.tab_bar.x, c.tab_bar.y, c.min.x, c.tab_bar.h) } else { c.tab_bar };
        let targets: Vec<_> = self.tabs.iter().zip(&c.tabs).map(|(tab, slot)| (tab.key, slot.rect)).collect();
        let active = self.tabs.get(self.active).map(|tab| tab.key);
        let drag = match self.drag {
            Drag::Tab { index, grab, moved: true, .. } => self.tabs.get(index).map(|tab| {
                let at = if c.vertical { my - grab.1 } else { mx - grab.0 };
                (index, tab.key, at)
            }),
            _ => None,
        };
        self.motion |= self.tab_strip.settle(&targets, active, drag.map(|(_, key, at)| (key, at)), StripFrame {
            bar, vertical: c.vertical, plus: c.plus, menu: c.tab_menu, animate, now,
        });
        let strip_start = g.dl.instances.len();
        for (r, ghost) in self.tab_strip.ghosts(now) {
            if r.w > 0.0 && r.h > 0.0 {
                g.dl.set_clip(Some(r));
                self.draw_tab_mark(g, r, ghost.color);
                if ghost.pinned {
                    self.draw_pinned_tab(g, r, ui, ghost, false, false);
                } else {
                    let x = r.x + (12.0 * s).round();
                    let y = baseline(g, r, ui);
                    g.text(x, y, &ghost.label, ui, theme.text_dim, (r.right() - x).max(0.0));
                }
            }
        }
        let active_dragged = drag.is_some_and(|(i, _, _)| i == self.active);
        if let Some(r) = self.tab_strip.indicator(now).filter(|_| !active_dragged) {
            self.draw_active_tab(g, r, radius);
        }
        let drag_index = drag.map(|(i, _, _)| i);
        let mut dragged_instances = None;
        for i in (0..c.tabs.len()).filter(|&i| Some(i) != drag_index).chain(drag_index) {
            let slot = &c.tabs[i];
            let key = self.tabs[i].key;
            let Some(r) = self.tab_strip.rect(key, now) else { continue };
            let paint = self.tab_ghost(i);
            if r.w <= 0.0 || r.h <= 0.0 {
                self.tab_strip.remember(key, paint);
                continue;
            }
            g.dl.set_clip(Some(r));
            let active = i == self.active;
            let hovered = slot.rect.contains(mx, my);
            let dragging = drag_index == Some(i);
            let tab_alpha = self.hover_state.tabs.alpha(TabHover::Tab(key), now) + self.hover_state.tabs.alpha(TabHover::Close(key), now);
            if dragging {
                dragged_instances = Some(g.dl.instances.len());
                self.draw_active_tab(g, r, radius);
            } else if !active {
                if let Some(fill) = opacity(theme.tab_hover, tab_alpha) {
                    g.dl.round_rect(Rect::new(r.x + 2.0 * s, r.y + 2.0 * s, r.w - 4.0 * s, r.h - 6.0 * s), fill, (6.0 * s).round());
                }
                if !c.vertical && i + 1 != self.active && i + 1 < c.tabs.len() {
                    if let Some(fill) = opacity(theme.divider, 1.0 - tab_alpha) {
                        let lh = (16.0 * s).round();
                        g.dl.rect(Rect::new(r.right() - 1.0, r.y + ((r.h - lh) / 2.0).round(), (1.0 * s).max(1.0), lh), fill);
                    }
                }
            }
            self.draw_tab_mark(g, r, paint.color);
            let show_close = slot.close.w > 0.0 && (active || hovered);
            let close = Rect::new(slot.close.x + r.right() - slot.rect.right(), slot.close.y + r.y - slot.rect.y, slot.close.w, slot.close.h);
            let text_x = r.x + (12.0 * s).round();
            let text_end = if show_close { close.x - 4.0 * s } else { r.right() - 12.0 * s };
            let y = baseline(g, r, ui);
            let checking = matches!(self.tabs[i].command_watch, crate::close::CommandWatch::Polling { .. });
            match self.rename.as_ref().filter(|n| n.tab == i) {
                Some(edit) => {
                    let field = Rect::new(text_x - (5.0 * s).round(), r.y + (5.0 * s).round(), (text_end - text_x + 8.0 * s).max(0.0).round(), r.h - (10.0 * s).round());
                    g.dl.round_rect(field, theme.surface, (4.0 * s).round());
                    g.dl.outline(field, theme.accent, (4.0 * s).round(), (1.0 * s).round().max(1.0));
                    let w = g.text(text_x, y, &edit.text, ui, theme.text, (text_end - text_x).max(0.0));
                    let caret_h = (14.0 * s).round();
                    g.dl.rect(Rect::new(text_x + w + 1.0, field.y + ((field.h - caret_h) / 2.0).round(), (1.0 * s).round().max(1.0), caret_h), theme.accent);
                }
                None if paint.pinned && checking => draw_busy_spinner(&mut g.dl, &mut self.busy, r, s, theme.accent),
                None if paint.pinned => self.draw_pinned_tab(g, r, ui, &paint, active, self.tab_activity(i)),
                None => {
                    let mut x = text_x;
                    if checking && text_end - x >= 16.0 * s {
                        let area = Rect::new(x, r.y, 14.0 * s, r.h);
                        draw_busy_spinner(&mut g.dl, &mut self.busy, area, s, theme.accent);
                        x += (20.0 * s).round();
                    }
                    if self.tab_activity(i) {
                        let d = (6.0 * s).round();
                        g.dl.circle(Rect::new(x, r.y + ((r.h - d) / 2.0).round(), d, d), theme.accent);
                        x += d + (6.0 * s).round();
                    }
                    if let Some((label, tint)) = &paint.badge {
                        let style = tab_badge_style(s);
                        let w = (g.measure(label, style) + 6.0 * s).round();
                        if x + w + 18.0 * s <= text_end {
                            let badge = Rect::new(x, r.y + ((r.h - 16.0 * s) / 2.0).round(), w, (16.0 * s).round());
                            draw_tab_badge(g, badge, label, *tint, s);
                            x = badge.right() + (5.0 * s).round();
                        }
                    }
                    g.text(x, y, &paint.label, ui, if active { theme.text } else { theme.text_dim }, (text_end - x).max(0.0));
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
                let fraction = self.busy.progress(ProgressBar::Tab(self.tabs[i].key), fraction);
                g.dl.rect(track, theme.divider);
                g.dl.rect(Rect::new(track.x, track.y, (track.w * fraction).round(), track.h), ink);
            }
            let close_fill = hover_fill(&self.hover_state.tabs, TabHover::Close(key), now, theme.button_hover);
            if let Some(fill) = close_fill {
                g.dl.round_rect(close, fill, (4.0 * s).round());
            }
            if show_close || close_fill.is_some() {
                centered(g, close, G_CLOSE, FontStyle::new(FontFamily::Icons, (8.0 * s).round(), false), if active { theme.text } else { theme.text_dim });
            }
            self.tab_strip.remember(key, paint);
        }
        g.dl.confine(strip_start, bar);
        let tabs_end = g.dl.instances.len();
        g.dl.set_clip(Some(c.tab_bar));
        let (plus, menu) = self.tab_strip.buttons(now);
        if let Some(fill) = hover_fill(&self.hover_state.tabs, TabHover::Plus, now, theme.button_hover) {
            g.dl.round_rect(plus, fill, (6.0 * s).round());
        }
        centered(g, plus, G_PLUS, icons, theme.text_dim);
        if menu.w > 0.0 && menu.h > 0.0 {
            let fill = if self.drag == Drag::TabMenu && hot == Some(TabHover::Menu) { Some(theme.button_press) }
                else { hover_fill(&self.hover_state.tabs, TabHover::Menu, now, theme.button_hover) };
            if let Some(fill) = fill {
                g.dl.round_rect(menu, fill, (6.0 * s).round());
            }
            g.dl.chevron((menu.x + menu.w / 2.0, menu.y + menu.h / 2.0), 8.0 * s, 90.0, 1.5 * s, theme.text_dim);
        }
        if let Some(start) = dragged_instances {
            g.dl.instances[start..].rotate_left(tabs_end - start);
        }
        if c.settings.w > 0.0 && c.settings.h > 0.0 {
            let pressed = self.drag == Drag::SettingsButton && hot == Some(TabHover::Settings);
            let fill = if pressed { Some(theme.button_press) }
                else { hover_fill(&self.hover_state.tabs, TabHover::Settings, now, theme.button_hover) };
            if let Some(fill) = fill {
                g.dl.round_rect(c.settings, fill, (6.0 * s).round());
            }
            let gear = FontStyle::new(FontFamily::Icons, (16.0 * s).round(), false);
            centered(g, c.settings, G_SETTINGS, gear, theme.text);
        }
        g.dl.set_clip(Some(c.caption));
        for (r, glyph, area) in [(c.min, G_MIN, HitArea::Minimize), (c.max, if maximized { G_RESTORE } else { G_MAX }, HitArea::Maximize), (c.close, G_CLOSE, HitArea::Close)] {
            let pressed = self.drag == Drag::Button(area);
            let close = area == HitArea::Close;
            let hot_ink = if close { theme.close_hover } else { theme.button_hover };
            let fill = if pressed { Some(if close { theme.close_hover } else { theme.button_press }) }
                else { hover_fill(&self.hover_state.window, area, now, hot_ink) };
            if let Some(fill) = fill {
                g.dl.rect(r, fill);
            }
            let ink = if close && (window_hot == Some(area) || pressed) { Color::rgb(0xFFFFFF) } else { theme.text };
            centered(g, r, glyph, icons, ink);
        }
        g.dl.set_clip(None);
    }

    fn draw_active_tab<B: TextBackend>(&self, g: &mut Gfx<B>, r: Rect, radius: f32) {
        g.dl.set_clip(Some(r));
        let shape = match self.config.appearance.tab_bar_position {
            crate::config::TabBarPosition::Top => Rect::new(r.x, r.y, r.w, r.h + radius),
            crate::config::TabBarPosition::Bottom => Rect::new(r.x, r.y - radius, r.w, r.h + radius),
            _ => r.inset(0.0, (2.0 * self.ui()).round()),
        };
        g.dl.round_rect(shape, self.theme.tab_active, radius);
    }

    fn draw_tab_mark<B: TextBackend>(&self, g: &mut Gfx<B>, r: Rect, color: Option<Color>) {
        let s = self.ui();
        let inset = (10.0 * s).round();
        if let Some(ink) = color.filter(|_| r.w > 2.0 * inset) {
            g.dl.round_rect(Rect::new(r.x + inset, r.y + (3.0 * s).round(), r.w - 2.0 * inset, (3.0 * s).round().max(2.0)), ink, (1.5 * s).round());
        }
    }

    fn tab_ghost(&self, index: usize) -> TabGhost {
        let pinned = self.tabs[index].pinned;
        let badge = self.tab_badge(index);
        let mut label = self.tab_label(index);
        if pinned && badge.is_none() && label.is_empty() {
            label = self.tab_title(index);
        }
        TabGhost { label, color: self.tab_color(index), badge, pinned }
    }

    fn draw_pinned_tab<B: TextBackend>(&self, g: &mut Gfx<B>, r: Rect, ui: FontStyle, tab: &TabGhost, active: bool, activity: bool) {
        let s = self.ui();
        let number = &tab.label;
        let ink = if active { self.theme.text } else { self.theme.text_dim };
        let mut dot = (r.right() - 8.0 * s, r.y + 7.0 * s);
        if let Some((label, tint)) = &tab.badge {
            let number_w = g.measure(number, ui);
            let gap = if number.is_empty() { 0.0 } else { (3.0 * s).round() };
            let badge_w = (g.measure(label, tab_badge_style(s)) + 6.0 * s).round().min((r.w - number_w - gap).max(0.0));
            let x = (r.x + (r.w - number_w - gap - badge_w) / 2.0).round();
            let badge = Rect::new(x + number_w + gap, r.y + ((r.h - 16.0 * s) / 2.0).round(), badge_w, (16.0 * s).round());
            let y = baseline(g, r, ui);
            g.text(x, y, number, ui, ink, number_w + 1.0);
            draw_tab_badge(g, badge, label, *tint, s);
            dot = (badge.right(), badge.y);
        } else {
            let y = baseline(g, r, ui);
            let width = (r.w - 8.0 * s).max(0.0);
            let w = g.measure(number, ui).min(width);
            g.text((r.x + (r.w - w) / 2.0).round(), y, number, ui, ink, width);
        }
        if activity {
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
            pane.ssh_connecting = launch.ssh.is_some();
            if pane.profile.ssh.is_some() || pane.profile.exit_decision(None) == crate::profile::ExitDecision::Reconnect {
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

    fn draw_terminal_background<B: TextBackend>(&self, g: &mut Gfx<B>, rect: Rect) {
        let Some(bitmap) = &self.background.bitmap else { return };
        let appearance = &self.config.appearance;
        let alpha = (appearance.background_image_opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        if alpha > 0 {
            if let Some(placement) = crate::background::placement(rect, bitmap.source_size, appearance.background_image_fit) {
                g.draw_background_image(placement.rect, placement.tile, alpha);
            }
        }
    }

    fn draw_pane<B: TextBackend>(&mut self, g: &mut Gfx<B>, id: PaneId, (pane_rect, visual): (Rect, Rect), focused: bool, (m, sidebar_gap, show_from): (Metrics, f32, f32), (blink_alpha, blinking): (f32, bool)) {
        let s = self.ui();
        let rect = self.pane_terminal_rect(id, pane_rect);
        let inner = pane_inner_with(rect, s, self.config.terminal.padding);
        let size = Size::new((inner.w / m.w).floor().max(2.0) as usize, (inner.h / m.h).floor().max(1.0) as usize);
        self.prepare_pane(id, size, m);
        let window_focused = self.terminal_has_focus();
        let palette = self.pane_palette(id);
        let Some(pane) = self.panes.get_mut(&id) else { return };
        let theme = self.theme;
        let terminal = pane.terminal.clone();
        let mut t = terminal.lock();
        let searching = self.search.as_ref().is_some_and(|s| s.pane == id);
        let mut has_matches = false;
        let matches = if searching {
            let status = t.search_status().unwrap_or_default();
            has_matches = status.count != 0;
            if let Some(bar) = &mut self.search {
                bar.current = status.current;
                bar.count = status.count;
                bar.missing = !bar.error && !bar.query.is_empty() && !has_matches;
            }
            t.visible_matches()
        } else {
            Vec::new()
        };
        let overrides = *t.colors();
        let default_bg = palette.background(&overrides);
        if default_bg != self.palette.background {
            let from = g.dl.instances.len();
            g.dl.rect(Rect::new(pane_rect.x - sidebar_gap, pane_rect.y, pane_rect.w + sidebar_gap, pane_rect.h), rgb(default_bg));
            if visual != pane_rect {
                g.dl.refit(from, pane_rect, visual);
            }
        }
        let left = rect.x.max(show_from);
        g.dl.set_clip(Some(Rect::new(left, rect.y, (rect.right() - left).max(0.0), rect.h)));
        self.draw_terminal_background(g, rect);
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
                g.dl.outline(r, rgb(theme.search_current), 0.0, (1.0 * dpi).round().max(1.0));
            } else {
                g.dl.rect(r, rgb(theme.search_match).alpha(0x58));
            }
        }
        let cursor = t.cursor();
        let show_cursor = cursor.shape != CursorShape::Hidden;
        let cursor_color = overrides[aster_vt::NamedColor::Cursor].unwrap_or(palette.cursor);
        let cur_cols = if cursor.wide { 2 } else { 1 };
        let cr = cell_rect(cursor.line, cursor.col, cur_cols);
        let active_cursor = focused && window_focused;
        let cursor_alpha = if active_cursor { blink_alpha } else { 1.0 };
        let cursor_visible = show_cursor && (!active_cursor || self.preedit.is_empty());
        self.motion |= active_cursor && cursor_visible && blinking;
        let draw_cursor = cursor_visible && cursor_alpha > 0.0;
        let thin = (1.0 * dpi).round().max(1.0);
        let block = draw_cursor && active_cursor && matches!(cursor.shape, CursorShape::Block);
        if draw_cursor {
            let ink = rgb(cursor_color).alpha((255.0 * cursor_alpha).round() as u8);
            match (active_cursor, cursor.shape) {
                (true, CursorShape::Block) => g.dl.rect(cr, ink),
                (true, CursorShape::Beam) => g.dl.rect(Rect::new(cr.x, cr.y, (2.0 * dpi).round().max(1.0), cr.h), ink),
                (true, CursorShape::Underline) => g.dl.rect(Rect::new(cr.x, cr.bottom() - (2.0 * dpi).round().max(1.0), cr.w, (2.0 * dpi).round().max(1.0)), ink),
                _ => g.dl.outline(cr, ink, 0.0, thin),
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
                fg = crate::theme::mix(fg, cursor_text, cursor_alpha);
            }
            let x = ox + v.col as f32 * m.w;
            let y = oy + v.line as f32 * m.h;
            let c = v.cell.c;
            if (c != ' ' || v.cell.zerowidth().is_some()) && c != '\t' && !flags.contains(Flags::HIDDEN) {
                let mut backdrop = if v.selected { selection_bg } else { bg };
                if min_contrast > 1.0 && !matches.is_empty() {
                    let i = matches.partition_point(|mt| (mt.line, mt.end) <= (v.line, v.col));
                    if let Some(mt) = matches.get(i).filter(|mt| mt.line == v.line && mt.start <= v.col) {
                        backdrop = if mt.current { selection_bg } else { crate::theme::mix(backdrop, theme.search_match, 0x58 as f32 / 255.0) };
                    }
                }
                let backdrop = if at_cursor { crate::theme::mix(backdrop, cursor_color, cursor_alpha) } else { backdrop };
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
        if !has_matches && offset > 0 && history > 0 {
            let total = (history + size.rows) as f32;
            let track = rect.inset(0.0, (4.0 * s).round());
            let th = (track.h * size.rows as f32 / total).max(24.0 * s);
            let ty = track.y + (track.h - th) * (1.0 - offset as f32 / history as f32);
            let bw = (4.0 * s).round();
            g.dl.round_rect(Rect::new(track.right() - bw - 2.0 * s, ty, bw, th), theme.text_dim.alpha(0x90), bw / 2.0);
        }
        g.dl.set_clip(None);
        if focused && self.pane_rename.is_none() && !self.sidebar_focused() {
            self.set_ime_area(cr);
        }
    }

    fn draw_pane_header<B: TextBackend>(&mut self, g: &mut Gfx<B>, id: PaneId, rect: Rect, focused: bool, show_busy: bool, now: Instant) {
        let header = self.pane_header(id, rect);
        let s = self.ui();
        let theme = self.theme;
        let checking = self.pane_profile_probe.as_ref().is_some_and(|probe| probe.pane == id);
        let busy = show_busy && (self.pane_connecting(id) || checking);
        let toolbar = self.ssh_toolbar(id, rect);
        let open = toolbar.is_some();
        let animate = self.animations();
        let slides = self.panel_motion.headers.entry(id).or_default();
        let height = slides.title.update((header.h > 0.0).then_some((header.h, ())), animate, now, &mut self.motion);
        let toolbar_height = slides.toolbar.update(toolbar.map(|toolbar| (header.h, toolbar)), animate, now, &mut self.motion);
        let floating_busy = busy && (header.h <= 0.0 || header.w <= 0.0) && rect.w >= 28.0 * s && rect.h >= 24.0 * s;
        if floating_busy {
            let area = Rect::new(rect.right() - 25.0 * s, rect.y + 3.0 * s, 22.0 * s, 20.0 * s);
            g.dl.round_rect(area, theme.overlay, 4.0 * s);
            draw_busy_spinner(&mut g.dl, &mut self.busy, area, s, theme.accent);
        }
        let Some((full, ())) = slides.title.content else { return };
        if header.w <= 0.0 || height <= 0.0 { return; }
        let toolbar = slides.toolbar.content;
        let title_right = toolbar.filter(|_| open).map(|(_, toolbar)| toolbar.buttons[0].x);
        let header = Rect::new(header.x, header.y, header.w, full);
        let from = g.dl.instances.len();
        self.draw_pane_title(g, id, header, focused, title_right, busy && !floating_busy);
        let shown = Rect::new(header.x, header.y, header.w, height);
        if height < full {
            g.dl.shift(from, 0.0, height - full);
            g.dl.confine(from, shown);
        }
        if let Some((full, toolbar)) = toolbar.filter(|_| toolbar_height > 0.0) {
            let from = g.dl.instances.len();
            g.dl.set_clip(Some(header));
            self.draw_ssh_toolbar(g, id, &toolbar, open);
            g.dl.set_clip(None);
            if toolbar_height < full {
                g.dl.shift(from, 0.0, toolbar_height - full);
            }
            g.dl.confine(from, shown);
        }
    }

    fn draw_pane_title<B: TextBackend>(&mut self, g: &mut Gfx<B>, id: PaneId, header: Rect, focused: bool, title_right: Option<f32>, busy: bool) {
        let Some(pane) = self.panes.get(&id) else { return };
        let s = self.ui();
        let theme = self.theme;
        let ui = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let line = (1.0 * s).round().max(1.0);
        let right = title_right.unwrap_or_else(|| header.right());
        let field = Rect::new(header.x, header.y, right - header.x, header.h).inset((5.0 * s).round(), (2.0 * s).round());
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
            let mut x = x;
            if busy && right - x >= 15.0 * s {
                let area = Rect::new(x, header.y, 12.0 * s, header.h);
                draw_busy_spinner(&mut g.dl, &mut self.busy, area, s, theme.accent);
                x += (17.0 * s).round();
            }
            g.text(x, y, title, ui, if focused { theme.text } else { theme.text_dim }, (right - x - 8.0 * s).max(0.0));
        }
        g.dl.set_clip(None);
    }

    fn draw_search_ruler<B: TextBackend>(&self, g: &mut Gfx<B>, pane: PaneId) {
        let Some(track) = self.search_ruler_rect() else { return };
        let Some(state) = self.panes.get(&pane) else { return };
        let Some(overview) = state.terminal.lock().search_overview() else { return };
        if overview.rows.is_empty() { return; }
        let theme = self.theme;
        g.dl.rect(track, theme.overlay.alpha(0xA0));
        let viewport = crate::search::ruler_viewport(track, overview.viewport_start, overview.viewport_rows, overview.total_lines);
        g.dl.rect(viewport, theme.text_dim.alpha(0x72));
        for mark in crate::search::ruler_marks(track, &overview.rows, overview.current_row, overview.total_lines) {
            let color = if mark.current { rgb(theme.search_current) } else { rgb(theme.search_match).alpha(0xC0) };
            g.dl.rect(mark.rect, color);
        }
    }

    fn draw_sliding_search<B: TextBackend>(&mut self, g: &mut Gfx<B>, now: Instant) {
        let open = self.search_geometry().filter(|geo| geo.frame.h > 0.0);
        let snapshot = open.as_ref().zip(self.search.clone()).map(|(geo, bar)| (geo.frame.h, bar));
        let animate = self.animations();
        let height = self.panel_motion.search.update(snapshot, animate, now, &mut self.motion);
        let Some((full, bar)) = self.panel_motion.search.content.as_ref() else { return };
        let Some((_, pane)) = self.layout.panes.iter().find(|(id, _)| *id == bar.pane) else { return };
        let geo = crate::search::geometry(*pane, self.ui());
        let from = self.draw_search(g, &geo, bar, open.is_some());
        if height <= 0.0 {
            g.dl.instances.truncate(from);
        } else if height < *full {
            g.dl.shift(from, 0.0, height - full);
            g.dl.confine(from, Rect::new(pane.x, geo.frame.y, pane.w, height + (14.0 * self.ui()).round()));
        }
    }

    fn draw_search<B: TextBackend>(&self, g: &mut Gfx<B>, geo: &crate::search::SearchGeometry, bar: &crate::search::SearchBar, open: bool) -> usize {
        use crate::search::SearchHit;
        if open {
            self.draw_search_ruler(g, bar.pane);
        }
        let from = g.dl.instances.len();
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
            } else if open && r.contains(mx, my) {
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
        from
    }

    fn draw_popup_hint<B: TextBackend>(&mut self, g: &mut Gfx<B>, pane: Rect, layer: Layer, text: Option<&str>) {
        let from = self.layers.start(&mut g.dl, layer, self.layers.opened(layer, text.is_some()), POP);
        if let Some(text) = text {
            let frame = self.draw_hint(g, pane, text, false);
            self.layers.pop(&mut g.dl, layer, from, frame, 0, POP);
        }
    }

    fn draw_hint<B: TextBackend>(&self, g: &mut Gfx<B>, pane: Rect, text: &str, busy: bool) -> Rect {
        let s = self.ui();
        let theme = self.theme;
        let h = hint_height(s);
        let r = Rect::new(pane.x, pane.bottom() - h, pane.w, h);
        g.dl.rect(r, theme.overlay);
        g.dl.rect(Rect::new(r.x, r.y, r.w, (1.0 * s).round().max(1.0)), theme.accent);
        let ui = FontStyle::new(FontFamily::Ui, (12.5 * s).round(), false);
        let mut inner = r.inset((14.0 * s).round(), 0.0);
        if busy {
            let pad = (20.0 * s).round();
            inner.x += pad;
            inner.w = (inner.w - pad).max(0.0);
        }
        let y = baseline(g, inner, ui);
        g.text(inner.x, y, text, ui, theme.text, inner.w);
        r
    }

    fn draw_offer<B: TextBackend>(&self, g: &mut Gfx<B>, pane: Rect, state: crate::app::PaneTransfer, typed: &str, busy: bool) -> Rect {
        use crate::app::PaneTransfer;
        match state {
            PaneTransfer::Incoming => return self.draw_hint(g, pane, t("The remote side wants to send files. Enter receives them into Downloads, Esc refuses."), false),
            PaneTransfer::Running => return self.draw_hint(g, pane, t("Transfer in progress. Esc cancels."), busy),
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
        r
    }

    fn draw_paste<B: TextBackend>(&mut self, g: &mut Gfx<B>, m: Metrics) {
        use crate::paste::{BUTTONS, PREVIEW_LINES};
        let layer = Layer::Paste;
        let from = self.layers.start(&mut g.dl, layer, self.paste_confirm.as_ref().map(|confirm| confirm.opened_at), POP);
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
        g.text(geo.title.x, ty, &tn("Paste {n} line?|Paste {n} lines?", lines.len() as u64), title, theme.text, geo.title.w);
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
            centered(g, *r, t(BUTTONS[i]), ui, if chosen { theme.on_accent } else { theme.text });
        }
        self.layers.pop(&mut g.dl, layer, from, geo.frame, 1, POP);
    }

    fn draw_ssh_toolbar<B: TextBackend>(&self, g: &mut Gfx<B>, pane: PaneId, toolbar: &crate::ssh_toolbar::Toolbar, open: bool) {
        let s = self.ui();
        let theme = self.theme;
        let ui = FontStyle::new(FontFamily::Ui, (12.0 * s).round(), false);
        let line = s.round().max(1.0);
        let (mx, my) = self.mouse;
        let names = [if toolbar.buttons[0].w >= 72.0 * s { t("Reconnect") } else { "↻" }, t("SFTP"), t("Ports")];
        for ((button, label), enabled) in toolbar.buttons.iter().zip(names).zip(self.ssh_toolbar_buttons(pane)) {
            let active = open && enabled && button.contains(mx, my);
            g.dl.round_rect(*button, if active { theme.button_hover } else { theme.surface }, 4.0 * s);
            g.dl.outline(*button, if active { theme.accent } else { theme.overlay_border }, 4.0 * s, line);
            centered(g, *button, label, ui, if enabled { theme.text } else { theme.text_dim });
        }
    }

    fn draw_ssh_forward_form<B: TextBackend>(&self, g: &mut Gfx<B>, area: Rect, form: &crate::ssh_forward_form::ForwardForm) -> Option<Rect> {
        use crate::ssh_forward_form::{ADD, CANCEL, SAVE};
        let geo = form.geometry(area, self.ui());
        let s = geo.scale;
        let theme = self.theme;
        let ui = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let heading = FontStyle::new(FontFamily::Ui, (16.0 * s).round(), true);
        let line = s.round().max(1.0);
        let (mx, my) = self.mouse;
        g.dl.rect(area, theme.dim);
        g.dl.round_rect(geo.frame, theme.overlay, 8.0 * s);
        g.dl.outline(geo.frame, theme.overlay_border, 8.0 * s, line);
        let y = baseline(g, geo.title, heading);
        g.text(geo.title.x, y, t("Add port forward"), heading, theme.text, geo.title.w);

        let button = |g: &mut Gfx<B>, rect: Rect, label: &str, selected: bool, focused: bool| {
            let color = if rect.contains(mx, my) { theme.button_hover } else if selected { theme.accent.alpha(0x38) } else { theme.surface };
            g.dl.round_rect(rect, color, 5.0 * s);
            g.dl.outline(rect, if focused || selected { theme.accent } else { theme.overlay_border }, 5.0 * s, line);
            centered(g, rect, label, ui, theme.text);
        };
        let labels = [t("Local"), t("Remote"), t("Dynamic")];
        for ((rect, label), kind) in geo.kinds.into_iter().zip(labels).zip(crate::ssh_forward_form::KINDS) {
            button(g, rect, label, form.kind == kind, form.kind_focused() && form.kind == kind);
        }

        let labels = [t("Listening address"), t("Port"), t("Destination host"), t("Destination port")];
        let mut ime = None;
        for (i, (slot, label)) in geo.fields.into_iter().zip(labels).take(form.field_count()).enumerate() {
            let field = &form.fields[i];
            let focused = i == form.focus;
            let label_r = Rect::new(slot.x, slot.y, slot.w, 19.0 * s);
            let y = baseline(g, label_r, small);
            g.text(label_r.x, y, label, small, theme.text_dim, label_r.w);
            let r = Rect::new(slot.x, label_r.bottom() + 2.0 * s, slot.w, 31.0 * s).snap();
            g.dl.round_rect(r, theme.surface, 5.0 * s);
            g.dl.outline(r, if focused { theme.accent } else { theme.overlay_border }, 5.0 * s, line);
            let inner = r.inset(9.0 * s, 2.0 * s);
            let y = baseline(g, inner, ui);
            let text = field.text();
            if focused {
                let offset = text.char_indices().nth(field.cursor_position().1).map_or(text.len(), |(i, _)| i);
                let head = g.measure(&text[..offset], ui);
                let x = inner.x.min(inner.right() - head - 2.0 * line);
                let first = g.dl.instances.len();
                let selection = field.selection();
                if !selection.is_empty() {
                    let a = g.measure(&text[..selection.start], ui);
                    let b = g.measure(&text[..selection.end], ui);
                    g.dl.rect(Rect::new(x + a, inner.y, b - a, inner.h), theme.accent.alpha(0x52));
                }
                let width = g.measure(text, ui);
                g.text(x, y, text, ui, theme.text, width + 1.0);
                let caret = Rect::new(x + head + 1.0, inner.y + 4.0 * s, line, (inner.h - 8.0 * s).max(1.0)).snap();
                g.dl.rect(caret, theme.accent);
                g.dl.confine(first, inner);
                ime = Some(caret);
            } else {
                g.text(inner.x, y, text, ui, theme.text, inner.w);
            }
        }

        let can_save = self.can_save_forward(form.pane);
        let box_r = Rect::new(geo.save.x, geo.save.y + (geo.save.h - 15.0 * s) * 0.5, 15.0 * s, 15.0 * s).snap();
        let checked = can_save && form.save;
        g.dl.round_rect(box_r, if checked { theme.accent } else { theme.surface }, 3.0 * s);
        g.dl.outline(box_r, if checked || can_save && (form.focus == SAVE || geo.save.contains(mx, my)) { theme.accent } else { theme.overlay_border }, 3.0 * s, line);
        if checked {
            centered(g, box_r, "✓", small, theme.on_accent);
        }
        let y = baseline(g, geo.save, ui);
        g.text(box_r.right() + 8.0 * s, y, t("Save to profile"), ui, if can_save { theme.text } else { theme.text_dim }, geo.save.w - 23.0 * s);

        if let Some(message) = form.error.or_else(|| (!can_save).then(|| t("Available for editable SSH profiles"))) {
            let y = baseline(g, geo.status, small);
            let color = if form.error.is_some() { Color::rgb(if theme.dark { 0xFF9490 } else { 0xA52320 }) } else { theme.text_dim };
            g.text(geo.status.x, y, message, small, color, geo.status.w);
        }
        button(g, geo.cancel, t("Cancel"), false, form.focus == CANCEL);
        button(g, geo.add, t("Add"), true, form.focus == ADD);
        ime
    }

    fn draw_ssh_auth<B: TextBackend>(&self, g: &mut Gfx<B>, pane: Rect, panel: &crate::ssh_auth::Panel, geo: crate::ssh_auth::Geometry, focused: bool, phase: Option<f32>) -> Rect {
        use crate::ssh_auth::Row;
        let s = self.ui();
        let theme = self.theme;
        let ui = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), false);
        let heading = FontStyle::new(FontFamily::Ui, (15.0 * s).round(), true);
        let line = s.round().max(1.0);
        let radius = 7.0 * s;
        let (mx, my) = self.mouse;
        let first = g.dl.instances.len();
        g.dl.rect(pane, theme.dim);
        g.dl.round_rect(geo.frame, theme.overlay, radius);
        g.dl.outline(geo.frame, theme.overlay_border, radius, line);
        let y = baseline(g, geo.title, heading);
        g.text(geo.title.x, y, t("SSH authentication"), heading, theme.text, geo.title.w);
        for &(index, row) in &geo.entries {
            let y = baseline(g, row, ui);
            if panel.pending {
                let mut x = row.x;
                if let Some(phase) = phase {
                    spinner(&mut g.dl, Rect::new(x, row.y, 14.0 * s, row.h), phase, s, theme.accent);
                    x += (20.0 * s).round();
                }
                g.text(x, y, t("Waiting for the SSH server…"), ui, theme.text_dim, (row.right() - x).max(0.0));
                continue;
            }
            match &panel.rows[index] {
                Row::Text(text) => { g.text(row.x, y, text, ui, theme.text_dim, row.w); }
                Row::Link(url) => {
                    if row.contains(mx, my) { g.dl.round_rect(row, theme.button_hover, radius); }
                    let w = g.measure(url, ui).min(row.w);
                    g.text(row.x, y, url, ui, theme.accent, row.w);
                    g.dl.rect(Rect::new(row.x, y + 2.0 * s, w, line), theme.accent);
                }
                Row::Prompt(index) => {
                    let caption = row.w * 0.43;
                    let label: String = panel.challenge.prompts[*index].label.chars().filter(|c| !c.is_control()).collect();
                    g.text(row.x, y, &label, ui, theme.text, caption - 6.0 * s);
                    let field = Rect::new(row.x + caption, row.y + 2.0 * s, row.w - caption, row.h - 4.0 * s);
                    g.dl.round_rect(field, theme.surface, 4.0 * s);
                    let active = focused && panel.focus == *index;
                    g.dl.outline(field, if active { theme.accent } else { theme.overlay_border }, 4.0 * s, line);
                    let value = panel.display(*index);
                    let inner = field.inset(7.0 * s, 1.0 * s);
                    let from = g.dl.instances.len();
                    let width = g.measure(&value, ui);
                    let x = inner.x.min(inner.right() - width - 3.0 * s);
                    g.text(x, y, &value, ui, theme.text, width + 2.0 * s);
                    if active { g.dl.rect(Rect::new(x + width + 2.0 * s, inner.y + 4.0 * s, line, inner.h - 8.0 * s), theme.accent); }
                    g.dl.confine(from, inner);
                }
            }
        }
        if let Some(row) = geo.remember {
            let check = Rect::new(row.x, row.y + 3.0 * s, 17.0 * s, 17.0 * s);
            g.dl.round_rect(check, if panel.remember { theme.accent.alpha(0x48) } else { theme.surface }, 3.0 * s);
            let active = focused && panel.focus == panel.challenge.prompts.len();
            g.dl.outline(check, if active || panel.remember { theme.accent } else { theme.overlay_border }, 3.0 * s, line);
            if panel.remember { centered(g, check, "✓", ui, theme.text); }
            let y = baseline(g, row, ui);
            g.text(check.right() + 8.0 * s, y, t("Remember password"), ui, theme.text, row.w - 25.0 * s);
        }
        for (button, label, primary) in [(geo.submit, t("Continue"), true), (geo.cancel, t("Cancel"), false)] {
            if primary && panel.pending { continue; }
            g.dl.round_rect(button, if button.contains(mx, my) { theme.button_hover } else if primary { theme.accent.alpha(0x42) } else { theme.surface }, 5.0 * s);
            g.dl.outline(button, if primary { theme.accent } else { theme.overlay_border }, 5.0 * s, line);
            centered(g, button, label, ui, theme.text);
        }
        g.dl.confine(first, pane);
        geo.frame
    }

    fn draw_close<B: TextBackend>(&mut self, g: &mut Gfx<B>) {
        use crate::close::{CloseTarget, BUTTONS, LISTED};
        let layer = Layer::Close;
        let from = self.layers.start(&mut g.dl, layer, self.close_confirm.as_ref().map(|confirm| confirm.opened_at), POP);
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
            _ if confirm.ssh => t("Close with active SSH connections?"),
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
            centered(g, *r, label, ui, if chosen { theme.on_accent } else { theme.text });
        }
        self.layers.pop(&mut g.dl, layer, from, geo.frame, 1, POP);
    }

    fn draw_tab_toast<B: TextBackend>(&mut self, g: &mut Gfx<B>, now: std::time::Instant) {
        let layer = Layer::Toast;
        let toast = self.tab_toast.as_ref();
        let from = self.layers.start(&mut g.dl, layer, toast.map(|toast| toast.opened_at), TOAST_OUT);
        let Some(toast) = toast else { return };
        let Some((opening, alpha, moving)) = toast.appearance(now, self.animations()) else {
            self.layers.forget_snapshot(layer);
            return;
        };
        self.motion |= moving;
        let text = &toast.text;
        let s = self.ui();
        let theme = self.theme;
        let ui = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), false);
        let content = self.chrome.content;
        let spinner_space = if toast.checking { (20.0 * s).round() } else { 0.0 };
        let w = (g.measure(text, ui) + 28.0 * s + spinner_space).min((content.w - 24.0 * s).max(0.0)).round();
        let h = (32.0 * s).round();
        let r = Rect::new((content.x + (content.w - w) / 2.0).round(), content.y + (14.0 * s).round(), w, h);
        g.dl.shadow(Rect::new(r.x, r.y + 3.0 * s, r.w, r.h), theme.shadow, (8.0 * s).round(), (12.0 * s).round());
        g.dl.round_rect(r, theme.overlay, (8.0 * s).round());
        g.dl.outline(r, theme.overlay_border, (8.0 * s).round(), (1.0 * s).round().max(1.0));
        let mut label = r;
        if toast.checking && r.w > 44.0 * s {
            let area = Rect::new(r.x + (12.0 * s).round(), r.y, 14.0 * s, r.h);
            let phase = if now < toast.fade_at { self.busy.phase() } else { crate::motion::spin_phase(toast.fade_at, self.animations()) };
            spinner(&mut g.dl, area, phase, s, theme.accent);
            label.x += spinner_space;
            label.w -= spinner_space;
        }
        centered(g, label, text, ui, theme.text);
        g.dl.pop_in(from, opening, (r.x + r.w / 2.0, r.y + r.h / 2.0));
        g.dl.fade(from, alpha);
        self.layers.save(&g.dl, layer, from);
    }

    fn draw_update_notice<B: TextBackend>(&mut self, g: &mut Gfx<B>) {
        let layer = Layer::UpdateNotice;
        let from = self.layers.start(&mut g.dl, layer, self.update_notice.as_ref().map(|notice| notice.opened_at), TOAST_OUT);
        let s = self.ui();
        let title = FontStyle::new(FontFamily::Ui, (14.0 * s).round(), true);
        let ui = FontStyle::new(FontFamily::Ui, (12.5 * s).round(), false);
        let content = self.chrome.content;
        let Some(notice) = self.update_notice.as_mut() else { return };
        notice.button_widths = notice.buttons().iter().map(|b| (g.measure(b.label(notice.notes_open), ui) + 28.0 * s).ceil()).collect();
        let inner = (crate::update::notice_geometry(content, s, notice).frame.w - 28.0 * s).max(1.0);
        let spinner_space = if notice.busy() { (20.0 * s).round() } else { 0.0 };
        notice.status_lines = wrapped_info(g, &notice.status(), ui, (inner - spinner_space).max(1.0));
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
        if notice.busy() {
            let area = Rect::new(geo.status.x, geo.status.y, 14.0 * s, geo.line_h);
            draw_busy_spinner(&mut g.dl, &mut self.busy, area, s, theme.accent);
        }
        for (k, text) in notice.status_lines.iter().take(((geo.status.h / geo.line_h) as usize).max(1)).enumerate() {
            let r = Rect::new(geo.status.x + spinner_space, geo.status.y + k as f32 * geo.line_h, (geo.status.w - spinner_space).max(0.0), geo.line_h);
            let y = baseline(g, r, ui);
            g.text(r.x, y, text, ui, status_ink, r.w);
        }
        if let (Some(bar), Some(fraction)) = (geo.bar, notice.fraction()) {
            let fraction = self.busy.progress(ProgressBar::Update(notice.opened_at), fraction);
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
            centered(g, *r, button.label(notice.notes_open), ui, if primary { theme.on_accent } else { theme.text });
        }
        self.layers.pop(&mut g.dl, layer, from, geo.frame, 0, TOAST_IN);
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

    fn draw_menu<B: TextBackend>(&mut self, g: &mut Gfx<B>, now: std::time::Instant) {
        use crate::menu::{item_progress, Entry};
        let layer = Layer::Menu;
        let menu = self.menu.clone();
        let from = self.layers.start(&mut g.dl, layer, menu.as_ref().map(|menu| menu.opened_at), POP);
        let Some(menu) = menu else { return };
        let Some(geo) = self.menu_geometry() else { return };
        let phase = self.layers.progress(layer, UNFOLD);
        let unfolded = geo.unfolded(phase);
        let animate = self.animations();
        self.motion |= self.hover_state.menu.aim(menu.selected, animate, now);
        let s = self.ui();
        let theme = self.theme;
        let radius = (7.0 * s).round();
        g.dl.shadow(Rect::new(geo.frame.x, geo.frame.y + 4.0 * s, geo.frame.w, geo.frame.h), theme.shadow, radius, (14.0 * s).round());
        g.dl.round_rect(geo.frame, theme.overlay, radius);
        g.dl.outline(geo.frame, theme.overlay_border, radius, (1.0 * s).round().max(1.0));
        if phase < 1.0 {
            g.dl.refit(from, geo.frame, unfolded);
        }
        let separators = g.dl.instances.len();
        for r in &geo.separators {
            g.dl.rect(*r, theme.overlay_border);
        }
        if phase < 1.0 {
            g.dl.confine(separators, unfolded);
        }
        let ui = FontStyle::new(FontFamily::Ui, (12.5 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (11.0 * s).round(), false);
        for (k, (i, r)) in geo.rows.iter().enumerate() {
            let Some(Entry::Item { label, action, enabled }) = menu.entries.get(*i) else { continue };
            let row = g.dl.instances.len();
            if let Some(fill) = hover_fill(&self.hover_state.menu, *i, now, theme.accent.alpha(0x38)) {
                g.dl.round_rect(*r, fill, (4.0 * s).round());
            }
            let ink = if *enabled { theme.text } else { theme.text_dim.alpha(0x80) };
            let hint = if menu.profile.is_some() { String::new() } else { self.keymap.bindings_of(action).next().map(crate::keymap::format_binding).unwrap_or_default() };
            let hint_w = if hint.is_empty() { 0.0 } else { g.measure(&hint, small) };
            let pad = (12.0 * s).round();
            let y = baseline(g, *r, ui);
            let label = match label {
                std::borrow::Cow::Borrowed(label) => t(label),
                std::borrow::Cow::Owned(label) => label,
            };
            g.text(r.x + pad, y, label, ui, ink, (r.w - hint_w - 3.0 * pad).max(0.0));
            if hint_w > 0.0 {
                let hy = baseline(g, *r, small);
                g.text(r.right() - pad - hint_w, hy, &hint, small, theme.text_dim, hint_w + 1.0);
            }
            if phase < 1.0 {
                let t = item_progress(menu.opened_at, now, k);
                let offset = if geo.opens_up { 4.0 * s } else { -4.0 * s };
                g.dl.nudge(row, 0.0, offset * (1.0 - t));
                g.dl.fade(row, t);
                g.dl.confine(row, unfolded);
            }
        }
        self.layers.save(&g.dl, layer, from);
    }

    fn draw_sliding_files<B: TextBackend>(&mut self, g: &mut Gfx<B>, now: Instant) {
        if self.files.as_ref().is_some_and(|f| !self.panes.contains_key(&f.pane)) {
            self.close_files();
        }
        let content = self.chrome.content;
        let open = self.files.is_some();
        let animate = self.animations();
        let width = self.panel_motion.files.update(open.then_some((content.w, content)), animate, now, &mut self.motion);
        let Some((full, rect)) = self.panel_motion.files.content else {
            self.panel_motion.closed_files = None;
            self.hover_state.files = Default::default();
            return;
        };
        if width <= 0.0 { return; }
        let Some(page) = self.files.as_ref().or(self.panel_motion.closed_files.as_ref()) else { return };
        let geo = crate::files::geometry(rect, self.ui(), page, &self.transfers_of(page.pane));
        let max_scroll = (geo.height - geo.list.h).max(0.0);
        let scroll = self.list_motion.files.sample(geo.scroll, max_scroll, animate, now, &mut self.motion);
        let (mx, my) = self.mouse;
        let hot = if open && geo.list.contains(mx, my) {
            geo.rows.iter().find(|(_, r)| r.contains(mx, my)).map(|(row, _)| *row)
        } else {
            None
        };
        self.motion |= self.hover_state.files.aim(hot, animate, now);
        if let Some(page) = self.files.as_mut() { page.scroll = geo.scroll; }
        let Some(page) = self.files.as_ref().or(self.panel_motion.closed_files.as_ref()) else { return };
        let pane = page.pane;
        let from = g.dl.instances.len();
        self.draw_files(g, rect, page, &geo, (open, scroll, now));
        self.draw_file_transfers(g, pane, &geo, open);
        if width < full {
            g.dl.shift(from, full - width, 0.0);
            g.dl.confine(from, Rect::new(rect.right() - width, rect.y, width, rect.h));
        }
    }

    fn draw_files<B: TextBackend>(&self, g: &mut Gfx<B>, content: Rect, page: &crate::files::FilesPage, geo: &crate::files::FilesGeometry, (open, scroll, now): (bool, f32, Instant)) {
        use crate::files::{date, human_size, Prompt};
        let s = self.ui();
        let theme = self.theme;
        let line = (1.0 * s).round().max(1.0);
        let radius = (6.0 * s).round();
        let (mx, my) = if open { self.mouse } else { (-1.0, -1.0) };
        g.dl.rect(content, theme.surface);
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
        if matches!(page.prompt, Some(Prompt::Delete { .. })) {
            g.dl.outline(geo.path, Color::rgb(0xD9534F), radius, line);
        }
        field(g, geo.filter, &page.query, t("Filter"), page.prompt.is_none());

        g.dl.set_clip(Some(geo.list));
        let name_x = geo.list.x + (40.0 * s).round();
        let date_w = (100.0 * s).round();
        let size_w = (90.0 * s).round();
        for (row, hit) in &geo.rows {
            let r = Rect::new(hit.x, hit.y + geo.scroll.round() - scroll.round(), hit.w, hit.h);
            if r.bottom() <= geo.list.y || r.y >= geo.list.bottom() {
                continue;
            }
            if *row == page.selected {
                g.dl.round_rect(r, theme.accent.alpha(0x30), radius);
            } else if let Some(fill) = hover_fill(&self.hover_state.files, *row, now, theme.button_hover) {
                g.dl.round_rect(r, fill, radius);
            }
            let y = baseline(g, r, ui);
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
                let sy = baseline(g, r, small);
                g.text(right - date_w - w - 12.0 * s, sy, &size, small, theme.text_dim, size_w);
            }
            if let Some(mode) = entry.mode {
                let p = crate::files::permissions(mode);
                let py = baseline(g, r, small);
                let pw = g.measure(&p, small);
                g.text(right - date_w - size_w - pw - 24.0 * s, py, &p, small, theme.text_dim, pw + 1.0);
            }
            if let Some(m) = entry.modified {
                let d = date(m);
                let w = g.measure(&d, small);
                let dy = baseline(g, r, small);
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
    }

    fn draw_file_transfers<B: TextBackend>(&mut self, g: &mut Gfx<B>, pane: PaneId, geo: &crate::files::FilesGeometry, open: bool) {
        use crate::files::{human_size, TransferState};
        let s = self.ui();
        let theme = self.theme;
        let radius = (6.0 * s).round();
        let ui = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let (mx, my) = self.mouse;
        for (id, r) in &geo.transfers {
            let Some(t) = self.transfers.iter().rev().find(|t| t.pane == pane && t.id == *id) else { continue };
            if open && r.contains(mx, my) {
                g.dl.round_rect(*r, theme.button_hover, radius);
            }
            let pad = (12.0 * s).round();
            let bar = Rect::new(r.x + pad, r.bottom() - (6.0 * s).round(), r.w - 2.0 * pad, (3.0 * s).round().max(2.0));
            let target = if t.total > 0 { (t.done as f32 / t.total as f32).clamp(0.0, 1.0) } else { 0.0 };
            let fraction = self.busy.progress(ProgressBar::Transfer(*id), target);
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
            let mut x = r.x + pad + lw + (14.0 * s).round();
            let mut width = r.w - lw - 3.0 * pad;
            if matches!(t.state, TransferState::Running) && width >= 24.0 * s {
                let area = Rect::new(x, r.y, 14.0 * s, (bar.y - r.y).max(0.0));
                draw_busy_spinner(&mut g.dl, &mut self.busy, area, s, theme.accent);
                let reserve = (20.0 * s).round();
                x += reserve;
                width -= reserve;
            }
            g.text(x, y, &state, small, color, width.max(0.0));
        }
        let hint = t("Enter open  ·  Ctrl+D download  ·  F4 edit  ·  F2 rename  ·  F7 new folder  ·  F9 permissions  ·  Del delete  ·  Ctrl+U or drop to upload");
        let hy = baseline(g, geo.hint, small);
        g.text(geo.hint.x, hy, hint, small, theme.text_dim, geo.hint.w);
    }

    fn draw_profile_tree_row<B: TextBackend>(&self, g: &mut Gfx<B>, page: &crate::settings::SettingsPage, tree: &crate::profiles_page::TreeRow, rect: Rect, (selected, (mx, my)): (bool, (f32, f32)), controls: &mut crate::control_motion::Frame<'_>) {
        use crate::profiles_page::{kind_label, layout::row_action, TreeKind};
        let theme = self.theme;
        let s = self.ui();
        let ui = FontStyle::new(FontFamily::Ui, (13.5 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let bold = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), true);
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
                let angle = controls.angle(crate::control_motion::Control::Group(false, header.clone()), open);
                g.dl.chevron((x + 4.0 * s, rect.y + rect.h * 0.5), 8.0 * s, angle, 1.5 * s, theme.text_dim);
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
                let label = if matches!(tree.kind, TreeKind::NewProfile) { t("Create") } else { t("New group") };
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
            inner = color_field(g, inner, field.1, theme, s);
        }
        if let ProfileFieldControl::Switch(on) = control {
            let h = (18.0 * s).round();
            let switch = Rect::new(inner.right() - 32.0 * s, r.y + (r.h - h) * 0.5, 32.0 * s, h).snap();
            g.dl.round_rect(switch, if on { theme.accent } else { theme.overlay_border }, h * 0.5);
            let knob = switch.h - 4.0 * s;
            let kx = if on { switch.right() - knob - 2.0 * s } else { switch.x + 2.0 * s };
            g.dl.circle(Rect::new(kx, switch.y + (switch.h - knob) * 0.5, knob, knob), if on { theme.on_accent } else { theme.text_dim });
            inner.w = (switch.x - 8.0 * s - inner.x).max(0.0);
        } else if matches!(control, ProfileFieldControl::Picker) {
            g.dl.chevron((inner.right() - 5.0 * s, r.y + r.h * 0.5), 8.0 * s, 0.0, (1.4 * s).max(1.0), theme.text_dim);
            inner.w = (inner.w - 18.0 * s).max(0.0);
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

    fn draw_profile_editor<B: TextBackend>(&self, g: &mut Gfx<B>, area: Rect, page: &crate::settings::SettingsPage, controls: &mut crate::control_motion::Frame<'_>) {
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
            (crate::profiles_page::layout::profile_editor_layout(area, s, editor), editor.title(), editor.error.as_deref())
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
            let labels = editor.tabs.iter().map(|tab| tab.label.as_str());
            self.draw_page_tabs(g, controls, crate::control_motion::Tabs::ProfileEditor, &layout.tabs, (editor.tab, labels), ui);
            for &(i, slot) in &layout.fields {
                let field = &editor.tabs[editor.tab].fields[i];
                if let Some(checked) = field.checked() {
                    let r = Rect::new(slot.x, slot.y + 13.0 * s, 19.0 * s, 19.0 * s).snap();
                    g.dl.round_rect(r, if checked { theme.accent.alpha(0x48) } else { theme.surface }, 4.0 * s);
                    g.dl.outline(r, if i == editor.focus || checked { theme.accent } else { theme.overlay_border }, 4.0 * s, line);
                    let progress = controls.check(editor.tab, i, checked);
                    if progress > 0.0 {
                        let from = g.dl.instances.len();
                        centered(g, r, "✓", ui, theme.text);
                        g.dl.grow(from, progress, (r.x + r.w * 0.5, r.y + r.h * 0.5), 0.6);
                    }
                    let label = Rect::new(r.right() + 9.0 * s, r.y, (slot.right() - r.right() - 9.0 * s).max(0.0), r.h);
                    let y = baseline(g, label, ui);
                    g.text(label.x, y, &field.label, ui, theme.text, label.w);
                    continue;
                }
                if field.is_button() {
                    let r = Rect::new(slot.x, slot.y + 21.0 * s, slot.w, 31.0 * s).snap();
                    g.dl.round_rect(r, if slot.contains(mx, my) { theme.button_hover } else { theme.surface }, 5.0 * s);
                    g.dl.outline(r, if i == editor.focus { theme.accent } else { theme.overlay_border }, 5.0 * s, line);
                    centered(g, r, &field.label, ui, theme.text);
                    continue;
                }
                let control = match field.switch() {
                    Some(value) => ProfileFieldControl::Switch(value),
                    None if field.is_icon() => ProfileFieldControl::Picker,
                    None if !field.choices.is_empty() => ProfileFieldControl::Choice { editable: field.editable },
                    None if !field.editable => ProfileFieldControl::ReadOnly,
                    None => ProfileFieldControl::Text,
                };
                let masked = field.masked();
                self.draw_editor_field(g, slot, (&field.label, masked.as_deref().unwrap_or_else(|| field.display())), control, i == editor.focus);
            }
            if let Some(preview) = layout.scheme_preview {
                let palette = editor.preview_scheme(&self.config).and_then(|name| self.scheme(&name)).unwrap_or(self.palette);
                let rect = Rect::new(preview.x, preview.y + 5.0 * s, preview.w.min(360.0 * s), 36.0 * s).snap();
                self.draw_scheme_swatches(g, rect, small, (color(palette.background), color(palette.foreground), palette.ansi.map(color)));
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

    fn draw_window_vault<B: TextBackend>(&mut self, g: &mut Gfx<B>, area: Rect) {
        let layer = Layer::Vault;
        let dialog = self.vault_prompt.as_ref();
        let from = self.layers.start(&mut g.dl, layer, dialog.map(|dialog| dialog.opened_at), POP);
        if let Some(dialog) = dialog {
            let (rect, sensitive) = self.draw_vault_dialog(g, area, dialog, false);
            self.layers.pop(&mut g.dl, layer, from, rect, 1, POP);
            if sensitive {
                self.layers.forget_snapshot(layer);
            }
        }
    }

    fn draw_vault_dialog<B: TextBackend>(&self, g: &mut Gfx<B>, area: Rect, dialog: &crate::vault_settings::VaultDialog, unlocked: bool) -> (Rect, bool) {
        let layout = crate::vault_settings::vault_layout(area, self.ui(), dialog, unlocked);
        let frame = layout.frame;
        if dialog.revealed || dialog.value.is_some() {
            g.with_transient_text(|g| self.draw_vault_dialog_content(g, area, dialog, unlocked, layout));
            (frame, true)
        } else {
            self.draw_vault_dialog_content(g, area, dialog, unlocked, layout);
            (frame, false)
        }
    }

    fn draw_vault_dialog_content<B: TextBackend>(&self, g: &mut Gfx<B>, area: Rect, dialog: &crate::vault_settings::VaultDialog, unlocked: bool, layout: crate::vault_settings::VaultLayout) {
        let s = self.ui();
        let theme = self.theme;
        let (mx, my) = self.mouse;
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
    }

    fn draw_scheme_editor<B: TextBackend>(&self, g: &mut Gfx<B>, area: Rect, editor: &crate::scheme_editor::SchemeEditor) {
        use crate::scheme_editor::{geometry, SchemeEditor};
        let s = self.ui();
        let theme = self.theme;
        let geo = geometry(area, s, editor.scroll);
        let ui = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let title = FontStyle::new(FontFamily::Ui, (17.0 * s).round(), true);
        let mono = FontStyle::new(FontFamily::Mono, (13.0 * s).round(), false);
        let radius = (8.0 * s).round();
        let line = s.round().max(1.0);
        let (mx, my) = self.mouse;
        g.dl.rect(area, theme.dim);
        g.dl.shadow(geo.frame, theme.shadow, radius, (18.0 * s).round());
        g.dl.round_rect(geo.frame, theme.overlay, radius);
        g.dl.outline(geo.frame, theme.overlay_border, radius, line);
        let text_y = baseline(g, geo.title, title);
        g.text(geo.title.x, text_y, t("Edit color scheme"), title, theme.text, geo.title.w);
        g.dl.set_clip(Some(geo.body));
        for (i, rect) in geo.fields.into_iter().enumerate() {
            if rect.bottom() <= geo.body.y || rect.y >= geo.body.bottom() {
                continue;
            }
            let labeled = Rect::new(rect.x, rect.y, rect.w, (18.0 * s).round());
            let y = baseline(g, labeled, small);
            g.text(labeled.x, y, &SchemeEditor::label(i), small, theme.text_dim, labeled.w);
            let input = Rect::new(rect.x, labeled.bottom(), rect.w, (27.0 * s).round());
            let focused = editor.focus == i;
            g.dl.round_rect(input, theme.surface, 4.0 * s);
            g.dl.outline(input, if focused { theme.accent } else { theme.overlay_border }, 4.0 * s, line);
            let swatch = Rect::new(input.right() - 25.0 * s, input.y + 5.0 * s, 20.0 * s, input.h - 10.0 * s).snap();
            if let Some(c) = (i > 0).then(|| editor.color(i)).flatten() {
                g.dl.round_rect(swatch, color(c), 3.0 * s);
                g.dl.outline(swatch, theme.overlay_border, 3.0 * s, line);
            }
            let style = if i == 0 { ui } else { mono };
            let x = input.x + 8.0 * s;
            let width = (if i == 0 { input.right() - 6.0 * s } else { swatch.x - 4.0 * s }) - x;
            let y = baseline(g, input, style);
            let value = &editor.fields[i];
            if focused && editor.replace {
                let w = g.measure(value, style).min(width).max(0.0);
                g.dl.rect(Rect::new(x, input.y + 3.0 * s, w, input.h - 6.0 * s), theme.selection);
            }
            let text_w = g.text(x, y, value, style, theme.text, width.max(0.0));
            if focused && !editor.replace {
                g.dl.rect(Rect::new(x + text_w.min(width.max(0.0)), input.y + 5.0 * s, line, input.h - 10.0 * s), theme.accent);
            }
        }
        g.dl.set_clip(None);
        if geo.max_scroll > 0.0 && geo.body.h > 0.0 {
            let h = (geo.body.h * geo.body.h / (geo.body.h + geo.max_scroll)).clamp(0.0, geo.body.h);
            let y = geo.body.y + (geo.body.h - h) * (editor.scroll / geo.max_scroll).clamp(0.0, 1.0);
            let thumb = Rect::new(geo.body.right() + 6.0 * s, y, 3.0 * s, h).snap();
            g.dl.round_rect(thumb, theme.overlay_border, 2.0 * s);
        }
        self.draw_palette_sample(g, geo.preview, editor.preview(), mono);
        let y = baseline(g, geo.message, small);
        let message = editor.error.map_or_else(|| t("Tab next field  ·  Ctrl+Enter save").to_owned(), |issue| issue.message());
        g.text(geo.message.x, y, &message, small, if editor.error.is_some() { theme.accent } else { theme.text_dim }, geo.message.w);
        for (rect, label, primary) in [(geo.cancel, t("Cancel"), false), (geo.save, t("Save"), true)] {
            g.dl.round_rect(rect, if rect.contains(mx, my) { theme.button_hover } else if primary { theme.accent.alpha(0x38) } else { theme.surface }, 5.0 * s);
            g.dl.outline(rect, if primary { theme.accent } else { theme.overlay_border }, 5.0 * s, line);
            centered(g, rect, label, ui, theme.text);
        }
    }

    fn draw_palette_sample<B: TextBackend>(&self, g: &mut Gfx<B>, area: Rect, palette: aster_vt::Palette, font: FontStyle) {
        let s = self.ui();
        let inner = area.inset(10.0 * s, 5.0 * s);
        let sample = t("Selected text");
        let step = g.measure("0", font).ceil().max(1.0);
        let line_h = (font.px() + 4.0 * s).ceil();
        let first = Rect::new(inner.x, inner.y, inner.w, line_h);
        let second = Rect::new(inner.x, first.bottom(), inner.w, line_h);
        let third = Rect::new(inner.x, second.bottom(), inner.w, line_h);
        g.dl.round_rect(area, color(palette.background), 5.0 * s);
        g.dl.outline(area, self.theme.overlay_border, 5.0 * s, s.round().max(1.0));
        g.dl.set_clip(Some(inner));
        let y = baseline(g, first, font);
        g.text(first.x, y, t("Terminal color preview"), font, color(palette.foreground), first.w);
        let y = baseline(g, second, font);
        let selected_w = g.measure(sample, font).min(second.w);
        g.dl.rect(Rect::new(second.x, second.y, selected_w, second.h), color(palette.selection));
        g.text(second.x, y, sample, font, color(palette.selection_text.unwrap_or(palette.foreground)), selected_w);
        let cursor = Rect::new(second.x + selected_w + step, second.y + 1.0, step, second.h - 2.0);
        g.dl.rect(cursor, color(palette.cursor));
        g.text(cursor.x, y, "_", font, color(palette.cursor_text.unwrap_or(palette.background)), cursor.w);
        let y = baseline(g, third, font);
        for (i, &c) in palette.ansi.iter().enumerate() {
            let x = third.x + step * (i * 2) as f32;
            g.text(x, y, &format!("{i:X}"), font, color(c), step * 2.0);
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

    fn draw_font_sample<B: TextBackend>(&self, g: &mut Gfx<B>, rect: Rect, ui: FontStyle) {
        let s = self.ui();
        let pad = (12.0 * s).round();
        let inner = rect.inset(pad, (6.0 * s).round());
        let label = Rect::new(inner.x, inner.y, inner.w, (17.0 * s).round());
        let line = Rect::new(inner.x, label.bottom(), inner.w, (inner.bottom() - label.bottom()).max(0.0));
        let radius = (6.0 * s).round();
        g.dl.round_rect(rect, color(self.palette.background), radius);
        g.dl.outline(rect, self.theme.overlay_border, radius, s.round().max(1.0));
        let y = baseline(g, label, ui);
        g.text(label.x, y, t("Font preview"), ui, color(self.palette.foreground), label.w);
        let size = self.metrics.map_or(self.config.terminal.font_size * self.scale, |m| m.px);
        let style = FontStyle { ligatures: self.config.terminal.ligatures, ..FontStyle::new(FontFamily::Mono, size.min(line.h).max(1.0), false) };
        let y = baseline(g, line, style);
        g.dl.set_clip(Some(line));
        let regular = format!("{}  -> == !=", t("Aa"));
        let advance = g.text(line.x, y, &regular, style, color(self.palette.foreground), line.w);
        let x = line.x + advance + pad;
        g.text(x, y, t("Aa"), FontStyle { bold: true, ..style }, color(self.palette.ansi[4]), (line.right() - x).max(0.0));
        g.dl.set_clip(None);
    }

    fn draw_scheme_swatches<B: TextBackend>(&self, g: &mut Gfx<B>, rect: Rect, font: FontStyle, (bg, fg, colors): (Color, Color, [Color; 16])) {
        let s = self.ui();
        let radius = (6.0 * s).round();
        let pad = (6.0 * s).round();
        g.dl.round_rect(rect, bg, radius);
        g.dl.outline(rect, self.theme.overlay_border, radius, s.round().max(1.0));
        let inner = rect.inset(pad, pad);
        let y = baseline(g, inner, font);
        let sample_w = g.text(inner.x, y, t("Aa"), font, fg, inner.w * 0.25);
        let grid_x = inner.x + sample_w.ceil() + pad;
        let gap = (3.0 * s).min((inner.right() - grid_x).max(0.0) / 16.0);
        let cell_w = ((inner.right() - grid_x - 7.0 * gap) / 8.0).max(0.0);
        let cell_h = (inner.h - gap) / 2.0;
        for (i, color) in colors.into_iter().enumerate() {
            let x = grid_x + (i % 8) as f32 * (cell_w + gap);
            let y = inner.y + (i / 8) as f32 * (cell_h + gap);
            g.dl.round_rect(Rect::new(x, y, cell_w, cell_h).snap(), color, 2.0 * s);
        }
    }

    fn draw_page_tabs<'a, B: TextBackend>(&self, g: &mut Gfx<B>, controls: &mut crate::control_motion::Frame<'_>, bar: crate::control_motion::Tabs, tabs: &[Rect], (selected, labels): (usize, impl IntoIterator<Item = &'a str>), font: FontStyle) {
        use crate::control_motion::Tabs;
        let Some(&target) = tabs.get(selected) else { return };
        let indicator = controls.indicator(bar, target);
        let s = self.ui();
        let radius = if bar == Tabs::ProfileEditor { 5.0 * s } else { (6.0 * s).round() };
        let line = s.round().max(1.0);
        let theme = self.theme;
        let (mx, my) = self.mouse;
        for &tab in tabs {
            if tab == indicator { continue; }
            let base = if bar == Tabs::ProfileEditor { theme.surface } else { theme.overlay };
            g.dl.round_rect(tab, if tab.contains(mx, my) { theme.button_hover } else { base }, radius);
            if matches!(bar, Tabs::Schemes | Tabs::Extensions) {
                g.dl.outline(tab, theme.overlay_border, radius, line);
            }
        }
        g.dl.round_rect(indicator, theme.accent.alpha(0x30), radius);
        g.dl.outline(indicator, theme.accent, radius, line);
        for (i, (&tab, label)) in tabs.iter().zip(labels).enumerate() {
            let ink = if i != selected && bar == Tabs::ProfileEditor { theme.text_dim } else { theme.text };
            let tab = if bar == Tabs::Extensions { tab.inset(5.0 * s, 0.0) } else { tab };
            centered(g, tab, label, font, ink);
        }
    }

    fn draw_extensions_page<B: TextBackend>(&self, g: &mut Gfx<B>, area: Rect, page: &crate::settings::SettingsPage, ui: FontStyle, small: FontStyle, controls: &mut crate::control_motion::Frame<'_>) {
        use crate::extensions_page::Target;
        let state = &page.extensions;
        let rows = self.extension_rows(page);
        let layout = state.layout(area, self.ui(), &rows);
        let s = self.ui();
        let theme = self.theme;
        let (mx, my) = self.mouse;
        let radius = (6.0 * s).round();
        let border = s.round().max(1.0);
        let error = Color::rgb(if theme.dark { 0xFF9490 } else { 0xA52320 });
        let button = |g: &mut Gfx<B>, rect: Rect, label: &str, target: Target, enabled: bool| {
            let fill = if enabled && rect.contains(mx, my) { theme.button_hover } else { theme.overlay };
            let focused = state.focus == Some(target);
            g.dl.round_rect(rect, fill, radius);
            g.dl.outline(rect, if focused { theme.accent } else { theme.overlay_border }, radius, border);
            let inner = rect.inset(5.0 * s, 0.0);
            centered(g, inner, label, ui, if enabled { theme.text } else { theme.text_dim });
        };
        g.dl.set_clip(Some(area));
        self.draw_page_tabs(g, controls, crate::control_motion::Tabs::Extensions, &layout.tabs, (usize::from(state.available), [t("Installed"), t("Available")]), ui);
        if let Some(Target::Tab(tab)) = state.focus.filter(|target| *target != Target::Tab(state.available)) {
            g.dl.outline(layout.tabs[usize::from(tab)], theme.accent, radius, border);
        }
        let field_label = if state.available { t("Extension index URL") } else { t("ZIP archive path") };
        self.draw_editor_field(g, layout.field, (field_label, &state.input), ProfileFieldControl::Text, state.editing);
        let submit_label = if state.available { t("Save") } else { t("Install from file…") };
        button(g, layout.submit, submit_label, Target::Submit, !state.busy && (state.available || !state.input.trim().is_empty()));
        if let Some(rect) = layout.root {
            button(g, rect, t("Open extensions folder"), Target::Root, !state.busy);
        }
        if let Some(text) = state.feedback.as_deref().or_else(|| state.busy.then(|| t("Please wait…"))) {
            let y = baseline(g, layout.feedback, small);
            g.text(layout.feedback.x, y, text, small, theme.text_dim, layout.feedback.w);
        }
        g.dl.set_clip(Some(layout.list));
        if rows.is_empty() {
            let text = state.empty_message(!self.config.extensions.index_url.trim().is_empty());
            let message = Rect::new(layout.list.x, layout.list.y + 12.0 * s, layout.list.w, 27.0 * s);
            let y = baseline(g, message, ui);
            g.text(message.x, y, &text, ui, theme.text_dim, message.w);
        }
        let name_font = FontStyle { bold: true, ..ui };
        for (i, ((rect, buttons), row)) in layout.rows.iter().zip(rows.iter()).enumerate() {
            if rect.bottom() <= layout.list.y || rect.y >= layout.list.bottom() { continue; }
            g.dl.round_rect(*rect, theme.overlay, radius);
            g.dl.outline(*rect, theme.overlay_border, radius, border);
            let inner = rect.inset(12.0 * s, 7.0 * s);
            let lines = [(&row.title, name_font, theme.text), (&row.metadata, small, theme.text_dim),
                (&row.description, small, if row.failed && row.status.is_empty() { error } else { theme.text_dim }),
                (&row.status, small, if row.failed { error } else { theme.text_dim })];
            for (j, (text, font, ink)) in lines.into_iter().enumerate() {
                let line = Rect::new(inner.x, inner.y + j as f32 * 19.0 * s, inner.w, 19.0 * s);
                let y = baseline(g, line, font);
                g.text(line.x, y, text, font, ink, line.w);
            }
            for (j, (rect, control)) in buttons.iter().zip(row.buttons.iter()).enumerate() {
                button(g, *rect, &control.label, Target::Row(i, j), control.enabled);
            }
        }
        if let Some(rect) = layout.scrollbar {
            g.dl.round_rect(rect, theme.overlay_border, 1.5 * s);
        }
        g.dl.set_clip(None);
    }

    fn draw_scheme_bar<B: TextBackend>(&self, g: &mut Gfx<B>, page: &crate::settings::SettingsPage, bar: crate::settings_colors::SchemeBar, ui: FontStyle, controls: &mut crate::control_motion::Frame<'_>) {
        use crate::settings_colors::SchemeTab;
        let s = self.ui();
        let theme = self.theme;
        let radius = (6.0 * s).round();
        let line = (1.0 * s).round().max(1.0);
        let (mx, my) = self.mouse;
        let name = page.scheme_tab.name(&self.config.appearance);
        let scheme = page.scheme_item(name).and_then(|item| page.scheme_at(item));
        let pending = page.deleting_scheme.as_deref();
        self.draw_page_tabs(g, controls, crate::control_motion::Tabs::Schemes, &bar.tabs, (usize::from(page.scheme_tab == SchemeTab::Light), SchemeTab::ALL.map(SchemeTab::label)), ui);
        let mut current = bar.current;
        let caption = if let Some(name) = pending {
            tf("Delete the color scheme {name}?", &[("name", name)])
        } else {
            let right = if scheme.is_some_and(|row| row.custom) { bar.cancel.x } else if scheme.is_some() { bar.delete.x } else { bar.delete.right() };
            current.w = (right - current.x).max(0.0);
            tf("Current color scheme: {name}", &[("name", scheme.map_or(name, |row| &row.name))])
        };
        let y = baseline(g, current, ui);
        g.text(current.x, y, &caption, ui, theme.text, current.w);
        let custom = scheme.is_some_and(|row| row.custom);
        let edit = if custom { bar.cancel } else { bar.delete };
        for (visible, rect, label) in [
            (scheme.is_some() && pending.is_none(), edit, t("Edit")),
            (pending.is_some(), bar.cancel, t("Cancel")),
            (custom || pending.is_some(), bar.delete, t("Delete scheme")),
        ] {
            if visible {
                g.dl.round_rect(rect, if rect.contains(mx, my) { theme.button_hover } else { theme.overlay }, radius);
                g.dl.outline(rect, theme.overlay_border, radius, line);
                centered(g, rect, label, ui, theme.text);
            }
        }
    }

    fn draw_sync_row<B: TextBackend>(&self, g: &mut Gfx<B>, page: &crate::settings::SettingsPage, geometry: crate::settings::Row, row: crate::sync_settings::Row, ui: FontStyle, small: FontStyle) {
        use crate::sync_settings::{button_rects, Button, Row};
        let s = self.ui();
        let selected = geometry.kind == crate::settings::RowKind::Item(page.selected);
        let editing = row == Row::New && selected && page.editing();
        let confirming = page.sync.confirming(row);
        let (label, detail) = page.sync.labels(row);
        let detail = if editing { page.notice.as_deref().unwrap_or(&detail) } else { &detail };
        let x = geometry.rect.x + (14.0 * s).round();
        let width = if row.stacked(geometry.rect.w, s) { geometry.rect.w - 28.0 * s } else { geometry.control.x - x - 12.0 * s };
        g.text(x, geometry.rect.y + (25.0 * s).round(), &label, ui, self.theme.text, width.max(1.0));
        g.text(x, geometry.rect.y + (43.0 * s).round(), detail, small, if confirming || editing && page.notice.is_some() { self.theme.accent } else { self.theme.text_dim }, width.max(1.0));
        let buttons = row.buttons(confirming, editing);
        let rects = button_rects(geometry.control, s, buttons.len(), editing);
        let radius = (6.0 * s).round();
        let line = s.round().max(1.0);
        if editing {
            let c = geometry.control;
            let field = Rect::new(c.x, c.y, (rects[0].x - c.x - 5.0 * s).max(1.0), c.h);
            g.dl.round_rect(field, self.theme.overlay, radius);
            g.dl.outline(field, self.theme.accent, radius, line);
            let inner = field.inset(8.0 * s, 0.0);
            let y = baseline(g, inner, ui);
            let text = page.edit_text();
            let width = if text.is_empty() {
                g.text(inner.x, y, t("Config name"), ui, self.theme.text_dim, inner.w);
                0.0
            } else {
                g.text(inner.x, y, &text, ui, self.theme.text, inner.w)
            };
            g.dl.rect(Rect::new(inner.x + width, field.y + 7.0 * s, line, field.h - 14.0 * s), self.theme.accent);
        }
        for (index, (&button, rect)) in buttons.iter().zip(rects).enumerate() {
            let enabled = button == Button::Cancel || self.sync_view.status != crate::sync::Status::Syncing && (!self.config.read_only() || button == Button::Refresh);
            let focused = selected && !editing && index == if confirming { 0 } else { page.sync.action.min(buttons.len() - 1) };
            let hover = enabled && rect.contains(self.mouse.0, self.mouse.1);
            g.dl.round_rect(rect, if hover { self.theme.button_hover } else { self.theme.overlay }, radius);
            g.dl.outline(rect, if focused { self.theme.accent } else { self.theme.overlay_border }, radius, line);
            centered(g, rect.inset(3.0 * s, 0.0), button.label(), ui, if enabled { self.theme.text } else { self.theme.text_dim });
        }
    }

    fn draw_settings<B: TextBackend>(&mut self, g: &mut Gfx<B>, now: Instant, controls: &mut crate::control_motion::Frame<'_>) {
        use crate::settings::{part_at, Control, Part, RowKind, VaultRow, SETTINGS};
        let s = self.ui();
        let ui = FontStyle::new(FontFamily::Ui, (13.5 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (11.5 * s).round(), false);
        let (_, _, width) = crate::settings::column_widths(self.chrome.content, s);
        let Some(page) = self.settings.as_ref() else {
            self.layers.start(&mut g.dl, Layer::RestartNotice, None, TOAST_OUT);
            self.hover_state.settings = Default::default();
            return;
        };
        let info_rows = page.shown.iter().filter_map(|&index| {
            let setting = SETTINGS.get(index).filter(|setting| matches!(setting.control, Control::ReadOnly(_)))?;
            let inset = if setting.key == "info.sync-status" { 43.0 } else { 28.0 };
            Some((index, wrapped_info(g, &self.setting_text(setting), small, (width - inset * s).max(1.0))))
        }).collect();
        let Some(page) = self.settings.as_mut() else { return };
        page.info_rows = info_rows;
        page.shortcut_rows.clear();
        for (n, &index) in page.shown.iter().enumerate() {
            if let Some(action) = page.shortcut(index) {
                let mut bindings: Vec<_> = self.keymap.bindings_of(&action).map(crate::keymap::format_binding).collect();
                let capturing = page.capture.as_ref().is_some_and(|(a, _)| *a == action);
                if capturing {
                    bindings = vec![t("Press a key, click, scroll or release modifiers").to_owned()];
                } else if bindings.is_empty() {
                    bindings.push(t("None").to_owned());
                }
                let hint = if capturing {
                    t("Press the new shortcut").to_owned()
                } else if n == page.selected {
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
        let animate = self.animations();
        self.settings.as_mut().unwrap().scroll = geo.scroll;
        let page = self.settings.as_ref().unwrap();
        let scroll = self.list_motion.settings.sample(geo.scroll, (geo.height - geo.list.h).max(0.0), animate && !page.editing(), now, &mut self.motion);
        let theme = self.theme;
        let line = (1.0 * s).round().max(1.0);
        let (mx, my) = self.mouse;
        g.dl.rect(geo.page, theme.surface);
        let title = FontStyle::new(FontFamily::Ui, (20.0 * s).round(), true);
        let section = FontStyle::new(FontFamily::Ui, (12.0 * s).round(), true);
        let ty = baseline(g, geo.title, title);
        g.text(geo.title.x, ty, t("Settings"), title, theme.text, geo.title.w);
        let radius = (6.0 * s).round();
        let extension_search = page.extensions_page() && page.extensions.available;
        let caret_h = (16.0 * s).round();
        if !page.extensions_page() || extension_search {
            g.dl.round_rect(geo.field, theme.overlay, radius);
            let editing = page.editing() || page.extensions.editing || self.config_editor_focused();
            g.dl.outline(geo.field, if editing { theme.overlay_border } else { theme.accent }, radius, line);
            let field_text = geo.field.inset((10.0 * s).round(), 0.0);
            let fy = baseline(g, field_text, ui);
            if page.query.is_empty() {
                let prompt = if extension_search { t("Search extensions") } else if page.colors_page() { t("Search color schemes") } else if page.searching_vault() { t("Search secrets") } else { t("Search settings") };
                g.text(field_text.x, fy, prompt, ui, theme.text_dim, field_text.w);
            }
            let qw = g.text(field_text.x, fy, &page.query, ui, theme.text, field_text.w);
            if !editing {
                g.dl.rect(Rect::new(field_text.x + qw + 1.0, geo.field.y + ((geo.field.h - caret_h) / 2.0).round(), line, caret_h), theme.accent);
            }
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
        if let Some(bar) = geo.scheme_bar {
            self.draw_scheme_bar(g, page, bar, ui, controls);
        }
        if let Some(sample) = geo.font_sample {
            self.draw_font_sample(g, sample, small);
        }
        let layer = Layer::RestartNotice;
        let from = self.layers.start(&mut g.dl, layer, self.layers.opened(layer, geo.restart.is_some()), TOAST_OUT);
        if let Some(button) = geo.restart {
            g.dl.round_rect(button, if button.contains(mx, my) { theme.button_hover } else { theme.overlay }, radius);
            g.dl.outline(button, theme.accent, radius, if page.selected == page.shown.len() { 2.0 * line } else { line });
            centered(g, button, t("Restart to apply"), ui, theme.text);
            self.layers.pop(&mut g.dl, layer, from, button, 0, TOAST_IN);
        }
        if page.extensions_page() {
            self.draw_extensions_page(g, geo.list, page, ui, small, controls);
            return;
        }
        if let Some(tabs) = geo.profile_tabs {
            self.draw_page_tabs(g, controls, crate::control_motion::Tabs::Profiles, &tabs, (usize::from(page.profiles_advanced), [t("Profiles"), t("Advanced")]), ui);
        }
        if self.config_editor_layout().is_some() {
            self.draw_config_editor(g, geo.list);
            return;
        }
        if page.shown.is_empty() {
            let message = if page.colors_page() { t("No color schemes match") } else if page.searching_vault() { t("No secrets match") } else { t("No settings match") };
            centered(g, Rect::new(geo.list.x, geo.list.y, geo.list.w, (80.0 * s).round()), message, ui, theme.text_dim);
        }
        let inset = (2.0 * s).round();
        let hot = geo.rows.iter().find_map(|row| match row.kind {
            RowKind::Item(n) if geo.list.contains(mx, my) && row.rect.inset(0.0, inset).contains(mx, my) => Some(page.shown[n]),
            _ => None,
        });
        let animate = self.animations();
        self.motion |= self.hover_state.settings.aim(hot, animate, now);
        g.dl.set_clip(Some(geo.list));
        let current_scheme = page.scheme_item(if page.colors_page() { page.scheme_tab.name(&self.config.appearance) } else { self.current_scheme() });
        let offset = geo.scroll.round() - scroll.round();
        let (mx, my) = if geo.list.contains(mx, my) { (mx, my + offset) } else { (-1.0, -1.0) };
        for row in &geo.rows {
            let row = crate::settings::Row {
                rect: Rect::new(row.rect.x, row.rect.y + offset, row.rect.w, row.rect.h),
                control: Rect::new(row.control.x, row.control.y + offset, row.control.w, row.control.h),
                ..*row
            };
            if row.rect.bottom() < geo.list.y || row.rect.y > geo.list.bottom() {
                continue;
            }
            match row.kind {
                RowKind::Section(name) => {
                    let mut r = Rect::new(row.rect.x + (14.0 * s).round(), row.rect.y + (10.0 * s).round(), row.rect.w, (28.0 * s).round());
                    if !page.searching() {
                        let angle = controls.angle(crate::control_motion::Control::Section(name), page.section_open(name));
                        let ink = if row.rect.contains(mx, my) { theme.accent } else { theme.text_dim };
                        g.dl.chevron((r.x + 4.0 * s, r.y + r.h * 0.5), 8.0 * s, angle, 1.5 * s, ink);
                        r.x += (18.0 * s).round();
                        r.w = (row.rect.right() - r.x).max(0.0);
                    }
                    let y = baseline(g, r, section);
                    g.text(r.x, y, t(name), section, theme.accent, r.w);
                    if name == crate::settings::SYSTEM && !page.searching() {
                        let version = if self.installation == crate::installation::State::Installed {
                            tf("Aster {version} · Installed for this user", &[("version", env!("CARGO_PKG_VERSION"))])
                        } else {
                            tf("Aster {version}", &[("version", env!("CARGO_PKG_VERSION"))])
                        };
                        g.text(r.x, y + (20.0 * s).round(), &version, ui, theme.text, r.w);
                    }
                }
                RowKind::Item(n) => {
                    let selected = n == page.selected;
                    let card = row.rect.inset(0.0, inset);
                    if selected {
                        g.dl.round_rect(card, theme.accent.alpha(0x26), radius);
                    } else if let Some(fill) = hover_fill(&self.hover_state.settings, page.shown[n], now, theme.button_hover) {
                        g.dl.round_rect(card, fill, radius);
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
                    if let Some(sync_row) = page.sync_row(item) {
                        self.draw_sync_row(g, page, row, sync_row, ui, small);
                        continue;
                    }
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
                        g.text(tx, row.rect.y + (25.0 * s).round(), &scheme.name, ui, theme.text, tw);
                        let marked = current_scheme == Some(item);
                        let detail_y = row.rect.y + (43.0 * s).round();
                        if page.colors_page() {
                            if marked {
                                g.dl.check(Rect::new(tx, row.rect.y + (32.0 * s).round(), 14.0 * s, 14.0 * s), theme.accent);
                            }
                            if let Some(source) = scheme.source.as_deref().or_else(|| scheme.custom.then(|| t("Custom scheme"))) {
                                let offset = if marked { 20.0 * s } else { 0.0 };
                                g.text(tx + offset, detail_y, source, small, theme.text_dim, (tw - offset).max(0.0));
                            }
                        } else {
                            let mut detail = scheme.source.as_deref().unwrap_or_else(|| if scheme.custom { t("Custom") } else { t("Built-in") }).to_owned();
                            if marked {
                                detail.push_str(" · ");
                                detail.push_str(t("Current"));
                            }
                            g.text(tx, detail_y, &detail, small, theme.text_dim, tw);
                        }
                        self.draw_scheme_swatches(g, c, small, (Color::rgb(scheme.background), Color::rgb(scheme.foreground), scheme.colors.map(Color::rgb)));
                        continue;
                    }
                    if let Some(tree) = page.tree_at(item) {
                        self.draw_profile_tree_row(g, page, tree, row.rect, (selected, (mx, my)), controls);
                        continue;
                    }
                    let setting = &SETTINGS[page.shown[n]];
                    let tx = row.rect.x + (14.0 * s).round();
                    if let Some(lines) = page.info_rows.get(&page.shown[n]) {
                        let tw = (row.rect.w - 28.0 * s).max(1.0);
                        g.text(tx, row.rect.y + (25.0 * s).round(), t(setting.label), ui, theme.text, tw);
                        let inset = if setting.key == "info.sync-status" {
                            let color = match &self.sync_view.status {
                                crate::sync::Status::Error(_) if self.config.sync.ready() => theme.close_hover,
                                crate::sync::Status::Syncing | crate::sync::Status::Synced(_) if self.config.sync.ready() => theme.accent,
                                _ if self.config.sync.ready() && self.config.sync.last_synced > 0 => theme.accent,
                                _ => theme.text_dim,
                            };
                            g.dl.circle(Rect::new(tx, row.rect.y + 36.0 * s, 7.0 * s, 7.0 * s), color);
                            15.0 * s
                        } else { 0.0 };
                        for (k, text) in lines.iter().enumerate() {
                            g.text(tx + inset, row.rect.y + (43.0 * s).round() + k as f32 * (16.0 * s).round(), text, small, theme.text_dim, (tw - inset).max(1.0));
                        }
                        continue;
                    }
                    let tw = (row.control.x - tx - 16.0 * s).max(0.0);
                    g.text(tx, row.rect.y + (25.0 * s).round(), t(setting.label), ui, theme.text, tw);
                    let notice = if setting.key == "appearance.background-image" && self.background.failed {
                        Some(t("Could not load the background image"))
                    } else if selected && setting.key == "vault.remember-minutes" {
                        page.vault_notice()
                    } else if selected {
                        page.notice.as_deref()
                    } else {
                        None
                    };
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
                                g.text(inner.x + ((inner.w - width) / 2.0).round(), y, text, ui, if chosen { theme.on_accent } else { theme.text }, inner.w);
                            }
                            g.dl.outline(c, theme.overlay_border, radius, line);
                        }
                        Control::Toggle => {
                            let progress = controls.switch(setting.key, self.setting_on(setting));
                            g.dl.round_rect(c, crate::control_motion::blend(theme.overlay_border, theme.accent, progress), c.h / 2.0);
                            let knob = (c.h - 6.0 * s).round();
                            let kx = crate::control_motion::knob_x(c, knob, 3.0 * s, progress);
                            g.dl.circle(Rect::new(kx, c.y + ((c.h - knob) / 2.0).round(), knob, knob), crate::control_motion::blend(theme.text_dim, theme.on_accent, progress));
                        }
                        Control::Run(action) => {
                            g.dl.round_rect(c, if c.contains(mx, my) { theme.button_hover } else { theme.overlay }, radius);
                            g.dl.outline(c, if selected { theme.accent } else { theme.overlay_border }, radius, line);
                            let label = match action {
                                crate::keymap::Action::ApplicationsMenu(true) => t("Add"),
                                crate::keymap::Action::ApplicationsMenu(false) => t("Remove"),
                                crate::keymap::Action::StartMenuShortcut => if self.start_menu_shortcut { t("Remove shortcut") } else { t("Create shortcut") },
                                crate::keymap::Action::InstallForUser => t("Install"),
                                crate::keymap::Action::EditEnvironment => t("Edit"),
                                _ => t("Open"),
                            };
                            centered(g, c, label, ui, theme.text);
                        }
                        Control::Slider { min, max, .. } if !(selected && page.editing()) => {
                            let track = crate::settings::slider_track(c, s);
                            let value = self.slider_value(setting, min).clamp(min, max);
                            let dragging = matches!(self.drag, Drag::SettingsSlider { item, .. } if item == page.shown[n]);
                            let fraction = controls.slider(setting.key, ((value - min) / (max - min)) as f32, dragging);
                            let fill = (track.w * fraction).round();
                            g.dl.round_rect(track, theme.overlay_border, track.h / 2.0);
                            g.dl.round_rect(Rect::new(track.x, track.y, fill, track.h), theme.accent, track.h / 2.0);
                            let knob = (12.0 * s).round();
                            g.dl.circle(Rect::new(track.x + fill - knob / 2.0, track.y + (track.h - knob) / 2.0, knob, knob), theme.accent);
                            let readout = Rect::new(track.right() + 10.0 * s, c.y, (c.right() - track.right() - 10.0 * s).max(0.0), c.h);
                            centered(g, readout, &format!("{:.0}%", value * 100.0), small, theme.text);
                        }
                        control => {
                            let editing = selected && page.editing() && matches!(control, Control::Text | Control::Color | Control::StringList | Control::Font { .. } | Control::Number { .. } | Control::Slider { .. });
                            g.dl.round_rect(c, theme.overlay, radius);
                            g.dl.outline(c, if editing { theme.accent } else { theme.overlay_border }, radius, line);
                            let stepper = !matches!(control, Control::Text | Control::Color | Control::StringList | Control::Font { .. } | Control::Slider { .. });
                            let edge = if stepper { (30.0 * s).round() } else { (10.0 * s).round() };
                            let mut inner = Rect::new(c.x + edge, c.y, (c.w - 2.0 * edge).max(0.0), c.h);
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
                            if matches!(control, Control::Color) {
                                inner = color_field(g, inner, &text, theme, s);
                            }
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
        let (mx, my) = self.mouse;
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
        let layer = Layer::SettingsVault;
        let dialog = page.vault_dialog.as_ref();
        let from = self.layers.start(&mut g.dl, layer, dialog.map(|dialog| dialog.opened_at), POP);
        if let Some(dialog) = dialog {
            let (rect, sensitive) = self.draw_vault_dialog(g, geo.page, dialog, page.vault.unlocked);
            self.layers.pop(&mut g.dl, layer, from, rect, 1, POP);
            if sensitive {
                self.layers.forget_snapshot(layer);
            }
            return;
        }
        if let Some(editor) = &page.scheme_editor {
            self.draw_scheme_editor(g, geo.page, editor);
            return;
        }
        if page.editor.is_some() || page.group_editor.is_some() {
            self.draw_profile_editor(g, geo.page, page, controls);
            return;
        }
        if geo.height > geo.list.h {
            let track = geo.list;
            let thumb_h = (track.h * track.h / geo.height).max(24.0 * s);
            let max_scroll = geo.height - track.h;
            let thumb_y = track.y + (track.h - thumb_h) * (scroll / max_scroll).clamp(0.0, 1.0);
            let w = (4.0 * s).round();
            g.dl.round_rect(Rect::new(track.right() + (8.0 * s).round(), thumb_y.round(), w, thumb_h.round()), theme.overlay_border, w / 2.0);
        }
    }

    fn draw_config_editor<B: TextBackend>(&mut self, g: &mut Gfx<B>, area: Rect) {
        use crate::config_editor::Side;
        let s = self.ui();
        let ui = FontStyle::new(FontFamily::Ui, (13.0 * s).round(), false);
        let small = FontStyle::new(FontFamily::Ui, (12.0 * s).round(), false);
        let mono = FontStyle::new(FontFamily::Mono, (13.0 * s).round(), false);
        self.config_editor.cell_width = g.measure("M", mono).max(s);
        let path = self.config_path.as_deref().unwrap_or_else(|| t("Config folder unavailable"));
        let path = wrapped_info(g, path, small, area.w);
        self.config_editor.path_rows = path.len();
        let Some(layout) = self.config_editor_layout() else { return };
        let editor = &self.config_editor;
        let theme = self.theme;
        let (mx, my) = self.mouse;
        let warning = Color::rgb(if theme.dark { 0xFF9490 } else { 0xA52320 });
        let line = s.round().max(1.0);
        let radius = 5.0 * s;
        g.dl.set_clip(Some(area));
        for (row, path) in path.iter().take(3).enumerate() {
            let rect = Rect::new(layout.path.x, layout.path.y + (3.0 + row as f32 * 16.0) * s, layout.path.w, 16.0 * s);
            let y = baseline(g, rect, small);
            g.text(rect.x, y, path, small, theme.text_dim, rect.w);
        }
        let buttons = [
            (Some(layout.save), editor.save_label(), editor.can_save(self.config.read_only()), true),
            (Some(layout.defaults), if editor.defaults.is_some() { t("Hide defaults") } else { t("Show the defaults") }, true, false),
            (Some(layout.reveal), t("Show file"), self.config_path.is_some(), false),
            (Some(layout.open), t("Open in the editor"), self.config_path.is_some(), false),
            (layout.reload, t("Reload from disk"), true, false),
        ];
        for (rect, label, enabled, primary) in buttons {
            let Some(rect) = rect else { continue };
            let accent = primary && enabled;
            let fill = if enabled && rect.contains(mx, my) { theme.button_hover } else if accent { theme.accent.alpha(0x30) } else { theme.overlay };
            g.dl.round_rect(rect, fill, radius);
            g.dl.outline(rect, if accent { theme.accent } else { theme.overlay_border }, radius, line);
            centered(g, rect.inset(5.0 * s, 0.0), label, ui, if enabled { theme.text } else { theme.text_dim });
        }
        let (status, failed) = editor.status(&self.config);
        for (row, message) in wrapped_info(g, &status, small, layout.status.w).iter().take(2).enumerate() {
            let r = Rect::new(layout.status.x, layout.status.y + row as f32 * 17.0 * s, layout.status.w, 17.0 * s);
            let y = baseline(g, r, small);
            g.text(r.x, y, message, small, if failed { warning } else { theme.text_dim }, r.w);
        }
        self.draw_editor_text(g, layout.file, &editor.file, t("Config file"), editor.focus == Some(Side::Current), editor.error_line());
        if let Some((view, defaults)) = layout.reference.zip(editor.defaults.as_ref()) {
            self.draw_editor_text(g, view, defaults, t("Defaults · read-only"), editor.focus == Some(Side::Defaults), None);
        }
        g.dl.set_clip(None);
    }

    fn draw_editor_text<B: TextBackend>(&self, g: &mut Gfx<B>, view: crate::config_editor::EditorView, editor: &crate::text_editor::TextEditor, title: &str, focused: bool, error_line: Option<usize>) {
        let s = self.ui();
        let theme = self.theme;
        let ui = FontStyle::new(FontFamily::Ui, (12.0 * s).round(), true);
        let mono = FontStyle::new(FontFamily::Mono, (13.0 * s).round(), false);
        let line_width = s.round().max(1.0);
        let warning = Color::rgb(if theme.dark { 0xFF9490 } else { 0xA52320 });
        g.dl.set_clip(Some(view.frame));
        g.dl.round_rect(view.frame, theme.overlay, 6.0 * s);
        g.dl.outline(view.frame, if focused { theme.accent } else { theme.overlay_border }, 6.0 * s, line_width);
        let ty = baseline(g, view.title, ui);
        g.text(view.title.x, ty, title, ui, theme.text_dim, view.title.w);
        g.dl.rect(Rect::new(view.title.x, view.text.y - line_width, view.title.w, line_width), theme.overlay_border);
        let selection = editor.selection();
        let (cursor_row, cursor_col) = editor.cursor_position();
        for (visible_row, (start, text)) in editor.lines().skip(editor.top).take(view.rows()).enumerate() {
            let row = editor.top + visible_row;
            let rect = Rect::new(view.text.x, view.text.y + visible_row as f32 * view.row_height, view.text.w, view.row_height);
            let error = error_line == Some(row);
            if error || focused && row == cursor_row {
                g.dl.rect(rect, if error { warning.alpha(0x22) } else { theme.accent.alpha(0x14) });
            }
            let end = start + text.len();
            let character = |offset: usize| text[..offset - start].chars().count();
            let selected_start = selection.start.max(start);
            let selected_end = selection.end.min(end);
            if !selection.is_empty() && (selected_start < selected_end || selection.contains(&end)) {
                let left = character(selected_start.min(end)).saturating_sub(editor.left).min(view.cols());
                let right = (character(selected_end) + usize::from(selection.contains(&end))).saturating_sub(editor.left).min(view.cols());
                if right > left {
                    g.dl.rect(Rect::new(rect.x + left as f32 * view.cell_width, rect.y, (right - left) as f32 * view.cell_width, rect.h), theme.selection);
                }
            }
            let number = (row + 1).to_string();
            let number_width = g.measure(&number, mono);
            let y = baseline(g, rect, mono);
            let number_x = (view.text.x - 9.0 * s - number_width).max(view.frame.x + 3.0 * s);
            g.text(number_x, y, &number, mono, if error { warning } else { theme.text_dim }, (view.text.x - number_x - 4.0 * s).max(0.0));
            let text: String = text.chars().skip(editor.left).take(view.cols()).collect();
            g.text(rect.x, y, &text, mono, theme.text, rect.w);
            if focused && row == cursor_row && (editor.left..editor.left + view.cols()).contains(&cursor_col) {
                let x = rect.x + (cursor_col - editor.left) as f32 * view.cell_width;
                g.dl.rect(Rect::new(x.round(), rect.y + 3.0 * s, line_width, (rect.h - 6.0 * s).max(0.0)), theme.accent);
            }
        }
    }

    fn draw_overlay<B: TextBackend>(&mut self, g: &mut Gfx<B>, now: Instant) {
        let layer = Layer::Overlay;
        let overlay = self.overlay.clone();
        let from = self.layers.start(&mut g.dl, layer, overlay.as_ref().map(|overlay| overlay.opened_at), POP);
        let Some(o) = overlay else {
            self.hover_state.overlay = Default::default();
            return;
        };
        let animate = self.animations();
        let (geo, hover) = if o.kind.scrolls_smoothly() {
            let layout = crate::overlay_list::OverlayList::new(self.chrome.content, self.ui(), &o);
            let target = layout.target(&o);
            let scroll = self.list_motion.overlay.sample(target, layout.max, animate && o.scroll_offset.is_some(), now, &mut self.motion);
            let hover = layout.hit_row(target, self.mouse);
            (layout.geometry(scroll), hover)
        } else {
            let geo = overlay_geometry(self, &o);
            let hover = overlay_row_at(&geo, self.mouse).map(|(i, _)| i);
            (geo, hover)
        };
        let hot = hover.map(|i| o.shown[i].0);
        self.motion |= self.hover_state.overlay.aim(hot, animate, now);
        self.draw_overlay_content(g, &o, &geo, now);
        self.layers.pop(&mut g.dl, layer, from, geo.frame, 0, POP);
    }

    fn draw_overlay_content<B: TextBackend>(&self, g: &mut Gfx<B>, o: &Overlay, geo: &OverlayGeometry, now: Instant) {
        let (mx, my) = self.mouse;
        let s = self.ui();
        let theme = self.theme;
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
            let message = match o.kind {
                OverlayKind::SerialSpeed(_) => t("Enter a speed from 50 to 4000000 baud"),
                OverlayKind::SshPorts(pane) if !self.ssh_forwards.get(&pane).is_some_and(|items| !items.is_empty()) => t("No SSH port forwards"),
                _ => t("No matches"),
            };
            centered(g, r, message, ui, theme.text_dim);
            return;
        }
        g.dl.set_clip(Some(geo.list));
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
            } else if let Some(fill) = hover_fill(&self.hover_state.overlay, *item, now, theme.button_hover) {
                g.dl.round_rect(*r, fill, (5.0 * s).round());
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
            let port_action = matches!(o.kind, OverlayKind::SshPorts(_)) && !pick.badge.is_empty();
            if port_action {
                let action = crate::ssh_toolbar::forward_action_rect(*r, s);
                g.dl.round_rect(action, if action.contains(mx, my) { theme.button_hover } else { theme.surface }, 4.0 * s);
                g.dl.outline(action, theme.overlay_border, 4.0 * s, s.round().max(1.0));
                centered(g, action, &pick.badge, small, theme.text);
                right = action.x - 8.0 * s;
            }
            let label_room = if port_action && !pick.hint.is_empty() { ((right - x) * 0.52).max(0.0) } else { (right - x).max(0.0) };
            let label_w = g.measure(&pick.label, ui).min(label_room);
            g.text(x, y, &pick.label, ui, theme.text, label_room);
            if !pick.hint.is_empty() && pick.hint != pick.label {
                let hy = baseline(g, *r, small);
                if profiles {
                    let hx = x + label_w + (10.0 * s).round();
                    if right - hx > 24.0 * s {
                        g.text(hx, hy, &pick.hint, small, theme.text_dim, right - hx);
                    }
                } else if port_action {
                    let hx = x + label_w + 9.0 * s;
                    g.text(hx, hy, &pick.hint, small, theme.text_dim, (right - hx).max(0.0));
                } else {
                    let hint_w = g.measure(&pick.hint, small).min(right - x);
                    g.text(right - hint_w, hy, &pick.hint, small, theme.text_dim, hint_w + 1.0);
                }
            }
        }
        g.dl.set_clip(None);
    }
}

pub fn hint_height(scale: f32) -> f32 {
    (30.0 * scale).round()
}
