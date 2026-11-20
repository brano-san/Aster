use super::*;

#[test]
fn clipboard_html_offsets_delimit_utf8_document_and_fragment() {
    let fragment = "<pre>界🙂&amp;</pre>";
    let data = clipboard_html(fragment);
    let offset = |field: &str| data.lines().find_map(|line| line.strip_prefix(field)).unwrap().parse::<usize>().unwrap();
    assert!(data[offset("StartHTML:")..offset("EndHTML:")].starts_with("<html>"));
    assert_eq!(&data[offset("StartFragment:")..offset("EndFragment:")], fragment);
    assert_eq!(&data[offset("EndHTML:")..], "\0");
}
