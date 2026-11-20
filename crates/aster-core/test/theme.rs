use super::*;

fn gray(value: u8) -> Rgb {
    Rgb { r: value, g: value, b: value }
}

#[test]
fn wcag_reference_colors() {
    assert!((contrast_ratio(gray(0), gray(255)) - 21.0).abs() < 0.001);
    assert!((luminance(Rgb { r: 255, g: 0, b: 0 }) - 0.2126).abs() < 0.0001);
    assert!((contrast_ratio(gray(119), gray(255)) - 4.478).abs() < 0.001);
}

#[test]
fn preserves_foreground_when_off_or_already_legible() {
    assert_eq!(ensure_contrast(gray(32), gray(32), 1.0), gray(32));
    assert_eq!(ensure_contrast(gray(221), gray(0), 7.0), gray(221));
}

#[test]
fn adjusts_to_nearest_passing_shade_in_both_directions() {
    for (bg, step) in [(gray(30), -1i16), (gray(230), 1)] {
        let fg = ensure_contrast(bg, bg, 7.0);
        assert!(contrast_ratio(fg, bg) >= 7.0);
        assert!(contrast_ratio(gray((i16::from(fg.r) + step) as u8), bg) < 7.0);
    }
}

#[test]
fn unreachable_contrast_uses_the_better_endpoint() {
    assert_eq!(ensure_contrast(gray(126), gray(128), 21.0), gray(0));
    assert_eq!(ensure_contrast(gray(66), gray(64), 21.0), gray(255));
}
