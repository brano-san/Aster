use crate::app::App;
use crate::input::Key;
use aster_gfx::render::Rect;
use aster_vt::TermMode;

pub const PREVIEW_LINES: usize = 8;
pub const BUTTONS: [&str; 3] = ["Paste", "Paste as one line", "Cancel"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasteConfirm {
    pub text: String,
    pub button: usize,
}

impl PasteConfirm {
    pub fn lines(&self) -> Vec<&str> {
        paste_lines(self.text.trim_end_matches(['\r', '\n', ' ', '\t'])).collect()
    }
}

pub fn line_count(text: &str) -> usize {
    paste_lines(text.trim_end_matches(['\r', '\n', ' ', '\t'])).count()
}

pub fn one_line(text: &str) -> String {
    paste_lines(text).map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

fn paste_lines(mut rest: &str) -> impl Iterator<Item = &str> {
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let Some(end) = rest.find(['\r', '\n']) else { return Some(std::mem::take(&mut rest)) };
        let (line, separator) = rest.split_at(end);
        rest = separator.strip_prefix("\r\n").unwrap_or(&separator[1..]);
        Some(line)
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct PasteGeometry {
    pub frame: Rect,
    pub title: Rect,
    pub preview: Rect,
    pub buttons: [Rect; 3],
}

pub fn geometry(content: Rect, s: f32, line_h: f32, confirm: &PasteConfirm) -> PasteGeometry {
    let shown = confirm.lines().len().min(PREVIEW_LINES) + usize::from(confirm.lines().len() > PREVIEW_LINES);
    let w = (560.0 * s).min(content.w - 32.0 * s).max(220.0 * s).round();
    let pad = (16.0 * s).round();
    let title_h = (28.0 * s).round();
    let preview_h = (shown as f32 * line_h + 16.0 * s).round();
    let button_h = (32.0 * s).round();
    let h = pad + title_h + (8.0 * s).round() + preview_h + pad + button_h + pad;
    let x = (content.x + (content.w - w) / 2.0).round();
    let y = (content.y + ((content.h - h) / 3.0).max(12.0 * s)).round();
    let frame = Rect::new(x, y, w, h);
    let title = Rect::new(x + pad, y + pad, w - 2.0 * pad, title_h);
    let preview = Rect::new(x + pad, title.bottom() + (8.0 * s).round(), w - 2.0 * pad, preview_h);
    let gap = (8.0 * s).round();
    let widths = [96.0, 150.0, 96.0].map(|bw: f32| (bw * s).round());
    let by = preview.bottom() + pad;
    let mut right = frame.right() - pad;
    let mut buttons = [Rect::default(); 3];
    for i in (0..3).rev() {
        buttons[i] = Rect::new(right - widths[i], by, widths[i], button_h);
        right -= widths[i] + gap;
    }
    PasteGeometry { frame, title, preview, buttons }
}

impl App {
    pub fn paste_confirm(&self) -> Option<&PasteConfirm> {
        self.paste_confirm.as_ref()
    }

    pub(crate) fn needs_paste_warning(&self, text: &str, mode: TermMode) -> bool {
        self.config.terminal.warn_on_multiline_paste && !mode.contains(TermMode::BRACKETED_PASTE) && line_count(text) > 1
    }

    pub(crate) fn ask_paste(&mut self, text: String) {
        self.paste_confirm = Some(PasteConfirm { text, button: 0 });
        self.dirty = true;
    }

    pub fn paste_geometry(&self) -> Option<PasteGeometry> {
        let line_h = self.metrics.map_or(17.0 * self.scale, |m| m.h);
        self.paste_confirm.as_ref().map(|c| geometry(self.chrome.content, self.ui(), line_h, c))
    }

    fn finish_paste(&mut self, button: usize) {
        let Some(c) = self.paste_confirm.take() else { return };
        self.dirty = true;
        match button {
            0 => self.paste_now(&c.text),
            1 => self.paste_now(&one_line(&c.text)),
            _ => {}
        }
    }

    pub(crate) fn paste_key(&mut self, key: Key) {
        let Some(c) = self.paste_confirm.as_mut() else { return };
        self.dirty = true;
        match key {
            Key::Escape => self.finish_paste(2),
            Key::Enter | Key::Space => {
                let b = c.button;
                self.finish_paste(b);
            }
            Key::Left | Key::Up => c.button = (c.button + 2) % 3,
            Key::Right | Key::Down | Key::Tab => c.button = (c.button + 1) % 3,
            _ => {}
        }
    }

    pub(crate) fn paste_click(&mut self, x: f32, y: f32) {
        let Some(geo) = self.paste_geometry() else { return };
        if let Some(i) = geo.buttons.iter().position(|b| b.contains(x, y)) {
            self.finish_paste(i);
        } else if !geo.frame.contains(x, y) {
            self.finish_paste(2);
        }
    }
}

#[cfg(test)]
#[path = "../test/paste.rs"]
mod tests;
