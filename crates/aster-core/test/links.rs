use super::*;

fn targets(text: &str) -> Vec<String> {
    find_links(text).into_iter().map(|l| l.target).collect()
}

#[test]
fn urls_stop_at_spaces_quotes_and_trailing_punctuation() {
    assert_eq!(targets("see https://example.com/a?b=1, then"), ["https://example.com/a?b=1"]);
    assert_eq!(targets("(https://en.wikipedia.org/wiki/Rust_(language))."), ["https://en.wikipedia.org/wiki/Rust_(language)"]);
    assert_eq!(targets("\"http://x.io/\""), ["http://x.io/"]);
    assert_eq!(targets("HTTPS://UP.CASE"), ["HTTPS://UP.CASE"]);
    assert!(targets("https:// nothing").is_empty());
}

#[test]
fn windows_and_unix_paths_are_found_at_word_starts() {
    assert_eq!(targets(r"error in C:\src\main.rs:12: oops"), [r"C:\src\main.rs:12"]);
    assert_eq!(targets("open D:/work/file.txt."), ["D:/work/file.txt"]);
    assert_eq!(targets("at /home/me/notes.md and ~/x"), ["/home/me/notes.md", "~/x"]);
    assert!(targets("a/b/c and /x and 3/4").is_empty());
}

#[test]
fn link_positions_are_char_indices() {
    let l = &find_links("日本 https://a.b/c")[0];
    assert_eq!((l.start, l.end), (3, 16));
}
