use super::*;
use crate::app::App;
use crate::config::Config;
use crate::keymap::Action;
use crate::menu::Entry;
use crate::settings::{Control, SettingsPage, PROFILES, SETTINGS, SHORTCUTS};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const UNTRANSLATED: [&str; 2] = ["SSH", "Mica"];

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("source folder") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn source_texts() -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    sources(&manifest.join("src"), &mut files);
    sources(&manifest.join("../aster-serial/src"), &mut files);
    files.push(manifest.join("../aster-proto/src/serial.rs"));
    files.into_iter().map(|p| {
        let text = std::fs::read_to_string(&p).expect("readable source");
        (p, text)
    }).collect()
}

fn read_literal(chars: &[char], start: usize) -> (String, usize) {
    let mut out = String::new();
    let mut i = start;
    while i < chars.len() {
        match chars[i] {
            '"' => return (out, i + 1),
            '\\' => {
                let next = chars.get(i + 1).copied().unwrap_or('\\');
                i += 2;
                match next {
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    '0' => out.push('\0'),
                    'x' => {
                        let hex: String = chars[i..i + 2].iter().collect();
                        out.push(u8::from_str_radix(&hex, 16).map(char::from).unwrap_or('?'));
                        i += 2;
                    }
                    'u' => {
                        let close = (i..chars.len()).find(|&k| chars[k] == '}').unwrap_or(i);
                        let hex: String = chars[i + 1..close].iter().collect();
                        out.push(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).unwrap_or('?'));
                        i = close + 1;
                    }
                    '\n' => {
                        while i < chars.len() && chars[i].is_whitespace() {
                            i += 1;
                        }
                    }
                    c => out.push(c),
                }
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    (out, i)
}

fn ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn literals(text: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let before = if i > 0 { chars[i - 1] } else { ' ' };
        if c == 'r' && !ident(before) && matches!(chars.get(i + 1), Some('"' | '#')) {
            let hashes = chars[i + 1..].iter().take_while(|&&h| h == '#').count();
            if chars.get(i + 1 + hashes) == Some(&'"') {
                let body = i + 2 + hashes;
                let mut k = body;
                while k < chars.len() && !(chars[k] == '"' && chars[k + 1..].iter().take(hashes).filter(|&&h| h == '#').count() == hashes) {
                    k += 1;
                }
                out.push((String::new(), chars[body..k.min(chars.len())].iter().collect()));
                i = k + 1 + hashes;
                continue;
            }
        }
        if c == '\'' {
            if chars.get(i + 1) == Some(&'\\') {
                i = (i + 2..chars.len()).find(|&k| chars[k] == '\'').map_or(chars.len(), |k| k + 1);
                continue;
            }
            if chars.get(i + 2) == Some(&'\'') {
                i += 3;
                continue;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            let head: String = chars[i.saturating_sub(4)..i].iter().collect();
            let call = if head.ends_with("tf(") && (head.len() < 4 || !ident(chars[i - 4])) {
                "tf"
            } else if head.ends_with("t(") && (i < 3 || !ident(chars[i - 3])) {
                "t"
            } else {
                ""
            };
            let (lit, next) = read_literal(&chars, i + 1);
            out.push((call.to_owned(), lit));
            i = next;
            continue;
        }
        i += 1;
    }
    out
}

fn placeholders(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let tail = &rest[open + 1..];
        match tail.find('}') {
            Some(close) => {
                out.insert(tail[..close].to_owned());
                rest = &tail[close + 1..];
            }
            None => break,
        }
    }
    out
}

fn app() -> App {
    App::new(Config::default(), Vec::new(), true, None, Arc::new(|| {}))
}

#[test]
fn the_catalog_is_a_flat_table_of_strings() {
    let table: toml::Table = toml::from_str(RU_CATALOG).expect("ru.toml parses");
    assert!(table.values().all(|v| v.is_str()));
    assert_eq!(russian().len(), table.len());
    assert!(russian().len() > 150);
}

#[test]
fn every_literal_passed_to_t_or_tf_has_a_russian_entry() {
    let mut checked = 0;
    for (path, text) in source_texts() {
        for (call, lit) in literals(&text) {
            if !call.is_empty() {
                checked += 1;
                assert!(russian().contains_key(lit.as_str()), "{}: {call}({lit:?}) has no entry in ru.toml", path.display());
            }
        }
    }
    assert!(checked > 50, "the scanner found only {checked} calls");
}

#[test]
fn every_catalog_entry_is_used_in_the_sources() {
    let used: BTreeSet<String> = source_texts().iter().flat_map(|(_, text)| literals(text)).map(|(_, lit)| lit).collect();
    let stale: Vec<_> = russian().keys().filter(|k| !used.contains(**k)).collect();
    assert!(stale.is_empty(), "stale entries in ru.toml: {stale:?}");
}

#[test]
fn translations_keep_the_placeholders_of_their_source() {
    for (source, russian) in russian() {
        assert_eq!(placeholders(source), placeholders(russian), "{source:?} -> {russian:?}");
        assert!(!russian.trim().is_empty(), "{source:?} has an empty translation");
    }
}

#[test]
fn the_scanner_handles_escapes_and_char_literals() {
    let found = literals(r#"let q = '"'; t("say \"hi\""); get("x"); tf("a {n}\u{b7}", &[]); r"raw""#);
    assert_eq!(found[0], ("t".to_owned(), "say \"hi\"".to_owned()));
    assert_eq!(found[1], (String::new(), "x".to_owned()));
    assert_eq!(found[2], ("tf".to_owned(), "a {n}\u{b7}".to_owned()));
    assert_eq!(found[3].1, "raw");
}

#[test]
fn every_label_drawn_from_constants_has_a_russian_entry() {
    let mut texts: Vec<String> = vec![SHORTCUTS.into(), PROFILES.into()];
    texts.extend(crate::paste::BUTTONS.iter().map(|b| b.to_string()));
    for s in SETTINGS {
        texts.extend([s.section, s.label, s.hint].map(str::to_owned));
        if let Control::Choice(options) | Control::Segments(options) = s.control {
            texts.extend(options.iter().map(|(_, l)| l.to_string()));
        }
    }
    texts.extend(Action::all().filter(|a| !matches!(a, Action::Tab(_))).map(|a| a.source_label()));
    let mut a = app();
    a.open_tab(crate::profile::Profile::new("local:test", "Shell", "shell", &[]));
    a.open_tab_menu(0, 10.0, 10.0);
    let mut menus = a.menu.take().expect("tab menu").entries;
    a.run(Action::PinTab);
    a.open_tab_menu(0, 10.0, 10.0);
    menus.extend(a.menu.take().expect("pinned tab menu").entries);
    a.open_terminal_menu(10.0, 10.0);
    menus.extend(a.menu.take().expect("terminal menu").entries);
    texts.extend(menus.into_iter().filter_map(|e| if let Entry::Item { label, .. } = e { Some(label.to_owned()) } else { None }));
    let missing: Vec<_> = texts.iter().filter(|t| !UNTRANSLATED.contains(&t.as_str()) && !russian().contains_key(t.as_str())).collect();
    assert!(missing.is_empty(), "missing translations: {missing:?}");
}

#[test]
fn t_follows_the_current_language_and_tf_fills_placeholders() {
    set_language("en");
    assert_eq!(language(), "en");
    assert_eq!(t("Settings"), "Settings");
    assert_eq!(t("Not in the catalog"), "Not in the catalog");
    assert_eq!(tf("Files on {host}", &[("host", "prod")]), "Files on prod");
    set_language("ru");
    assert_eq!(language(), "ru");
    assert_eq!(t("Settings"), "Настройки");
    assert_eq!(t("Not in the catalog"), "Not in the catalog");
    assert_eq!(tf("Files on {host}", &[("host", "prod")]), "Файлы на prod");
    assert_eq!(tf("{shortcut} was taken from {actions}", &[("shortcut", "Ctrl-T"), ("actions", "{actions}")]), "Ctrl-T снято с: {actions}");
    assert_eq!(Action::Tab(3).label(), "Перейти на вкладку 3");
    assert_eq!(Action::Tab(3).source_label(), "Switch to tab 3");
    set_language("de");
    assert_eq!(language(), "en");
}

#[test]
fn the_language_setting_resolves_against_the_system_language() {
    assert_eq!(resolve("auto", "ru-RU"), "ru");
    assert_eq!(resolve("auto", "ru_RU.UTF-8"), "ru");
    assert_eq!(resolve("auto", "en-US"), "en");
    assert_eq!(resolve("auto", "C"), "en");
    assert_eq!(resolve("en", "ru-RU"), "en");
    assert_eq!(resolve("ru", "en-GB"), "ru");
    assert_eq!(resolve("", "ru"), "ru");
}

#[test]
fn the_app_follows_the_system_language_unless_the_config_names_one() {
    let mut a = app();
    assert_eq!(language(), "en");
    a.set_system_language("ru-RU");
    assert_eq!(language(), "ru");
    assert_eq!(a.system_language(), "ru-RU");
    a.set_system_language("en-US");
    assert_eq!(t("New tab"), "New tab");
    let mut config = a.config().clone();
    config.appearance.language = "ru".into();
    a.set_config(config.clone());
    assert_eq!(t("New tab"), "Новая вкладка");
    a.set_system_language("ru-RU");
    config.appearance.language = "en".into();
    a.set_config(config);
    assert_eq!(language(), "en", "an explicit language overrides the system");
    set_language("en");
}

#[test]
fn the_process_exit_notice_is_translated() {
    set_language("ru");
    let text = tf("[Process exited with code {code}. Press any key to close.]", &[("code", "2")]);
    assert!(text.starts_with("[Процесс завершился с кодом 2."));
    set_language("en");
}

#[test]
fn settings_search_matches_english_and_russian_text_while_russian_is_active() {
    set_language("ru");
    let mut page = SettingsPage::new();
    page.query = "язык".into();
    page.refilter();
    let keys: Vec<_> = page.shown.iter().filter_map(|&i| SETTINGS.get(i)).map(|s| s.key).collect();
    assert_eq!(keys, ["appearance.language"]);
    page.query = "language".into();
    page.refilter();
    assert!(page.shown.iter().filter_map(|&i| SETTINGS.get(i)).any(|s| s.key == "appearance.language"));
    page.query = "новая вкладка".into();
    page.refilter();
    assert!(page.shown.iter().any(|&i| crate::settings::shortcut(i) == Some(Action::NewTab)));
    set_language("en");
}
