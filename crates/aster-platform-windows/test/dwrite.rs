use super::*;

#[test]
fn terminal_font_weights_invalidate_existing_directwrite_formats() {
    let mut text = DWriteText::new().unwrap();
    let regular = FontStyle::new(FontFamily::Mono, 16.0, false);
    let bold = FontStyle::new(FontFamily::Mono, 16.0, true);
    let ui = FontStyle::new(FontFamily::Ui, 16.0, true);
    assert_eq!(unsafe { text.format(regular).unwrap().GetFontWeight() }, DWRITE_FONT_WEIGHT_NORMAL);
    assert!(text.set_mono_font("Consolas", [300, 800]));
    assert_eq!(unsafe { text.format(regular).unwrap().GetFontWeight() }, DWRITE_FONT_WEIGHT(300));
    assert_eq!(unsafe { text.format(bold).unwrap().GetFontWeight() }, DWRITE_FONT_WEIGHT(800));
    assert_eq!(unsafe { text.format(ui).unwrap().GetFontWeight() }, DWRITE_FONT_WEIGHT_SEMI_BOLD);
}

#[test]
fn system_font_families_filter_proportional_faces() {
    let all = font_families(false).unwrap();
    let monospace = font_families(true).unwrap();
    assert!(all.iter().any(|name| name == "Arial"));
    assert!(monospace.iter().any(|name| name == "Consolas"));
    assert!(monospace.iter().all(|name| name != "Arial" && all.contains(name)));
}

fn family_for(text: &mut DWriteText, style: FontStyle, s: &str) -> String {
    let mut shaped = ShapedText::default();
    text.shape(s, style, &mut shaped);
    assert_ne!(shaped.glyphs[0].id, 0, "missing glyph for {s}");
    let face = &text.faces[shaped.glyphs[0].font as usize];
    let font = unsafe { text.collection.GetFontFromFontFace(face).unwrap() };
    DWriteText::family_name(&unsafe { font.GetFontFamily().unwrap() }, w!("en-us")).unwrap()
}

#[test]
fn preferred_fallback_only_supplies_missing_mono_glyphs() {
    let mut text = DWriteText::new().unwrap();
    let style = FontStyle::new(FontFamily::Mono, 16.0, false);
    assert!(text.set_mono_family("Consolas"));
    let original = family_for(&mut text, style, "ש");
    let symbol_face = family_for(&mut text, style, "⚜");
    let preferred = if original == "Times New Roman" { "Arial" } else { "Times New Roman" };
    let ui = FontStyle::new(FontFamily::Ui, 16.0, false);
    let ui_face = family_for(&mut text, ui, "ש");
    assert!(text.set_fallback_family(preferred));
    assert_eq!(family_for(&mut text, style, "ש"), preferred);
    assert_eq!(family_for(&mut text, style, "Ж"), "Consolas");
    assert_eq!(family_for(&mut text, style, "⚜"), symbol_face);
    assert_eq!(family_for(&mut text, ui, "ש"), ui_face);
    assert!(text.set_fallback_family(""));
    assert_eq!(family_for(&mut text, style, "ש"), original);
}

#[test]
fn mono_ligatures_follow_style_and_expose_byte_clusters() {
    let mut text = DWriteText::new().unwrap();
    assert!(text.set_mono_family("Calibri"));
    let mut style = FontStyle::new(FontFamily::Mono, 18.0, false);
    let mut shaped = ShapedText::default();
    text.shape("éfi", style, &mut shaped);
    assert_eq!(shaped.glyphs.iter().map(|g| g.cluster).collect::<Vec<_>>(), [0, 2, 3]);
    style.ligatures = true;
    text.shape("éfi", style, &mut shaped);
    assert_eq!(shaped.glyphs.iter().map(|g| g.cluster).collect::<Vec<_>>(), [0, 2]);
    assert!(shaped.glyphs[1].cluster_x > shaped.glyphs[0].cluster_x);
}

#[test]
fn missing_symbol_font_glyph_uses_requested_fallback() {
    let mut text = DWriteText::new().unwrap();
    assert!(text.set_mono_family("Segoe MDL2 Assets"));
    assert!(text.set_fallback_family("Times New Roman"));
    let style = FontStyle::new(FontFamily::Mono, 16.0, false);
    assert_eq!(family_for(&mut text, style, "Ж"), "Times New Roman");
    assert_eq!(family_for(&mut text, style, "\u{e700}"), "Segoe MDL2 Assets");
    assert!(text.set_fallback_family("Calibri"));
    let mut shaped = ShapedText::default();
    text.shape("éfi", FontStyle { ligatures: true, ..style }, &mut shaped);
    assert_eq!(shaped.glyphs.iter().map(|g| g.cluster).collect::<Vec<_>>(), [0, 2]);
    assert!(text.set_fallback_family("Segoe MDL2 Assets"));
    assert_ne!(family_for(&mut text, style, "Ж"), "Segoe MDL2 Assets");
}
