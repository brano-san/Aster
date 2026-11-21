use super::*;

fn query(path: &str) -> String {
    let out = std::process::Command::new("reg").args(["query", &format!(r"HKCU\{path}\command"), "/ve"]).output().expect("reg");
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
#[ignore = "writes to the current user's registry"]
fn the_menu_is_added_and_removed() {
    assert!(set("Open in Aster", true));
    assert!(set_paste("Paste path", true));
    let exe = std::env::current_exe().unwrap().to_string_lossy().into_owned();
    for (path, arg) in PLACES {
        let value = query(path);
        assert!(value.contains(&format!("\"{exe}\" open \"{arg}\"")), "{value}");
    }
    for (path, arg) in PASTE_PLACES {
        let value = query(path);
        assert!(value.contains(&format!("\"{exe}\" paste-path \"{arg}\"")), "{value}");
    }
    assert!(set("Open in Aster", false));
    assert!(set_paste("Paste path", false));
    assert!(PLACES.iter().chain(&PASTE_PLACES).all(|(path, _)| query(path).is_empty()));
}
