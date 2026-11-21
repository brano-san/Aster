use super::*;

#[test]
fn xml_edit_ignores_markup_inside_other_actions_and_comments() {
    let original = format!("\u{feff}<?xml version=\"1.0\"?>\r\n<actions>\r\n<!-- {} </actions> -->\r\n<action><name><![CDATA[</action>]]></name><unique-id>user</unique-id></action>\r\n</actions>\r\n", action("/not/ours", "Open", false));
    let installed = edit(&original, Some(("/opt/Aster/aster", "Open"))).unwrap();
    let refreshed = edit(&installed, Some(("/new/path/aster", "Open"))).unwrap();
    assert!(!refreshed.contains("/opt/Aster/aster"));
    assert!(refreshed.contains("/new/path/aster"));
    assert_eq!(edit(&refreshed, None).unwrap(), original);
}

#[test]
fn xml_edit_restores_an_empty_root_and_refuses_unbalanced_input() {
    let original = "<actions />\r\n";
    let installed = edit(original, Some(("/aster", "Open"))).unwrap();
    assert_eq!(edit(&installed, None).unwrap(), original);
    assert!(edit("<actions><action></actions>", Some(("/aster", "Open"))).is_err());
}
