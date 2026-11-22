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

fn unpack(color: Color) -> Rgb {
    let [r, g, b, _] = color.0.to_le_bytes();
    Rgb { r, g, b }
}

#[test]
fn derived_ui_ink_meets_minimum_contrast_on_light_dark_and_selected_surfaces() {
    for level in [28, 110, 128, 230] {
        let mut palette = Palette::DARK;
        palette.background = gray(level);
        palette.foreground = gray(level.saturating_add(2));
        palette.ansi[4] = gray(level.saturating_sub(2));
        let theme = Theme::from_palette(&palette);
        assert_eq!(theme.background, color(palette.background));
        assert!(contrast_ratio(unpack(theme.on_accent), unpack(theme.accent)) >= 4.5);
        for surface in [theme.surface, theme.chrome, theme.overlay, theme.tab_hover] {
            let background = unpack(surface);
            let selected = mix(background, unpack(theme.accent), 80.0 / 255.0);
            for ink in [theme.text, theme.text_dim, theme.accent] {
                assert!(contrast_ratio(unpack(ink), background) >= 4.5, "{level}: {ink:?} on {surface:?}");
                assert!(contrast_ratio(unpack(ink), selected) >= 4.5, "{level}: {ink:?} on selected {surface:?}");
            }
        }
    }
}

#[test]
fn fixed_ui_palette_preserves_the_terminal_colors() {
    let mut palette = Palette::DARK;
    palette.background = gray(85);
    palette.foreground = gray(160);
    let fixed = Theme::fixed(&palette, false);
    assert_eq!(fixed.background, color(palette.background));
    assert_eq!(fixed.foreground, color(palette.foreground));
    assert_eq!(fixed.surface, color(Palette::LIGHT.background));
    assert_eq!(fixed.accent, color(Palette::LIGHT.ansi[4]));
    assert!(!fixed.dark);
}
