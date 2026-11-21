use super::*;

#[test]
fn run_value_quotes_the_executable_with_optional_hidden_flag() {
    let exe = Path::new(r"C:\Portable Apps\Aster\aster.exe");
    let text = |args| {
        let raw = run_value(exe, args);
        assert_eq!(raw.last(), Some(&0));
        String::from_utf16(&raw[..raw.len() - 1]).unwrap()
    };
    assert_eq!(text(&[]), r#""C:\Portable Apps\Aster\aster.exe""#);
    assert_eq!(text(&["--hidden"]), r#""C:\Portable Apps\Aster\aster.exe" --hidden"#);
}
