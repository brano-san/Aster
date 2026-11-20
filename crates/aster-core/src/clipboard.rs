use aster_vt::{Flags, Palette, Terminal};
use std::fmt::Write;

pub(crate) fn selection_html(terminal: &Terminal, palette: &Palette) -> String {
    let mut html = String::from("<pre>");
    let mut previous = None;
    terminal.selected_cells(|cell| {
        let Some(cell) = cell else {
            html.push('\n');
            return;
        };
        let (fg, bg) = palette.cell(cell, terminal.colors());
        let bold = cell.flags.contains(Flags::BOLD);
        if previous != Some((fg, bg, bold)) {
            if previous.is_some() {
                html.push_str("</span>");
            }
            let weight = if bold { "bold" } else { "normal" };
            write!(html, "<span style=\"color:#{:02x}{:02x}{:02x};background-color:#{:02x}{:02x}{:02x};font-weight:{weight}\">", fg.r, fg.g, fg.b, bg.r, bg.g, bg.b).unwrap();
            previous = Some((fg, bg, bold));
        }
        for c in std::iter::once(cell.c).chain(cell.zerowidth().into_iter().flatten().copied()) {
            match c {
                '&' => html.push_str("&amp;"),
                '<' => html.push_str("&lt;"),
                '>' => html.push_str("&gt;"),
                '\t' => html.push(' '),
                _ => html.push(c),
            }
        }
    });
    if previous.is_some() {
        html.push_str("</span>");
    }
    html.push_str("</pre>");
    html
}

#[cfg(test)]
#[path = "../test/clipboard.rs"]
mod tests;
