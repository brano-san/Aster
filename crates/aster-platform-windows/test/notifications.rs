use super::*;

#[test]
fn notification_text_cannot_change_the_xml_structure() {
    assert_eq!(xml_text("<tag>&\0"), "&lt;tag&gt;&amp; ");
}