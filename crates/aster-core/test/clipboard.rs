use super::*;
use aster_vt::{SelectionType, Side, Size};
use std::sync::Arc;

#[test]
fn block_html_resolves_palette_overrides_and_preserves_selected_columns() {
    let mut terminal = Terminal::new(Size::new(6, 2), Default::default(), Arc::new(|_| {}));
    terminal.feed(b"\x1b]4;1;rgb:12/34/56\x07\x1b[31mxAB  z\r\nxCD  z");
    terminal.start_selection(SelectionType::Block, terminal.viewport_point(0, 1), Side::Left);
    terminal.update_selection(terminal.viewport_point(1, 3), Side::Right);
    assert_eq!(selection_html(&terminal, &Palette::LIGHT), "<pre><span style=\"color:#123456;background-color:#ffffff;font-weight:normal\">AB\nCD</span></pre>");
}
