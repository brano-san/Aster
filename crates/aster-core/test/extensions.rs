use super::*;
use crate::config::Config;
use crate::keymap::{Action as KeyAction, Chord, Outcome};

fn app() -> App {
    let mut app = App::new(Config::default(), vec![crate::profile::Profile::new("local:shell", "Shell", "shell", &[])], true, None, Arc::new(|| {}));
    app.open_default_tab();
    app.take_effects();
    let manifest = Manifest::parse(r#"
id = "hello"
name = "Hello"
version = "1"
command = "hello.exe"
[[commands]]
id = "hello.greet"
title = "Greet"
shortcut = "Ctrl+Alt+G"
menu = "terminal"
[[commands]]
id = "hello.close"
title = "Say goodbye"
menu = "tab"
"#).unwrap();
    app.set_extensions(Arc::new(Snapshot {
        installed: vec![Installed { manifest, directory: "hello".into(), status: Status::Running, schemes: Vec::new(), profiles: Vec::new() }],
        errors: Vec::new(),
    }));
    app
}

#[test]
fn palette_lists_one_command_and_sends_active_pane_context() {
    let mut app = app();
    let picks = app.command_picks();
    let greet: Vec<_> = picks.iter().filter(|pick| pick.label == "Hello: Greet").collect();
    assert_eq!(greet.len(), 1);
    let action = KeyAction::from_id(&greet[0].value).unwrap();
    app.run(action);
    assert!(app.take_effects().iter().any(|effect| matches!(effect,
        Effect::Extension(Request::Run { extension, command, title, .. }) if extension == "hello" && command == "hello.greet" && title == "Shell")));
}

#[test]
fn contributions_follow_built_in_menu_items_with_one_separator() {
    let app = app();
    for (entries, id) in [(app.terminal_menu_entries(), "hello.greet"), (app.tab_menu_entries(0), "hello.close")] {
        assert_eq!(entries[entries.len() - 2], crate::menu::Entry::Separator);
        assert_eq!(entries.last().and_then(|entry| match entry {
            crate::menu::Entry::Item { action, .. } => Some(action.id()),
            _ => None,
        }).as_deref(), Some(format!("extension.hello:{id}").as_str()));
    }
}

#[test]
fn shortcuts_accept_manifest_defaults_and_user_rebinding() {
    let mut app = app();
    let initial = Chord::parse("Ctrl+Alt+G").unwrap();
    let custom = Chord::parse("Ctrl+Alt+J").unwrap();
    let action = KeyAction::from_id("extension.hello:hello.greet").unwrap();
    assert_eq!(app.keymap.press(initial), Outcome::Action(action.clone()));
    let mut config = app.config().clone();
    config.hotkeys.insert(action.id(), vec![custom.to_string()]);
    app.set_config(config);
    assert_eq!(app.keymap.press(initial), Outcome::None);
    assert_eq!(app.keymap.press(custom), Outcome::Action(action));
}

#[test]
fn contributions_are_withdrawn_on_disable_and_schemes_have_a_source() {
    let mut app = app();
    let mut snapshot = (*app.extensions).clone();
    snapshot.installed[0].schemes.push(toml::from_str(include_str!("../../aster-ext/test/fixtures/hello/schemes/night.toml")).unwrap());
    snapshot.installed[0].profiles.push(Profile { name: "Local tools".into(), command: "shell".into(), args: vec!["--login".into()], cwd: None, icon: None });
    app.set_extensions(Arc::new(snapshot));
    assert!(app.profile_picks().iter().any(|pick| pick.label == "Local tools" && pick.group == "Hello"));
    assert!(app.scheme_rows().iter().any(|row| row.name == "Hello Night" && row.source.as_deref() == Some("Hello") && !row.custom));
    let mut config = app.config().clone();
    config.extensions.disabled.push("hello".into());
    app.set_config(config);
    assert!(app.profile_picks().iter().all(|pick| pick.group != "Hello"));
    assert!(!app.scheme_names().contains(&"Hello Night".into()));
    assert!(app.command_picks().iter().all(|pick| pick.label != "Hello: Greet"));
    assert_eq!(app.keymap.press(Chord::parse("Ctrl+Alt+G").unwrap()), Outcome::None);
}

#[test]
fn extensions_page_searches_catalog_and_requests_the_indexed_version() {
    use crate::input::{Key, Mods};
    let mut app = app();
    app.set_viewport(1100, 700, 1.0);
    app.chrome = app.tab_chrome();
    app.run(KeyAction::Settings);
    let index = crate::settings::PAGES.iter().position(|page| *page == crate::settings::EXTENSIONS).unwrap();
    app.settings.as_mut().unwrap().show_page(index);
    app.settings_key(Key::Right, Mods::NONE);
    let mut config = app.config().clone();
    config.extensions.index_url = "https://example.test/extensions.json".into();
    app.set_config(config);
    assert!(app.take_effects().iter().any(|effect| matches!(effect,
        Effect::Extension(Request::Catalog(url)) if url == "https://example.test/extensions.json")));
    app.extension_reply(Reply::Catalog { url: "https://example.test/extensions.json".into(), result: Ok(vec![
        CatalogEntry { id: "hello".into(), name: "Hello".into(), version: "2".into(), author: "Sample".into(), description: "Greeting".into(), url: "https://example.test/hello.zip".into() },
        CatalogEntry { id: "tools".into(), name: "Tools".into(), version: "1".into(), author: "Sample".into(), description: "Utilities".into(), url: "https://example.test/tools.zip".into() },
    ]) });
    app.settings_text("greeting");
    let page = app.settings.as_ref().unwrap();
    let rows = app.extension_rows(page);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "Hello");
    let area = app.settings_geometry().unwrap().list;
    let button = page.extensions.layout(area, app.ui(), &rows).rows[0].1[0];
    app.settings_click(button.x + 1.0, button.y + 1.0);
    assert!(app.take_effects().iter().any(|effect| matches!(effect,
        Effect::Extension(Request::InstallCatalog(entry)) if entry.id == "hello" && entry.version == "2")));
}

#[test]
fn extensions_page_requires_confirmation_to_remove_an_extension() {
    let mut app = app();
    app.set_viewport(1100, 700, 1.0);
    app.chrome = app.tab_chrome();
    app.run(KeyAction::Settings);
    let index = crate::settings::PAGES.iter().position(|page| *page == crate::settings::EXTENSIONS).unwrap();
    app.settings.as_mut().unwrap().show_page(index);
    let area = app.settings_geometry().unwrap().list;
    let button = |app: &App, index: usize| {
        let page = app.settings.as_ref().unwrap();
        page.extensions.layout(area, app.ui(), &app.extension_rows(page)).rows[0].1[index]
    };
    let remove = button(&app, 1);
    app.settings_click(remove.x + 1.0, remove.y + 1.0);
    assert!(!app.take_effects().iter().any(|effect| matches!(effect, Effect::Extension(Request::Remove(_)))));
    let confirm = button(&app, 0);
    app.settings_click(confirm.x + 1.0, confirm.y + 1.0);
    assert!(app.take_effects().iter().any(|effect| matches!(effect, Effect::Extension(Request::Remove(id)) if id == "hello")));
}
