use super::*;

#[test]
fn preferred_face_supplies_missing_cells_only_for_mono() {
    let mut text = LinuxText::new();
    let sans = text.load(fonts::SANS, 0, false).unwrap();
    let icons = text.icons;
    text.adopted = true;
    text.fallback.clear();
    text.ui = [icons; 2];
    text.mono = [icons; 2];
    text.preferred = Some([sans; 2]);
    assert!(!text.faces[icons as usize].covers('A'));
    let icon = (0xe000..=0xf8ff).filter_map(char::from_u32).find(|&c| text.faces[icons as usize].covers(c)).unwrap();
    let mut shaped = ShapedText::default();
    let mono = FontStyle::new(FontFamily::Mono, 16.0, false);
    text.shape(&format!("A{icon}"), mono, &mut shaped);
    assert_eq!(shaped.glyphs.iter().map(|g| g.font).collect::<Vec<_>>(), [sans, icons]);
    text.shape("A", FontStyle::new(FontFamily::Ui, 16.0, false), &mut shaped);
    assert_eq!(shaped.glyphs[0].font, icons);
}

#[test]
fn mono_shape_plans_distinguish_enabled_and_disabled_features() {
    let mut text = LinuxText::new();
    let sans = text.load(fonts::SANS, 0, false).unwrap();
    text.mono = [sans; 2];
    let mut style = FontStyle::new(FontFamily::Mono, 16.0, false);
    let mut shaped = ShapedText::default();
    for enabled in [false, true, false] {
        style.ligatures = enabled;
        text.shape("->", style, &mut shaped);
        assert!(!shaped.glyphs.is_empty());
    }
    assert_eq!(text.faces[sans as usize].plans.len(), 2);
}
