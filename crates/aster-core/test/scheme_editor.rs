use super::*;
use crate::config::Config;

#[test]
fn editor_requires_a_unique_name_and_six_digit_hex_colors() {
    let mut editor = SchemeEditor::new(None, "   ".into(), Palette::DARK);
    assert_eq!(editor.validate(|_| false), Err(SchemeIssue::MissingName));
    editor.fields[0] = "Aster Dark".into();
    assert_eq!(editor.validate(|name| name == "Aster Dark"), Err(SchemeIssue::DuplicateName));
    editor.fields[0] = "Custom palette".into();
    for invalid in ["#abc", "#01234g", "#0€00", "112233"] {
        editor.fields[2] = invalid.into();
        assert_eq!(editor.validate(|_| false), Err(SchemeIssue::Color(2)));
        assert_eq!(editor.preview().background, Palette::DARK.background);
    }
}

#[test]
fn saved_colors_roundtrip_and_match_the_live_preview() {
    let mut editor = SchemeEditor::new(None, "  Custom palette  ".into(), Palette::DARK);
    editor.fields[2] = "#112233".into();
    editor.fields[4] = "#abcdef".into();
    editor.fields[5] = "#123456".into();
    editor.fields[6] = "#234567".into();
    editor.fields[22] = "#fedcba".into();
    let custom = editor.validate(|_| false).unwrap();
    assert_eq!(custom.name, "Custom palette");
    assert_eq!(custom.colors.len(), 16);
    assert_eq!(custom.colors[15], "#FEDCBA");
    let config = Config { color_schemes: vec![custom], ..Config::default() };
    let text = config.to_text();
    assert!(text.contains("[[color-schemes]]"));
    assert!(text.contains("cursor-text = \"#ABCDEF\""));
    assert_eq!(Config::parse(&text).unwrap().color_schemes[0].palette(), Some(editor.preview()));
}
