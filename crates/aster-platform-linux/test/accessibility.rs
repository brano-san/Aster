use super::text::{range_bounds, string_at, text_slice};
use super::*;
use aster_core::accessibility::TextLine;

#[test]
fn text_offsets_count_unicode_scalars_and_follow_wrapped_lines() {
    let text = Text {
        value: "a界e\u{301}\nnext".into(),
        selection: Some((1, 4)),
        caret: Some(4),
        lines: vec![
            TextLine {
                start: 0,
                end: 5,
                bounds: Rect::new(1., 2., 80., 14.),
                offsets: vec![0., 8., 24., 24., 32., 32.],
            },
            TextLine {
                start: 5,
                end: 9,
                bounds: Rect::new(1., 16., 80., 14.),
                offsets: vec![0., 8., 16., 24., 32.],
            },
        ],
    };
    assert_eq!(text_slice(&text.value, 1, 4), "界e\u{301}");
    assert_eq!(text_slice(&text.value, 5, -1), "next");
    assert_eq!(text_slice(&text.value, 15, -1), "");
    assert_eq!(string_at(&text, 3, 0), ("\u{301}".into(), 3, 4));
    assert_eq!(string_at(&text, 4, 3), ("a界e\u{301}\n".into(), 0, 5));
    assert_eq!(string_at(&text, 5, 3), ("next".into(), 5, 9));
    assert_eq!(range_bounds(&text, 1, 2), Some(Rect::new(9., 2., 16., 14.)));
    assert_eq!(range_bounds(&text, 2, 4), Some(Rect::new(25., 2., 8., 14.)));
    assert_eq!(range_bounds(&text, 2, 3), Some(Rect::new(25., 2., 8., 14.)));
    assert_eq!(range_bounds(&text, 3, 7), Some(Rect::new(1., 2., 32., 28.)));
}

#[test]
fn text_differences_keep_wide_characters_and_combining_marks_at_scalar_offsets() {
    let (offset, deleted, inserted) = text_change("a界旧e\u{301}", "a界新しいe\u{301}");
    assert_eq!((offset, deleted, inserted), (2, "旧", "新しい"));
}
