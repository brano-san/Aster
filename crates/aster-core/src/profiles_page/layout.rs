use aster_gfx::render::Rect;
use super::editor::ProfileEditor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorHit {
    Tab(usize),
    Field(usize),
    Save,
    Cancel,
}

pub(crate) struct EditorLayout {
    pub frame: Rect,
    pub title: Rect,
    pub tabs: Vec<Rect>,
    pub fields: Vec<(usize, Rect)>,
    pub group_defaults: Option<Rect>,
    pub scheme_preview: Option<Rect>,
    pub message: Rect,
    pub save: Rect,
    pub cancel: Rect,
    pub thumb: Option<Rect>,
}

impl EditorLayout {
    pub fn hit(&self, x: f32, y: f32) -> Option<EditorHit> {
        if self.save.contains(x, y) {
            return Some(EditorHit::Save);
        }
        if self.cancel.contains(x, y) {
            return Some(EditorHit::Cancel);
        }
        self.tabs.iter().position(|r| r.contains(x, y)).map(EditorHit::Tab)
            .or_else(|| self.fields.iter().find(|(_, r)| r.contains(x, y)).map(|(i, _)| EditorHit::Field(*i)))
    }
}

pub(crate) fn row_action(row: Rect, s: f32) -> Rect {
    let h = (30.0 * s).min(row.h - 4.0 * s);
    Rect::new(row.right() - 90.0 * s, row.y + (row.h - h) / 2.0, 78.0 * s, h).snap()
}

pub(crate) fn choice_step(slot: Rect, s: f32, x: f32) -> isize {
    if x >= slot.right() - 24.0 * s { 1 } else if x >= slot.right() - 48.0 * s { -1 } else { 0 }
}

pub(crate) fn editor_layout(area: Rect, s: f32, tabs: usize, fields: usize, focus: usize) -> EditorLayout {
    let compact = area.h < 260.0 * s;
    let available = area.inset(if compact { 4.0 } else { 12.0 } * s, if compact { 2.0 } else { 10.0 } * s);
    let width = available.w.min(690.0 * s);
    let columns = if width >= 520.0 * s && fields > 3 { 2 } else { 1 };
    let gap = (16.0 * s).round();
    let pad = (if compact { 6.0 } else { 20.0 } * s).min(width / 12.0).round();
    let tab_height = if tabs == 0 { 0.0 } else { 44.0 * s };
    let head = if compact { 34.0 * s } else { 36.0 * s + tab_height };
    let foot = if compact { 38.0 * s } else { 72.0 * s };
    let pitch = 60.0 * s;
    let rows = fields.div_ceil(columns);
    let group = tabs == 0 && fields > 4;
    let group_row = 4_usize.div_ceil(columns);
    let group_gap = if group { 24.0 * s } else { 0.0 };
    let desired = 2.0 * pad + head + rows as f32 * pitch + group_gap + foot;
    let height = desired.min(available.h);
    let frame = Rect::new(available.x + (available.w - width) * 0.5, available.y + (available.h - height) * 0.5, width, height).snap();
    let content = frame.inset(pad, pad);
    let title_w = if compact && tabs > 0 { content.w * 0.34 } else { content.w };
    let title = Rect::new(content.x, content.y, title_w, 28.0 * s).snap();
    let tab_x = if compact { title.right() + 3.0 * s } else { content.x };
    let tab_y = if compact { content.y } else { content.y + 36.0 * s };
    let tab_width = ((content.right() - tab_x) / tabs.max(1) as f32).min(210.0 * s);
    let tab_rects = (0..tabs).map(|i| Rect::new(tab_x + i as f32 * tab_width, tab_y, tab_width - 4.0 * s, 30.0 * s).snap()).collect();
    let save_w = (if compact { 67.0 } else { 95.0 } * s).min(content.w * 0.45);
    let save = Rect::new(content.right() - save_w, content.bottom() - 32.0 * s, save_w, 32.0 * s).snap();
    let cancel = Rect::new(save.x - save_w - 8.0 * s, save.y, save_w, save.h).snap();
    let message = if compact {
        Rect::new(content.x, save.y, (cancel.x - content.x - 6.0 * s).max(0.0), save.h)
    } else {
        Rect::new(content.x, save.y - 28.0 * s, content.w, 22.0 * s)
    }.snap();
    let top = content.y + head;
    let body = (message.y - top - 6.0 * s).max(0.0);
    let focus_row = focus.min(fields.saturating_sub(1)) / columns;
    let offset = |row: usize| row as f32 * pitch + if group && row >= group_row { group_gap } else { 0.0 };
    let end = rows as f32 * pitch + group_gap;
    let scroll = (offset(focus_row) + pitch - body).clamp(0.0, (end - body).max(0.0));
    let col_w = (content.w - (columns - 1) as f32 * gap) / columns as f32;
    let mut slots = Vec::new();
    for i in 0..fields {
        let y = top + offset(i / columns) - scroll;
        let h = pitch - 4.0 * s;
        if y >= top - 0.5 && y + h <= top + body + 0.5 {
            slots.push((i, Rect::new(content.x + (i % columns) as f32 * (col_w + gap), y, col_w, h).snap()));
        }
    }
    let group_y = top + offset(group_row) - group_gap - scroll;
    let group_defaults = (group && group_y >= top && group_y + group_gap <= top + body).then(|| Rect::new(content.x, group_y, content.w, group_gap).snap());
    let thumb = (end > body && body > 0.0).then(|| {
        let h = (body * body / end).max(12.0 * s).min(body);
        Rect::new(frame.right() - 8.0 * s, top + (body - h) * scroll / (end - body), 3.0 * s, h).snap()
    });
    EditorLayout { frame, title, tabs: tab_rects, fields: slots, group_defaults, scheme_preview: None, message, save, cancel, thumb }
}

pub(crate) fn profile_editor_layout(area: Rect, s: f32, editor: &ProfileEditor) -> EditorLayout {
    let fields = editor.tabs[editor.tab].fields.len();
    let colors = editor.colors_tab();
    let mut layout = editor_layout(area, s, editor.tabs.len(), fields + usize::from(colors), editor.focus);
    if colors && layout.fields.last().is_some_and(|(i, _)| *i == fields) {
        layout.scheme_preview = layout.fields.pop().map(|(_, rect)| rect);
    }
    layout
}
