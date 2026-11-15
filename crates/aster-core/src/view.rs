use crate::app::{App, Drag, Effect, Metrics};
use crate::input::HitArea;
use crate::overlay::{Overlay, VISIBLE};
use crate::panes::PaneId;
use crate::theme::color;
use aster_gfx::render::{Color, Rect};
use aster_gfx::text::{FontFamily, FontStyle, Gfx, TextBackend};
use aster_vt::{CursorShape, Flags, Rgb, Size};

pub const TAB_H: f32 = 36.0;
const CAPTION_W: f32 = 46.0;
const PLUS_W: f32 = 36.0;
const TAB_MAX: f32 = 220.0;
const TAB_MIN: f32 = 84.0;
const TAB_TOP: f32 = 6.0;
const PAD: f32 = 6.0;
const G_CLOSE: &str = "\u{E8BB}";
const G_PLUS: &str = "\u{E710}";
const G_MIN: &str = "\u{E921}";
const G_MAX: &str = "\u{E922}";
const G_RESTORE: &str = "\u{E923}";

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TabSlot {
    pub rect: Rect,
    pub close: Rect,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chrome {
    pub tab_bar: Rect,
    pub tabs: Vec<TabSlot>,
    pub plus: Rect,
    pub min: Rect,
    pub max: Rect,
    pub close: Rect,
    pub content: Rect,
}

pub fn chrome(w: f32, h: f32, s: f32, tabs: usize) -> Chrome {
    let bar_h = (TAB_H * s).round();
    let cw = (CAPTION_W * s).round();
    let close = Rect::new(w - cw, 0.0, cw, bar_h);
    let max = Rect::new(w - 2.0 * cw, 0.0, cw, bar_h);
    let min = Rect::new(w - 3.0 * cw, 0.0, cw, bar_h);
    let left = (8.0 * s).round();
    let plus_w = (PLUS_W * s).round();
    let avail = (min.x - left - plus_w - 16.0 * s).max(0.0);
    let tab_w = if tabs == 0 { 0.0 } else { (avail / tabs as f32).clamp(TAB_MIN * s, TAB_MAX * s).floor() };
    let top = (TAB_TOP * s).round();
    let close_w = (24.0 * s).round();
    let slots: Vec<TabSlot> = (0..tabs)
        .map(|i| {
            let rect = Rect::new(left + i as f32 * tab_w, top, tab_w, bar_h - top);
            let close = Rect::new(rect.right() - close_w - (6.0 * s).round(), rect.y + ((rect.h - close_w) / 2.0).round(), close_w, close_w);
            TabSlot { rect, close }
        })
        .collect();
    let plus_x = (left + tabs as f32 * tab_w + 4.0 * s).min(min.x - plus_w);
    let plus = Rect::new(plus_x, top + ((bar_h - top - plus_w * 0.8) / 2.0).round(), plus_w, (plus_w * 0.8).round());
    Chrome { tab_bar: Rect::new(0.0, 0.0, w, bar_h), tabs: slots, plus, min, max, close, content: Rect::new(0.0, bar_h, w, (h - bar_h).max(0.0)) }
}

pub fn pane_inner(r: Rect, s: f32) -> Rect {
    let p = (PAD * s).round();
    r.inset(p, p)
}

fn rgb(c: Rgb) -> Color {
    color(c)
}

fn baseline<B: TextBackend>(g: &mut Gfx<B>, r: Rect, style: FontStyle) -> f32 {
    let m = g.metrics(style);
    (r.y + (r.h + m.ascent - m.descent) / 2.0).round()
}

fn centered<B: TextBackend>(g: &mut Gfx<B>, r: Rect, text: &str, style: FontStyle, c: Color) {
    let w = g.measure(text, style);
    let y = baseline(g, r, style);
    g.text((r.x + (r.w - w) / 2.0).round(), y, text, style, c, r.w);
}

pub struct OverlayGeometry {
    pub frame: Rect,
    pub input: Rect,
    pub items: Vec<(usize, Rect)>,
}

pub fn overlay_geometry(app: &App, o: &Overlay) -> OverlayGeometry {
    let s = app.scale;
    let (w, _) = app.size;
    let width = (560.0 * s).min(w - 32.0 * s).max(200.0 * s).round();
    let x = ((w - width) / 2.0).round();
    let y = app.chrome.content.y + (10.0 * s).round();
    let input_h = (40.0 * s).round();
    let item_h = (30.0 * s).round();
    let pad = (6.0 * s).round();
    let n = o.shown.len().min(VISIBLE);
    let rows = n.max(1);
    let frame = Rect::new(x, y, width, input_h + rows as f32 * item_h + 2.0 * pad);
    let input = Rect::new(x, y, width, input_h);
    let items = (0..n).map(|k| (o.scroll + k, Rect::new(x + pad, y + input_h + pad + k as f32 * item_h, width - 2.0 * pad, item_h))).collect();
    OverlayGeometry { frame, input, items }
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
        g.set_mono_family(&self.config.terminal.font);
        g.set_ui_family(&self.config.appearance.ui_font);
        let m = self.metrics_for(g);
        self.metrics = Some(m);
        let s = self.scale;
        let (w, h) = self.size;
        self.chrome = chrome(w, h, s, self.tabs.len());
        let theme = self.theme;
        g.begin(theme.chrome);
        self.draw_tab_bar(g, maximized);
        let content = self.chrome.content;
        g.dl.rect(content, theme.background);
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
        for (id, rect) in &layout.panes {
            self.draw_pane(g, *id, *rect, focus == Some(*id), m);
            if split && focus != Some(*id) {
                g.dl.rect(*rect, theme.dim);
            }
        }
        for sp in &layout.splitters {
            g.dl.rect(sp.rect, theme.divider);
        }
        self.draw_search(g);
        if let Some(o) = self.overlay.clone() {
            self.draw_overlay(g, &o);
        }
        self.mark_drawn();
    }

    fn draw_tab_bar<B: TextBackend>(&mut self, g: &mut Gfx<B>, maximized: bool) {
        let s = self.scale;
        let theme = self.theme;
        let c = self.chrome.clone();
        let ui = FontStyle::new(FontFamily::Ui, (12.5 * s).round(), false);
        let icons = FontStyle::new(FontFamily::Icons, (10.0 * s).round(), false);
        let (mx, my) = self.mouse;
        let radius = (8.0 * s).round();
        g.dl.set_clip(Some(c.tab_bar));
        for (i, slot) in c.tabs.iter().enumerate() {
            let active = i == self.active;
            let hovered = slot.rect.contains(mx, my);
            let dragging = matches!(self.drag, Drag::Tab { index, moved: true, .. } if index == i);
            let r = slot.rect;
            if active || dragging {
                g.dl.round_rect(Rect::new(r.x, r.y, r.w, r.h + radius), theme.tab_active, radius);
            } else if hovered {
                g.dl.round_rect(Rect::new(r.x + 2.0 * s, r.y + 2.0 * s, r.w - 4.0 * s, r.h - 6.0 * s), theme.tab_hover, (6.0 * s).round());
            } else if i + 1 != self.active && i + 1 < c.tabs.len() {
                let lh = (16.0 * s).round();
                g.dl.rect(Rect::new(r.right() - 1.0, r.y + ((r.h - lh) / 2.0).round(), (1.0 * s).max(1.0), lh), theme.divider);
            }
            let show_close = active || hovered;
            let text_x = r.x + (12.0 * s).round();
            let text_end = if show_close { slot.close.x - 4.0 * s } else { r.right() - 12.0 * s };
            let title = self.tab_title(i);
            let y = baseline(g, r, ui);
            g.text(text_x, y, &title, ui, if active { theme.text } else { theme.text_dim }, (text_end - text_x).max(0.0));
            if show_close {
                if slot.close.contains(mx, my) {
                    g.dl.round_rect(slot.close, theme.button_hover, (4.0 * s).round());
                }
                centered(g, slot.close, G_CLOSE, FontStyle::new(FontFamily::Icons, (8.0 * s).round(), false), if active { theme.text } else { theme.text_dim });
            }
        }
        if c.plus.contains(mx, my) {
            g.dl.round_rect(c.plus, theme.button_hover, (6.0 * s).round());
        }
        centered(g, c.plus, G_PLUS, icons, theme.text_dim);
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

    fn draw_pane<B: TextBackend>(&mut self, g: &mut Gfx<B>, id: PaneId, rect: Rect, focused: bool, m: Metrics) {
        let s = self.scale;
        let inner = pane_inner(rect, s);
        let size = Size::new((inner.w / m.w).floor().max(2.0) as usize, (inner.h / m.h).floor().max(1.0) as usize);
        let mut effects = Vec::new();
        let Some(pane) = self.panes.get_mut(&id) else { return };
        if let Some(mut launch) = pane.pending.take() {
            launch.cols = size.cols as u16;
            launch.rows = size.rows as u16;
            {
                let mut t = pane.terminal.lock();
                t.resize(size);
                t.set_cell_pixels(m.w as u16, m.h as u16);
            }
            pane.size = size;
            effects.push(Effect::Spawn { pane: id, launch, terminal: pane.terminal.clone() });
            if !pane.queued.is_empty() {
                effects.push(Effect::Write { pane: id, bytes: std::mem::take(&mut pane.queued) });
            }
        } else if pane.size != size {
            {
                let mut t = pane.terminal.lock();
                t.resize(size);
                t.set_cell_pixels(m.w as u16, m.h as u16);
            }
            pane.size = size;
            if pane.exited.is_none() {
                effects.push(Effect::Resize { pane: id, cols: size.cols as u16, rows: size.rows as u16 });
            }
        }
        let palette = self.palette;
        let window_focused = self.focused && self.overlay.is_none();
        let blink_on = self.blink_on || !self.config.terminal.cursor_blink;
        let theme = self.theme;
        let terminal = pane.terminal.clone();
        let mut t = terminal.lock();
        let matches = if self.search.as_ref().is_some_and(|s| s.pane == id) { t.visible_matches() } else { Vec::new() };
        if !matches.is_empty() {
            if let Some(bar) = self.search.as_mut() {
                bar.missing = false;
            }
        }
        let overrides = *t.colors();
        let default_bg = palette.background(&overrides);
        if default_bg != palette.background {
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
        for mt in &matches {
            let color = if mt.current { Color::rgb(0xF0A030).alpha(0xC0) } else { Color::rgb(0xE5C07B).alpha(0x60) };
            g.dl.rect(cell_rect(mt.line, mt.start, mt.end.saturating_sub(mt.start).max(1)), color);
        }
        let cursor = t.cursor();
        let show_cursor = cursor.shape != CursorShape::Hidden;
        let cursor_color = overrides[aster_vt::NamedColor::Cursor].unwrap_or(palette.cursor);
        let cur_cols = if cursor.wide { 2 } else { 1 };
        let cr = cell_rect(cursor.line, cursor.col, cur_cols);
        let active_cursor = focused && window_focused;
        let draw_cursor = show_cursor && (!active_cursor || (blink_on && self.preedit.is_empty()));
        let thin = (1.0 * s).round().max(1.0);
        let block = draw_cursor && active_cursor && matches!(cursor.shape, CursorShape::Block);
        if draw_cursor {
            match (active_cursor, cursor.shape) {
                (true, CursorShape::Block) => g.dl.rect(cr, rgb(cursor_color)),
                (true, CursorShape::Beam) => g.dl.rect(Rect::new(cr.x, cr.y, (2.0 * s).round().max(1.0), cr.h), rgb(cursor_color)),
                (true, CursorShape::Underline) => g.dl.rect(Rect::new(cr.x, cr.bottom() - (2.0 * s).round().max(1.0), cr.w, (2.0 * s).round().max(1.0)), rgb(cursor_color)),
                _ => g.dl.outline(cr, rgb(cursor_color), 0.0, thin),
            }
        }
        let px = m.px;
        let cursor_text = palette.cursor_text.unwrap_or(default_bg);
        let selection_text = palette.selection_text;
        let under_y = (m.baseline + (m.h - m.baseline) * 0.35).round().min(m.h - thin);
        t.visible(|v| {
            let flags = v.cell.flags;
            if flags.contains(Flags::WIDE_CHAR_SPACER) {
                return;
            }
            let (mut fg, _) = palette.cell(v.cell, &overrides);
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
            if c != ' ' && c != '\t' && !flags.contains(Flags::HIDDEN) && !g.cell_box(x, y, c, m.w as u32, m.h as u32, rgb(fg)) {
                let style = FontStyle::new(FontFamily::Mono, px, flags.contains(Flags::BOLD)).italic(flags.contains(Flags::ITALIC));
                g.cell(x, y + m.baseline, c, style, rgb(fg));
            }
            if flags.intersects(Flags::ALL_UNDERLINES | Flags::STRIKEOUT) {
                let span = if flags.contains(Flags::WIDE_CHAR) { 2.0 } else { 1.0 };
                let w = m.w * span;
                let ucolor = v.cell.underline_color().map(|c| palette.resolve(c, &overrides)).unwrap_or(fg);
                let uy = y + under_y;
                if flags.contains(Flags::UNDERCURL) {
                    let band = (3.0 * s).round().max(3.0);
                    g.dl.undercurl(Rect::new(x, uy - band / 2.0, w, band), rgb(ucolor), thin);
                } else if flags.contains(Flags::DOUBLE_UNDERLINE) {
                    g.dl.rect(Rect::new(x, uy - thin, w, thin), rgb(ucolor));
                    g.dl.rect(Rect::new(x, uy + thin, w, thin), rgb(ucolor));
                } else if flags.contains(Flags::DOTTED_UNDERLINE) || flags.contains(Flags::DASHED_UNDERLINE) {
                    let dash = if flags.contains(Flags::DOTTED_UNDERLINE) { thin } else { (3.0 * s).round() };
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
        });
        let offset = t.display_offset();
        let history = t.history();
        drop(t);
        if let Some(link) = self.hover_link.as_ref().filter(|l| l.pane == id) {
            let x = ox + link.start as f32 * m.w;
            let w = (link.end.saturating_sub(link.start)) as f32 * m.w;
            g.dl.rect(Rect::new(x, oy + link.line as f32 * m.h + under_y, w, thin), theme.accent);
        }
        if focused && !self.preedit.is_empty() {
            let style = FontStyle::new(FontFamily::Mono, px, false);
            let w = g.measure(&self.preedit.text, style);
            let r = Rect::new(cr.x, cr.y, w.max(m.w), m.h);
            g.dl.rect(r, theme.background);
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
        g.dl.set_clip(None);
        if focused {
            self.set_ime_area(cr);
        }
        for e in effects {
            self.push(e);
        }
    }

    fn draw_search<B: TextBackend>(&mut self, g: &mut Gfx<B>) {
        use crate::search::SearchHit;
        let Some(geo) = self.search_geometry() else { return };
        let Some(bar) = self.search.clone() else { return };
        let s = self.scale;
        let theme = self.theme;
        let radius = (6.0 * s).round();
        g.dl.shadow(Rect::new(geo.frame.x, geo.frame.y + 3.0 * s, geo.frame.w, geo.frame.h), theme.shadow, radius, (10.0 * s).round());
        g.dl.round_rect(geo.frame, theme.overlay, radius);
        let border = if bar.error || bar.missing { Color::rgb(0xD9534F) } else { theme.overlay_border };
        g.dl.outline(geo.frame, border, radius, (1.0 * s).round().max(1.0));
        let ui = FontStyle::new(FontFamily::Ui, (12.5 * s).round(), false);
        let y = baseline(g, geo.field, ui);
        if bar.query.is_empty() {
            g.text(geo.field.x, y, "Search", ui, theme.text_dim, geo.field.w);
        }
        let w = g.text(geo.field.x, y, &bar.query, ui, theme.text, geo.field.w);
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

    fn draw_overlay<B: TextBackend>(&mut self, g: &mut Gfx<B>, o: &Overlay) {
        let s = self.scale;
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
            centered(g, r, "No matches", ui, theme.text_dim);
            return;
        }
        let (mx, my) = self.mouse;
        for (i, r) in &geo.items {
            let (item, _) = &o.shown[*i];
            let pick = &o.items[*item];
            if *i == o.selected {
                g.dl.round_rect(*r, theme.accent.alpha(0x38), (5.0 * s).round());
            } else if r.contains(mx, my) {
                g.dl.round_rect(*r, theme.button_hover, (5.0 * s).round());
            }
            let y = baseline(g, *r, ui);
            let hint_w = if pick.hint.is_empty() { 0.0 } else { g.measure(&pick.hint, small) };
            g.text(r.x + (10.0 * s).round(), y, &pick.label, ui, theme.text, r.w - hint_w - 30.0 * s);
            if hint_w > 0.0 {
                let hy = baseline(g, *r, small);
                g.text(r.right() - hint_w - (10.0 * s).round(), hy, &pick.hint, small, theme.text_dim, hint_w + 1.0);
            }
        }
    }
}
