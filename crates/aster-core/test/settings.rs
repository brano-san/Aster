use super::*;
use crate::app::Effect;
use crate::input::Event;
use crate::keymap::Action;
use crate::profile::Profile;
use std::sync::Arc;

fn app() -> App {
    let mut alpha = Profile::new("local:a", "Alpha", "a.exe", &[]);
    alpha.env.insert("MARK".into(), "1".into());
    let profiles = vec![alpha, Profile::new("local:b", "Beta", "b.exe", &[])];
    let mut a = App::new(Config::default(), profiles, true, None, Arc::new(|| {}));
    a.set_viewport(1000, 700, 1.0);
    a
}

fn key(a: &mut App, k: Key) {
    a.on_event(Event::Key { key: k, mods: Mods::NONE, pressed: true });
}

fn typed(a: &mut App, text: &str) {
    for c in text.chars() {
        a.on_event(Event::Char(c));
    }
}

fn saved(a: &mut App) -> Option<Config> {
    a.take_effects().into_iter().filter_map(|e| if let Effect::SaveConfig(t) = e { Config::parse(&t).ok() } else { None }).next_back()
}

#[test]
fn every_setting_points_at_a_config_value_of_the_matching_type() {
    let config = Config::default();
    for s in SETTINGS.iter().filter(|s| !matches!(s.control, Control::Run { .. } | Control::ReadOnly(_))) {
        let value = get(&config, s.key).unwrap_or_else(|| panic!("{} is missing", s.key));
        let fits = match s.control {
            Control::Toggle => value.is_bool(),
            Control::Number { .. } | Control::Percent { .. } => value.is_integer() || value.is_float(),
            Control::Choice(_) | Control::Schemes | Control::Profiles | Control::Text | Control::Color | Control::Font { .. } => value.is_str(),
            Control::StringList => value.is_array(),
            Control::Run { .. } | Control::ReadOnly(_) => unreachable!(),
        };
        assert!(fits, "{} holds {value:?}", s.key);
        if let Control::Choice(options) = s.control {
            assert!(options.iter().any(|(v, _)| Some(*v) == value.as_str()), "{} default is not an option", s.key);
        }
    }
}

#[test]
fn window_and_appearance_list_their_own_settings() {
    let keys = |name: &str| SETTINGS.iter().filter(|s| s.section == name).map(|s| s.key).collect::<Vec<_>>();
    assert_eq!(keys("Window"), ["appearance.opacity", "appearance.background-blur", "appearance.backdrop", "appearance.density", "appearance.tab-bar", "appearance.tab-index", "appearance.tab-icon", "appearance.tab-close-buttons", "appearance.tab-menu-button", "appearance.pane-titles", "panes.resize-step", "panes.focus-follows-mouse", "appearance.profile-sidebar", "appearance.profile-sidebar-width", "appearance.tabs-in-fullscreen", "appearance.tab-width", "appearance.tab-bar-position", "appearance.mru-tabs", "appearance.cycle-tabs", "appearance.animations", "terminal.close-window-with-last-tab", "notifications.command-finished", "notifications.activity", "notifications.activity-silence-seconds", "appearance.tray"]);
    for key in ["terminal.font", "terminal.font-size", "terminal.line-padding", "terminal.padding-top", "terminal.padding-bottom", "terminal.padding-right", "terminal.padding-left", "terminal.cursor", "terminal.cursor-blink", "terminal.fallback-font", "appearance.ui-font"] {
        assert!(keys(APPEARANCE).contains(&key));
    }
    assert_eq!(&PAGES[..4], [SYSTEM, APPEARANCE, PROFILES, TERMINAL]);
    assert!(PAGES[4..PAGES.len() - 3].iter().filter(|&&name| name != EXTENSIONS).map(|name| name.to_ascii_lowercase()).is_sorted());
    assert!(PAGES.windows(2).any(|pages| pages == [SHORTCUTS, EXTENSIONS]));
    assert_eq!(&PAGES[PAGES.len() - 3..], [VAULT, SYNC, CONFIG_FILE]);
}

#[test]
fn config_sync_shows_the_active_backend_and_persists_parts_as_an_array() {
    use crate::sync::{Backend, Part as SyncPart};
    let mut a = app();
    a.open_settings();
    a.show_settings_page(PAGES.iter().position(|name| *name == SYNC).unwrap());
    let shown = |a: &App, key: &str| a.settings.as_ref().unwrap().shown.iter().any(|&i| SETTINGS.get(i).is_some_and(|s| s.key == key));
    assert!(!shown(&a, "sync.folder"));
    let mut config = a.config.clone();
    config.sync.backend = Backend::Folder;
    a.set_config(config);
    assert!(shown(&a, "sync.folder"));
    assert!(!shown(&a, "sync.repository"));
    assert!(!a.config.sync.parts.contains(&SyncPart::Vault));
    a.apply_setting("sync.parts.vault", Value::Boolean(true));
    let saved = saved(&mut a).unwrap();
    assert!(saved.sync.parts.contains(&SyncPart::Vault));
    assert!(saved.to_text().contains("parts = ["));
}

#[test]
fn deleting_a_synced_config_requires_a_second_action() {
    use crate::sync::{Backend, Entry, Operation, View};
    use crate::sync_settings::Row as SyncRow;
    let mut a = app();
    let mut config = a.config.clone();
    config.sync.backend = Backend::Folder;
    config.sync.folder = "store".into();
    a.set_config(config);
    a.open_settings();
    a.show_settings_page(PAGES.iter().position(|name| *name == SYNC).unwrap());
    a.set_sync_view(View { entries: vec![Entry { name: "Shared".into(), machine: "Laptop".into(), modified: 1 }], ..Default::default() });
    let page = a.settings.as_mut().unwrap();
    page.selected = page.shown.iter().position(|&item| page.sync_row(item) == Some(SyncRow::Entry(0))).unwrap();
    a.take_effects();
    a.settings_key(Key::Delete, Mods::NONE);
    assert!(!a.take_effects().iter().any(|effect| matches!(effect, Effect::ConfigSync(Operation::Delete(_)))));
    a.settings_key(Key::Enter, Mods::NONE);
    assert!(a.take_effects().iter().any(|effect| matches!(effect, Effect::ConfigSync(Operation::Delete(name)) if name == "Shared")));
}

#[test]
fn opacity_uses_a_dropdown_and_keeps_keyboard_steps() {
    let mut a = app();
    a.chrome = a.tab_chrome();
    a.run(Action::Settings);
    typed(&mut a, "opacity");
    let page = a.settings.as_ref().unwrap();
    let selected = page.shown.iter().position(|&item| SETTINGS.get(item).is_some_and(|setting| setting.key == "appearance.opacity")).unwrap();
    let r = a.settings_geometry().unwrap().rows.into_iter().find(|r| r.kind == RowKind::Item(selected)).unwrap().control;
    let image_opacity = a.config().appearance.background_image_opacity;
    let (x, y) = (r.x + r.w * 0.5, r.y + r.h * 0.5);
    a.settings_click(x, y);
    assert!(!a.settings_geometry().unwrap().suggestions.is_empty());
    key(&mut a, Key::Home);
    key(&mut a, Key::Enter);
    assert!(a.config().appearance.opacity < 0.8);
    assert_eq!(a.config().appearance.background_image_opacity, image_opacity);
    let before = a.config().appearance.opacity;
    key(&mut a, Key::Right);
    assert!((a.config().appearance.opacity - before - 0.05).abs() < 0.001);
}

#[test]
fn image_fit_lists_all_modes_and_a_click_selects_the_mode() {
    let mut a = app();
    a.chrome = a.tab_chrome();
    a.run(Action::Settings);
    typed(&mut a, "background image fit");
    key(&mut a, Key::Enter);
    let geometry = a.settings_geometry().unwrap();
    let menu = a.settings.as_ref().unwrap().choice_menu.as_ref().unwrap();
    assert_eq!(menu.options.iter().map(|(value, _)| value.as_str().unwrap()).collect::<Vec<_>>(), ["cover", "contain", "stretch", "tile"]);
    let row = geometry.suggestions.iter().find(|(index, _)| *index == 3).unwrap().1;
    a.settings_click(row.x + row.w / 2.0, row.y + row.h / 2.0);
    assert_eq!(a.config().appearance.background_image_fit, crate::background::Fit::Tile);
    assert!(a.settings.as_ref().unwrap().choice_menu.is_none());
}

#[test]
fn blur_starts_at_zero_and_is_only_editable_below_full_opacity() {
    let mut a = app();
    a.chrome = a.tab_chrome();
    a.run(Action::Settings);
    typed(&mut a, "background blur");
    key(&mut a, Key::Enter);
    assert!(a.settings.as_ref().unwrap().choice_menu.is_none());
    assert_eq!(a.config().appearance.background_blur, 0.0);
    let mut config = a.config().clone();
    config.appearance.opacity = 0.75;
    a.set_config(config);
    key(&mut a, Key::Enter);
    assert!(a.settings.as_ref().unwrap().choice_menu.is_some());
    key(&mut a, Key::End);
    key(&mut a, Key::Enter);
    assert_eq!(saved(&mut a).unwrap().appearance.background_blur, 1.0);
}

#[test]
fn settings_section_headings_never_hide_rows() {
    let mut a = app();
    a.chrome = a.tab_chrome();
    a.run(Action::Settings);
    a.show_settings_page(PAGES.iter().position(|&page| page == WINDOW).unwrap());
    let geo = a.settings_geometry().unwrap();
    let heading = geo.rows.iter().find(|row| row.kind == RowKind::Section(WINDOW)).unwrap().rect;
    a.settings_click(heading.x + 15.0, heading.y + 18.0);
    assert_eq!(a.settings_geometry().unwrap().rows, geo.rows);
    a.on_event(Event::Key { key: Key::Up, mods: Mods { alt: true, ..Mods::NONE }, pressed: true });
    assert_eq!(a.settings_geometry().unwrap().rows, geo.rows);
    typed(&mut a, "opacity");
    assert!(a.settings_geometry().unwrap().rows.iter().any(|row| matches!(row.kind, RowKind::Item(_))));
}

#[test]
fn empty_font_input_offers_installed_families_on_open() {
    let mut a = app();
    a.chrome = a.tab_chrome();
    a.set_font_families(vec![], vec!["Interface Family".into()]);
    a.run(Action::Settings);
    typed(&mut a, "interface font");
    key(&mut a, Key::Enter);
    assert_eq!(a.settings.as_ref().unwrap().font_matches, ["Interface Family"]);
    assert!(!a.settings_geometry().unwrap().suggestions.is_empty());
}

#[test]
fn the_color_scheme_page_displays_the_same_font_sample_as_appearance() {
    let mut a = app();
    a.chrome = a.tab_chrome();
    a.run(Action::Settings);
    for section in [APPEARANCE, COLORS] {
        a.show_settings_page(PAGES.iter().position(|page| *page == section).unwrap());
        let geo = a.settings_geometry().unwrap();
        let sample = geo.font_sample.expect("font sample");
        assert!(sample.h > 0.0 && sample.bottom() <= geo.list.y);
        if let Some(bar) = geo.scheme_bar { assert!(sample.y >= bar.current.bottom()); }
    }
}

#[test]
fn settings_geometry_clamps_saved_scroll_to_the_new_viewport() {
    let mut page = SettingsPage::new();
    page.show_page(PAGES.iter().position(|&page| page == WINDOW).unwrap());
    page.scroll = 2000.0;
    let small = geometry(Rect::new(0.0, 0.0, 1000.0, 450.0), 1.0, &page);
    let large = geometry(Rect::new(0.0, 0.0, 1000.0, 1000.0), 1.0, &page);
    assert!(large.scroll < small.scroll);
    assert_eq!(large.rows.last().unwrap().rect.bottom(), large.list.bottom());
}

#[test]
fn restart_button_tracks_startup_changes_and_uses_the_restart_effect() {
    let mut a = app();
    a.chrome = a.tab_chrome();
    a.run(Action::Settings);
    assert!(a.settings_geometry().unwrap().restart.is_none());
    typed(&mut a, "terminal.open-on-start");
    key(&mut a, Key::Enter);
    key(&mut a, Key::Home);
    key(&mut a, Key::Enter);
    assert!(!saved(&mut a).unwrap().terminal.open_on_start);
    let r = a.settings_geometry().unwrap().restart.unwrap();
    a.settings_click(r.x + 4.0, r.y + 4.0);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Restart)));
    key(&mut a, Key::Enter);
    key(&mut a, Key::End);
    key(&mut a, Key::Enter);
    assert!(a.settings_geometry().unwrap().restart.is_none());
}

#[test]
fn loading_a_newer_config_warns_and_settings_cannot_edit_it() {
    let mut a = app();
    let newer = Config::parse("version = 42\n[terminal]\nopen-on-start = false").unwrap();
    a.set_config(newer.clone());
    assert!(a.tab_toast.as_ref().unwrap().text.contains("42"));
    a.run(Action::Settings);
    typed(&mut a, "terminal.open-on-start");
    key(&mut a, Key::Enter);
    key(&mut a, Key::Home);
    key(&mut a, Key::Enter);
    assert!(saved(&mut a).is_none());
    assert_eq!(a.config(), &newer);
    assert!(a.settings_geometry().unwrap().restart.is_none());
}

#[test]
fn color_swatches_accept_only_complete_hex_colors() {
    assert_eq!(color_value(" #00Aaff "), Some(aster_gfx::render::Color::rgb(0x00aaff)));
    for text in ["", "red", "#fff", "#0088xx", "#0088ff99", "#+12345"] {
        assert!(color_value(text).is_none(), "{text}");
    }
}

#[test]
fn cursor_shape_can_be_picked_directly_from_its_dropdown() {
    let mut a = app();
    a.chrome = a.tab_chrome();
    a.run(Action::Settings);
    typed(&mut a, "cursor shape");
    let row = a.settings_geometry().unwrap().rows.into_iter().find(|r| matches!(r.kind, RowKind::Item(0))).unwrap();
    a.settings_click(row.control.right() - 2.0, row.control.y + 5.0);
    assert!(saved(&mut a).is_none());
    let choice = a.settings_geometry().unwrap().suggestions.iter().find(|(index, _)| *index == 2).unwrap().1;
    a.settings_click(choice.x + 2.0, choice.y + 2.0);
    assert_eq!(saved(&mut a).unwrap().terminal.cursor, crate::config::CursorStyle::Underline);
    key(&mut a, Key::Left);
    assert_eq!(saved(&mut a).unwrap().terminal.cursor, crate::config::CursorStyle::Beam);
}

#[test]
fn shortcut_capture_adds_without_duplicates_and_replaces_chords() {
    let mut a = app();
    let mut config = a.config().clone();
    config.hotkeys.insert("new-tab".into(), vec!["Ctrl-K Ctrl-C".into()]);
    a.set_config(config);
    select_shortcut(&mut a, "new tab");
    for _ in 0..2 {
        key(&mut a, Key::Insert);
        a.on_event(ctrl(Key::Char('J')));
    }
    assert_eq!(a.shortcut_text(Action::NewTab), "Ctrl-K Ctrl-C, Ctrl-J");
    key(&mut a, Key::Enter);
    a.on_event(ctrl(Key::Char('U')));
    assert_eq!(saved(&mut a).unwrap().hotkeys["new-tab"], ["Ctrl-U"]);
}

#[test]
fn shortcut_chips_wrap_and_keep_the_hint_below_all_bindings() {
    let bindings = vec!["Ctrl-K Ctrl-C".into(), "Ctrl-Shift-J".into(), "Ctrl-Alt-Shift-F2".into(), "Ctrl-U".into()];
    let hint = "Enter replace · Insert add · Del clear · Shift+Del restore";
    let row = ShortcutLayout::new(240.0, 1.0, bindings.clone(), hint, |s| s.len() as f32 * 7.0);
    assert_eq!(row.chips.iter().map(|(lines, _)| lines.join(" ")).collect::<Vec<_>>(), bindings);
    assert!(row.chips.windows(2).any(|chips| chips[1].1.y > chips[0].1.y));
    assert!(row.chips.iter().all(|(_, r)| r.right() <= 240.0 && r.bottom() < row.hint_top));
    assert_eq!(row.hint_lines.join(" "), hint);
    assert!(row.height > row.hint_top + 16.0);
}

#[test]
fn shortcut_editing_keys_work_when_assigned_and_ignore_captured_text() {
    let mut a = app();
    let mut config = a.config().clone();
    config.hotkeys.insert("new-window".into(), vec!["Insert".into()]);
    a.set_config(config);
    select_shortcut(&mut a, "new tab");
    key(&mut a, Key::Insert);
    key(&mut a, Key::Space);
    a.on_event(Event::Char(' '));
    assert_eq!(a.settings.as_ref().unwrap().query, "new tab");
    assert_eq!(a.shortcut_text(Action::NewTab), "Ctrl-Shift-T, Space");
}

#[test]
fn font_suggestions_filter_by_family_and_accept_keyboard_or_click() {
    let mut a = app();
    a.chrome = a.tab_chrome();
    let mono: Vec<_> = ["Cascadia Code", "Cascadia Mono PL"].map(str::to_owned).into();
    let mut all = mono.clone();
    all.push("Book Caslon".into());
    a.set_font_families(mono, all);
    a.run(Action::Settings);
    a.show_settings_page(PAGES.iter().position(|p| *p == APPEARANCE).unwrap());
    key(&mut a, Key::Enter);
    typed(&mut a, "CAS");
    assert_eq!(a.settings.as_ref().unwrap().font_matches, ["Cascadia Code", "Cascadia Mono PL"]);
    key(&mut a, Key::Down);
    key(&mut a, Key::Enter);
    assert_eq!(saved(&mut a).unwrap().terminal.font, "Cascadia Mono PL");
    typed(&mut a, "interface font");
    key(&mut a, Key::Enter);
    typed(&mut a, "casl");
    let r = a.settings_geometry().unwrap().suggestions[0].1;
    a.settings_click(r.x + 2.0, r.y + 2.0);
    assert_eq!(saved(&mut a).unwrap().appearance.ui_font, "Book Caslon");
    key(&mut a, Key::Enter);
    typed(&mut a, "custom family");
    key(&mut a, Key::Enter);
    assert_eq!(a.config().appearance.ui_font, "custom family");
}

#[test]
fn font_suggestions_limit_matches_and_keep_the_last_choice_visible() {
    let mut a = app();
    a.set_viewport(520, 290, 1.0);
    a.chrome = a.tab_chrome();
    let mono = (0..13).map(|i| format!("Example Mono {i}")).collect();
    a.set_font_families(mono, Vec::new());
    a.run(Action::Settings);
    typed(&mut a, "terminal.fallback-font");
    key(&mut a, Key::Enter);
    typed(&mut a, "EXample");
    for _ in 0..10 {
        key(&mut a, Key::Down);
    }
    let page = a.settings.as_ref().unwrap();
    assert_eq!((page.font_matches.len(), page.font_index), (8, 7));
    let geo = a.settings_geometry().unwrap();
    let &(index, last) = geo.suggestions.last().unwrap();
    assert_eq!(index, 7);
    assert!(last.bottom() <= geo.list.bottom());
    a.settings_click(last.x + 1.0, last.y + 1.0);
    assert_eq!(saved(&mut a).unwrap().terminal.fallback_font, "Example Mono 7");
}

#[test]
fn setting_keys_are_unique() {
    let mut keys: Vec<_> = SETTINGS.iter().map(|s| s.key).collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), SETTINGS.len());
}

#[test]
fn config_page_reports_disk_status_and_runs_file_actions() {
    let mut a = app();
    a.set_config_path(Some("data/config.toml".to_owned()));
    a.run(Action::Settings);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::CheckConfigFile)));
    a.on_event(ctrl(Key::PageUp));
    let page = a.settings.as_ref().unwrap();
    assert_eq!(PAGES[page.page], "Config file");
    let rows: Vec<_> = page.shown.iter().map(|&i| &SETTINGS[i]).collect();
    assert_eq!(rows.iter().map(|s| s.label).collect::<Vec<_>>(), ["File path", "Open in the editor", "Open the folder", "File status", "Show the defaults"]);
    assert_eq!(a.setting_text(rows[0]), "data/config.toml");
    key(&mut a, Key::Enter);
    assert!(!a.settings.as_ref().unwrap().editing());
    a.set_config_status(None);
    assert_eq!(a.setting_text(rows[3]), t("The config file is valid"));
    a.set_config_status(Some("line 3, column 9: invalid key".into()));
    assert_eq!(a.setting_text(rows[3]), "line 3, column 9: invalid key");
    for (at, expected) in [(1, Action::OpenConfigFile), (2, Action::OpenConfigFolder), (4, Action::ShowDefaultConfig)] {
        assert!(matches!(rows[at].control, Control::Run { action, .. } if action == &expected));
        a.settings.as_mut().unwrap().selected = at;
        key(&mut a, Key::Enter);
        assert!(a.take_effects().iter().any(|e| matches!((e, &expected),
            (Effect::OpenConfigFile, Action::OpenConfigFile) | (Effect::OpenConfigFolder, Action::OpenConfigFolder) | (Effect::ShowDefaultConfig, Action::ShowDefaultConfig))));
    }
}

#[test]
fn application_page_opens_the_notes_and_checks_for_updates_in_place() {
    let mut a = app();
    a.chrome = a.tab_chrome();
    a.run(Action::Settings);
    let page = a.settings.as_ref().unwrap();
    let keys: Vec<_> = page.shown.iter().map(|&i| SETTINGS[i].key).collect();
    assert_eq!(keys[..4], APPLICATION_ACTIONS);
    assert_eq!(keys[4], "appearance.language");
    assert_eq!(keys[keys.len() - 2..], ACCESSIBILITY_SETTINGS);
    let geo = a.settings_geometry().unwrap();
    let about = geo.about.unwrap();
    let link = |n| geo.rows.iter().find(|row| row.kind == RowKind::Item(n)).unwrap().control;
    assert!(link(0).x >= about.x && link(0).right() <= about.right());
    assert!(link(1).x >= about.right());
    assert!(geo.rows.iter().any(|row| row.kind == RowKind::Section(APPLICATION_SETTINGS)));
    assert!(geo.rows.iter().any(|row| row.kind == RowKind::Section(ACCESSIBILITY)));
    for (n, prefix) in [(1, "https://github.com/brano-san/Aster/issues/new?body="), (2, "https://github.com/brano-san/Aster")] {
        let r = link(n);
        a.settings_click(r.x + 8.0, r.y + 8.0);
        let effects = a.take_effects();
        assert!(effects.iter().any(|e| matches!(e, Effect::Open(target) if if n == 1 { target.starts_with(prefix) } else { target == prefix })));
    }
    let r = link(3);
    a.settings_click(r.x + 8.0, r.y + 8.0);
    let url = concat!("https://github.com/brano-san/Aster/releases/tag/v", env!("CARGO_PKG_VERSION"));
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Open(target) if target == url)));
    a.settings.as_mut().unwrap().selected = 0;
    key(&mut a, Key::Enter);
    let effects = a.take_effects();
    assert!(effects.iter().any(|e| matches!(e, Effect::Update(crate::update::UpdateRequest::Check { manual: true }))));
    assert!(!effects.iter().any(|e| matches!(e, Effect::Open(_))));
    key(&mut a, Key::Enter);
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::Update(_))));
    a.settings.as_mut().unwrap().selected = keys.iter().position(|&key| key == "updates.check-automatically").unwrap();
    key(&mut a, Key::Enter);
    key(&mut a, Key::Home);
    key(&mut a, Key::Enter);
    assert!(!saved(&mut a).unwrap().updates.check_automatically);

    use crate::update::{Release, UpdateEvent, UpdateRequest};
    let release = Release { version: "9.1.0".into(), notes: String::new(), url: String::new(), signature: String::new() };
    a.on_update(UpdateEvent::Checked { manual: true, found: Ok(Some(release.clone())) });
    assert_eq!(a.application_update_label(), t("Update"));
    a.settings.as_mut().unwrap().selected = 0;
    key(&mut a, Key::Enter);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Update(UpdateRequest::Install(target)) if *target == release)));
    assert_eq!(a.application_update_label(), t("Downloading…"));
}

#[test]
fn application_overview_stacks_on_narrow_windows_and_leaves_room_for_search_results() {
    let mut page = SettingsPage::new();
    let viewport = Rect::new(0.0, 0.0, 500.0, 600.0);
    let geo = geometry(viewport, 1.0, &page);
    let about = geo.about.unwrap();
    assert_eq!(about.w, geo.list.w);
    for row in &geo.rows[1..APPLICATION_ACTIONS.len()] {
        assert!(row.rect.y >= about.bottom());
        assert_eq!(row.rect.w, about.w);
    }
    page.query = "github".into();
    page.refilter();
    let searched = geometry(viewport, 1.0, &page);
    assert!(searched.about.is_none());
    assert!(page.shown.iter().any(|&i| SETTINGS.get(i).is_some_and(|setting| setting.key == "action.open-repository")));
}

#[test]
fn autostart_is_off_by_default_and_can_be_toggled_from_application_settings() {
    let mut a = app();
    assert!(!a.config().system.autostart);
    a.run(Action::Settings);
    let page = a.settings.as_mut().unwrap();
    page.selected = page.shown.iter().position(|&i| SETTINGS[i].key == "system.autostart").unwrap();
    key(&mut a, Key::Enter);
    key(&mut a, Key::End);
    key(&mut a, Key::Enter);
    assert!(saved(&mut a).unwrap().system.autostart);
}

#[test]
fn applications_menu_shows_only_the_available_launcher_action() {
    let mut a = app();
    a.run(Action::Settings);
    let launcher_action = |app: &App| app.settings.as_ref().unwrap().shown.iter().position(|&i| {
        matches!(SETTINGS[i].control, Control::Run { action: Action::ApplicationsMenu(_), .. })
    });
    assert!(launcher_action(&a).is_none());
    for installed in [false, true] {
        a.set_launcher_installed(installed);
        a.settings.as_mut().unwrap().selected = launcher_action(&a).unwrap();
        key(&mut a, Key::Enter);
        assert!(a.take_effects().iter().any(|e| matches!(e, Effect::ApplicationsMenu(install) if *install != installed)));
    }
}

#[test]
fn values_are_written_by_path_and_type_checked() {
    let config = set(&Config::default(), "terminal.search-options.regex", Value::Boolean(true)).unwrap();
    assert!(config.terminal.search_options.regex);
    let config = set(&config, "appearance.dock", Value::String("left".into())).unwrap();
    assert_eq!(config.appearance.dock, crate::config::Dock::Left);
    assert!(set(&config, "appearance.dock", Value::String("middle".into())).is_err());
    assert!(set(&config, "terminal.font-size", Value::String("big".into())).is_err());
}

#[test]
fn numbers_print_without_float_noise() {
    assert_eq!(number_text(14.0), "14");
    assert_eq!(number_text(0.550000011920929), "0.55");
}

#[test]
fn the_query_filters_by_every_word_across_label_hint_and_section() {
    let mut page = SettingsPage::new();
    page.vault.configured = true;
    page.installation = crate::installation::State::Available;
    let mut listed = std::collections::BTreeSet::new();
    use crate::default_terminal::State as DefaultTerminal;
    use crate::sync::Backend;
    for (installed, backend, terminal) in [
        (None, Backend::Off, DefaultTerminal::Unavailable),
        (Some(false), Backend::Folder, DefaultTerminal::Available),
        (Some(true), Backend::Git, DefaultTerminal::Selected),
    ] {
        page.launcher_installed = installed;
        page.sync.backend = backend;
        page.default_terminal = terminal;
        page.profiles_advanced = false;
        for (k, name) in PAGES.iter().enumerate() {
            page.show_page(k);
            assert!(page.shown.iter().all(|&i| page.section(i) == *name
                || *name == SYSTEM && SETTINGS.get(i).is_some_and(|setting| ACCESSIBILITY_SETTINGS.contains(&setting.key))), "{name} shows only its own items");
            listed.extend(page.shown.iter().copied().filter(|&i| i < SETTINGS.len()));
            if *name == PROFILES {
                page.profiles_advanced = true;
                page.refilter();
                listed.extend(page.shown.iter().copied().filter(|&i| i < SETTINGS.len()));
            }
            if *name == SHORTCUTS {
                assert!(page.shown.iter().any(|&i| shortcut(i).is_some()), "shortcuts have their page");
            }
        }
    }
    let tab_settings: Vec<_> = SETTINGS.iter().enumerate().filter(|(i, _)| !listed.contains(i)).map(|(_, setting)| setting.key).collect();
    assert_eq!(tab_settings, ["extensions.index-url", "appearance.color-scheme", "appearance.light-color-scheme"]);
    for key in tab_settings {
        page.show_page(0);
        page.query = key.into();
        page.refilter();
        let index = SETTINGS.iter().position(|setting| setting.key == key).unwrap();
        assert!(page.shown.contains(&index), "{key} is reachable via settings search");
        listed.insert(index);
    }
    assert_eq!(listed.len(), SETTINGS.len(), "every setting is on some page");
    page.query = "dock top".into();
    page.refilter();
    let keys: Vec<_> = page.shown.iter().map(|&i| SETTINGS[i].key).collect();
    assert_eq!(keys, ["appearance.dock-always-on-top"]);
}

#[cfg(windows)]
#[test]
fn install_action_is_visible_only_for_an_unpacked_nonportable_copy() {
    use crate::installation::State;
    let setting = SETTINGS.iter().position(|setting| setting.key == "action.install-for-user").unwrap();
    let mut page = SettingsPage::new();
    for state in [State::Unavailable, State::Installed, State::Available] {
        page.installation = state;
        page.refilter();
        assert_eq!(page.shown.contains(&setting), state == State::Available);
    }
}

#[test]
fn boolean_options_save_only_after_choosing_an_item() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "blinking");
    key(&mut a, Key::Enter);
    assert!(a.config().terminal.cursor_blink);
    key(&mut a, Key::Home);
    key(&mut a, Key::Enter);
    assert!(!a.config().terminal.cursor_blink);
    assert_eq!(saved(&mut a).map(|c| c.terminal.cursor_blink), Some(false));
    key(&mut a, Key::Escape);
    assert!(a.settings_open(), "the first escape clears the query");
    key(&mut a, Key::Escape);
    assert!(!a.settings_open());
}

#[test]
fn arrows_step_numbers_within_bounds_and_cycle_choices() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "font size");
    key(&mut a, Key::Right);
    assert_eq!(a.config().terminal.font_size, 15.0);
    for _ in 0..60 {
        key(&mut a, Key::Right);
    }
    assert_eq!(a.config().terminal.font_size, 48.0);
    a.settings = Some(SettingsPage::new());
    typed(&mut a, "cursor shape");
    key(&mut a, Key::Left);
    assert_eq!(a.config().terminal.cursor, crate::config::CursorStyle::Underline);
    key(&mut a, Key::Right);
    assert_eq!(a.config().terminal.cursor, crate::config::CursorStyle::Block);
}

#[test]
fn integer_settings_stay_integers() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "scrollback");
    key(&mut a, Key::Right);
    assert_eq!(a.config().terminal.scrollback_lines, 26_000);
    assert!(saved(&mut a).is_some());
}

#[test]
fn text_and_numbers_are_edited_in_place() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "interface font");
    key(&mut a, Key::Enter);
    typed(&mut a, "Inter");
    key(&mut a, Key::Enter);
    assert_eq!(a.config().appearance.ui_font, "Inter");
    a.settings = Some(SettingsPage::new());
    typed(&mut a, "line padding");
    key(&mut a, Key::Enter);
    key(&mut a, Key::Backspace);
    typed(&mut a, "7");
    key(&mut a, Key::Escape);
    assert_eq!(a.config().terminal.line_padding, 0.0, "escape drops the edit");
    key(&mut a, Key::Enter);
    key(&mut a, Key::Backspace);
    typed(&mut a, "99");
    key(&mut a, Key::Enter);
    assert_eq!(a.config().terminal.line_padding, 20.0, "typed numbers are clamped");
}

#[test]
fn the_default_profile_offers_the_automatic_choice_and_every_profile() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "default profile");
    key(&mut a, Key::Right);
    assert_eq!(a.config().terminal.profile, "local:a");
    key(&mut a, Key::Right);
    key(&mut a, Key::Right);
    assert_eq!(a.config().terminal.profile, "");
}

#[test]
fn keys_do_not_reach_the_terminal_while_settings_are_open() {
    let mut a = app();
    a.open_default_tab();
    a.run(Action::Settings);
    typed(&mut a, "ls");
    key(&mut a, Key::Enter);
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::Write { .. })));
}

#[test]
fn the_settings_scrollbar_drags_and_jumps_on_click() {
    use crate::input::MouseButton;

    let mut a = app();
    a.set_viewport(1000, 360, 1.0);
    a.chrome = a.tab_chrome();
    a.run(Action::Settings);
    let geo = a.settings_geometry().unwrap();
    let max = geo.height - geo.list.h;
    assert!(max > 0.0);
    let x = geo.list.right() + 10.0;
    let top = geo.list.y + 8.0;
    let bottom = geo.list.bottom() + 10.0;
    let down = |a: &mut App, y| a.on_event(Event::MouseDown { x, y, button: MouseButton::Left, clicks: 1 });
    let up = |a: &mut App, y| a.on_event(Event::MouseUp { x, y, button: MouseButton::Left });

    down(&mut a, top);
    assert_eq!(a.settings.as_ref().unwrap().scroll, 0.0);
    a.on_event(Event::MouseMove { x, y: bottom });
    assert_eq!(a.settings.as_ref().unwrap().scroll, max);
    up(&mut a, bottom);
    a.on_event(Event::MouseMove { x, y: top });
    assert_eq!(a.settings.as_ref().unwrap().scroll, max);
    down(&mut a, top);
    up(&mut a, top);
    assert_eq!(a.settings.as_ref().unwrap().scroll, 0.0);
}

#[test]
fn moving_the_selection_scrolls_it_into_view() {
    let mut a = app();
    a.set_viewport(1000, 360, 1.0);
    a.chrome = a.tab_chrome();
    a.run(Action::Settings);
    let rows = a.settings.as_ref().unwrap().shown.len();
    for _ in 0..rows {
        key(&mut a, Key::Down);
    }
    let geo = a.settings_geometry().unwrap();
    let page = a.settings.as_ref().unwrap();
    assert_eq!(page.selected, rows - 1);
    let row = geo.rows.iter().find(|r| r.kind == RowKind::Item(page.selected)).unwrap();
    assert!(row.rect.bottom() <= geo.list.bottom() + 0.5 && row.rect.y >= geo.list.y);
    assert!(page.scroll > 0.0);
}

#[test]
fn clicking_a_boolean_option_applies_it() {
    let mut a = app();
    a.chrome = a.tab_chrome();
    a.run(Action::Settings);
    typed(&mut a, "copy on select");
    let row = a.settings_geometry().unwrap().rows.into_iter().find(|r| matches!(r.kind, RowKind::Item(_))).unwrap();
    a.settings_click(row.control.x + 20.0, row.control.y + 10.0);
    let off = a.settings_geometry().unwrap().suggestions[0].1;
    a.settings_click(off.x + 5.0, off.y + 5.0);
    assert!(!a.config().terminal.copy_on_select);
}

#[test]
fn each_section_is_listed_in_one_piece() {
    let mut seen: Vec<&str> = Vec::new();
    for s in SETTINGS {
        if seen.last() != Some(&s.section) {
            assert!(!seen.contains(&s.section), "{} appears in two places", s.section);
            seen.push(s.section);
        }
    }
}

fn ctrl_shift(c: char) -> Mods {
    let _ = c;
    Mods { ctrl: true, shift: true, ..Mods::NONE }
}

fn select_shortcut(a: &mut App, query: &str) {
    a.run(Action::Settings);
    typed(a, query);
    let page = a.settings.as_ref().unwrap();
    let at = page.shown.iter().position(|&i| shortcut(i).is_some()).expect("a shortcut row");
    a.settings.as_mut().unwrap().selected = at;
}

#[test]
fn a_captured_shortcut_replaces_the_binding_and_frees_it_elsewhere() {
    let mut a = app();
    select_shortcut(&mut a, "new tab");
    key(&mut a, Key::Enter);
    assert!(a.settings.as_ref().unwrap().capture.is_some());
    a.on_event(Event::Key { key: Key::Char('P'), mods: ctrl_shift('P'), pressed: true });
    let config = saved(&mut a).expect("saved");
    assert_eq!(config.hotkeys.get("new-tab"), Some(&vec!["Ctrl-Shift-P".to_owned()]));
    assert_eq!(config.hotkeys.get("command-selector"), Some(&Vec::<String>::new()), "the old owner loses it");
    assert!(a.settings.as_ref().unwrap().notice.as_deref().is_some_and(|n| n.contains("Show command selector")));
    assert_eq!(a.shortcut_text(Action::NewTab), "Ctrl-Shift-P");
}

#[test]
fn escape_cancels_capture_and_delete_clears_or_restores_the_default() {
    let mut a = app();
    select_shortcut(&mut a, "new tab");
    key(&mut a, Key::Enter);
    key(&mut a, Key::Escape);
    assert!(a.settings.as_ref().unwrap().capture.is_none());
    assert!(a.settings_open(), "escape only ends the capture");
    key(&mut a, Key::Delete);
    assert_eq!(a.shortcut_text(Action::NewTab), "None");
    a.on_event(Event::Key { key: Key::Delete, mods: Mods { shift: true, ..Mods::NONE }, pressed: true });
    assert_eq!(a.shortcut_text(Action::NewTab), "Ctrl-Shift-T");
    assert!(!a.config().hotkeys.contains_key("new-tab"));
}

#[test]
fn the_settings_shortcut_itself_can_be_captured() {
    let mut a = app();
    select_shortcut(&mut a, "open settings");
    key(&mut a, Key::Enter);
    a.on_event(Event::Key { key: Key::Char(','), mods: Mods { ctrl: true, ..Mods::NONE }, pressed: true });
    assert!(a.settings_open());
    assert_eq!(a.shortcut_text(Action::Settings), "Ctrl-,");
}

fn select_profile(a: &mut App, name: &str) {
    a.run(Action::Settings);
    a.settings.as_mut().unwrap().show_page(PAGES.iter().position(|p| *p == PROFILES).unwrap());
    let page = a.settings.as_ref().unwrap();
    let at = page.shown.iter().position(|&i| match page.profile_at(i) {
        Some(p) => p.name == name,
        None => name.is_empty() && page.is_new_profile(i),
    }).expect("a profile row");
    a.settings.as_mut().unwrap().selected = at;
}

fn ctrl(k: Key) -> Event {
    Event::Key { key: k, mods: Mods { ctrl: true, ..Mods::NONE }, pressed: true }
}

fn focus_field(a: &mut App, tab: &str, label: &str) {
    let editor = a.settings.as_ref().unwrap().editor.as_ref().unwrap();
    let index = editor.tabs.iter().position(|t| t.label == tab).expect("editor tab");
    let steps = (index + editor.tabs.len() - editor.tab) % editor.tabs.len();
    for _ in 0..steps { a.on_event(ctrl(Key::Tab)); }
    let editor = a.settings.as_ref().unwrap().editor.as_ref().unwrap();
    let fields = &editor.tabs[editor.tab].fields;
    let index = fields.iter().position(|f| f.label == label).expect("editor field");
    let steps = (index + fields.len() - editor.focus) % fields.len();
    for _ in 0..steps { key(a, Key::Tab); }
}

fn choose_template(a: &mut App, name: &str) {
    select_profile(a, "");
    key(a, Key::Enter);
    key(a, Key::Enter);
    typed(a, name);
    key(a, Key::Enter);
}

#[test]
fn profile_choices_commit_only_after_accepting_the_highlighted_option() {
    let mut a = app();
    let mut config = a.config().clone();
    config.groups.extend(["Build", "Prod", "Test"].map(|name| crate::profile::ProfileGroup { name: name.into(), ..Default::default() }));
    a.set_config(config);
    choose_template(&mut a, "Local shell");
    focus_field(&mut a, "General", "Name");
    typed(&mut a, "Terminal");
    focus_field(&mut a, "General", "Group");

    key(&mut a, Key::Enter);
    key(&mut a, Key::End);
    let editor = a.settings.as_ref().unwrap().editor.as_ref().unwrap();
    assert!(editor.tabs[editor.tab].fields[editor.focus].value.is_empty());
    key(&mut a, Key::Escape);

    key(&mut a, Key::Space);
    key(&mut a, Key::End);
    key(&mut a, Key::Up);
    key(&mut a, Key::Enter);
    a.on_event(ctrl(Key::Enter));
    assert_eq!(saved(&mut a).unwrap().profiles[0].group.as_deref(), Some("Prod"));
}

#[test]
fn profile_on_off_fields_use_the_same_dropdown_as_other_choices() {
    let mut a = app();
    choose_template(&mut a, "Local shell");
    focus_field(&mut a, "General", "Name");
    typed(&mut a, "Private");
    focus_field(&mut a, "General", "Hidden");
    key(&mut a, Key::Enter);
    let editor = a.settings.as_ref().unwrap().editor.as_ref().unwrap();
    assert_eq!(editor.tabs[editor.tab].fields[editor.focus].value, "false");
    key(&mut a, Key::End);
    key(&mut a, Key::Enter);
    a.on_event(ctrl(Key::Enter));
    assert!(saved(&mut a).unwrap().profiles[0].hidden);
}

#[test]
fn an_editable_profile_choice_keeps_text_entry_and_accepts_a_mouse_selection() {
    use crate::profiles_page::layout::field_control;
    let mut a = app();
    choose_template(&mut a, "Serial port");
    typed(&mut a, "COM42");
    focus_field(&mut a, "Serial", "Speed");
    a.chrome = a.tab_chrome();
    let area = a.settings_geometry().unwrap().page;
    let page = a.settings.as_ref().unwrap();
    let layout = page.profile_layout(area, 1.0).unwrap();
    let editor = page.editor.as_ref().unwrap();
    let slot = layout.fields.iter().find(|(i, _)| *i == editor.focus).unwrap().1;
    let field = field_control(slot, 1.0);
    a.settings_click(field.x + 12.0, field.y + 10.0);
    typed(&mut a, "12345");
    assert!(a.settings.as_ref().unwrap().profile_choice().is_none());

    a.settings_click(field.right() - 12.0, field.y + 10.0);
    let page = a.settings.as_ref().unwrap();
    let editor = page.editor.as_ref().unwrap();
    assert_eq!(editor.tabs[editor.tab].fields[editor.focus].value, "12345");
    let (focus, open) = page.profile_choice().unwrap();
    let (_, row) = layout.choice_rows(1.0, focus, open).into_iter().find(|(i, _)| open.options[*i].0 == "?").unwrap();
    a.settings_click(row.x + 12.0, row.y + 10.0);
    a.on_event(ctrl(Key::Enter));
    assert_eq!(saved(&mut a).unwrap().profiles[0].serial.as_deref(), Some("COM42:?"));
}

#[test]
fn new_profile_templates_prefill_the_editor_and_focus_the_connection_field() {
    let mut a = app();
    for (name, tab, initial) in [("Local shell", "Command", "a.exe"), ("SSH connection", "SSH", ""), ("Serial port", "Serial", ""), ("Telnet", "Network", "telnet://"), ("TCP", "Network", "tcp://")] {
        choose_template(&mut a, name);
        let editor = a.settings.as_ref().unwrap().editor.as_ref().unwrap();
        let active = &editor.tabs[editor.tab];
        assert_eq!((editor.focus, active.label.as_str(), active.fields[editor.focus].value.as_str()), (0, tab, initial));
        assert!(editor.tabs[0].fields[0].value.is_empty());
        if tab != "Command" { assert!(editor.tabs.iter().all(|t| t.label != "Command")); }
        key(&mut a, Key::Escape);
    }
    select_profile(&mut a, "");
    key(&mut a, Key::Enter);
    key(&mut a, Key::Escape);
    let page = a.settings.as_ref().unwrap();
    assert!(page.is_new_profile(page.current_item().unwrap()));
    assert!(page.editor.is_none() && a.overlay.is_none() && saved(&mut a).is_none());
}

#[test]
fn h_toggles_custom_and_detected_profiles_preserving_selection_and_query() {
    let mut a = app();
    let mut config = a.config().clone();
    config.profiles.push(Profile { id: "custom:mine".into(), name: "Mine".into(), command: "shell".into(), ..Default::default() });
    a.set_config(config);
    for name in ["Alpha", "Mine"] {
        select_profile(&mut a, name);
        for hidden in [true, false] {
            key(&mut a, Key::Char('H'));
            typed(&mut a, "h");
            let page = a.settings.as_ref().unwrap();
            let row = page.profile_at(page.current_item().unwrap()).unwrap();
            assert_eq!((row.name.as_str(), row.hidden, page.query.as_str()), (name, hidden, ""));
            let id = row.id.clone();
            let config = saved(&mut a).unwrap();
            assert_eq!(config.hidden_profiles.contains(&id), hidden && name == "Alpha");
            assert_eq!(config.profiles[0].hidden, hidden && name == "Mine");
            assert_eq!(a.find_profile(name).unwrap().id, id);
        }
    }
    let mut external = a.config().clone();
    external.hidden_profiles.push("local:b".into());
    a.set_config(external);
    assert!(a.settings.as_ref().unwrap().profiles.iter().any(|row| row.name == "Beta" && row.hidden));
}

#[test]
fn editing_a_layout_preserves_its_tree_and_config_only_fields() {
    let mut a = app();
    let original = Profile {
        id: "custom:pair".into(), name: "Pair".into(), hidden: true,
        on_exit: crate::profile::OnExit::Keep,
        layout: Some(crate::session::ProfileLayout::Split {
            vertical: false, ratio: 0.4,
            a: Box::new(crate::session::ProfileLayout::Pane { profile: "local:a".into(), cwd: Some("work".into()) }),
            b: Box::new(crate::session::ProfileLayout::Pane { profile: "local:b".into(), cwd: None }),
        }),
        ..Default::default()
    };
    let mut config = a.config().clone();
    config.profiles.push(original.clone());
    a.set_config(config);
    select_profile(&mut a, "Pair");
    key(&mut a, Key::Enter);
    typed(&mut a, " renamed");
    a.on_event(ctrl(Key::Enter));
    let updated = Profile { name: "Pair renamed".into(), ..original };
    assert_eq!(saved(&mut a).unwrap().profiles, [updated]);
}

#[test]
fn arguments_split_on_spaces_and_respect_quotes() {
    assert_eq!(split_args(r#"-NoLogo -File "C:\My Scripts\a.ps1" ''"#), ["-NoLogo", "-File", r"C:\My Scripts\a.ps1", ""]);
    assert_eq!(join_args(&["-d".into(), "My Distro".into()]), r#"-d "My Distro""#);
    let mixed = [r#"a'b"c"#, "two words", "", r"C:\tail\", r#"it's "a b"\"#].map(str::to_owned);
    assert_eq!(split_args(&join_args(&mixed)), mixed);
}

#[test]
fn a_detected_profile_is_copied_into_an_editable_custom_one() {
    let mut a = app();
    select_profile(&mut a, "Alpha");
    key(&mut a, Key::Enter);
    let editor = a.settings.as_ref().unwrap().editor.clone().expect("editor");
    assert_eq!(editor.tabs[0].fields[0].value, "Alpha (copy)");
    assert_eq!(editor.original, None);
    focus_field(&mut a, "Command", "Command line");
    typed(&mut a, r#" -l "two words""#);
    key(&mut a, Key::Tab);
    typed(&mut a, r"D:\work");
    a.on_event(ctrl(Key::Enter));
    let config = saved(&mut a).expect("saved");
    let custom = config.profiles.iter().find(|p| p.name == "Alpha (copy)").expect("custom profile");
    assert_eq!(custom.id, "custom:alpha-copy");
    assert_eq!(custom.args, ["-l", "two words"]);
    assert_eq!(custom.cwd.as_deref(), Some(r"D:\work"));
    assert_eq!(custom.env.get("MARK").map(String::as_str), Some("1"), "the copy keeps the environment of its source");
    let page = a.settings.as_ref().unwrap();
    assert!(page.editor.is_none());
    assert_eq!(page.profile_at(page.shown[page.selected]).map(|p| p.custom), Some(true));
}

#[test]
fn a_new_ssh_profile_needs_only_a_name_and_a_destination() {
    let mut a = app();
    choose_template(&mut a, "SSH connection");
    focus_field(&mut a, "General", "Name");
    a.on_event(ctrl(Key::Backspace));
    a.on_event(ctrl(Key::Enter));
    assert!(a.settings.as_ref().unwrap().editor.as_ref().unwrap().error.is_some(), "a connection still needs a destination");
    typed(&mut a, "Prod");
    focus_field(&mut a, "SSH", "SSH destination");
    typed(&mut a, "deploy@prod");
    a.on_event(ctrl(Key::Enter));
    let config = saved(&mut a).expect("saved");
    let p = config.profiles.iter().find(|p| p.name == "Prod").unwrap();
    assert_eq!((p.ssh.as_deref(), p.command.as_str(), p.args.as_slice()), (Some("deploy@prod"), "ssh", &["deploy@prod".to_owned()][..]));
    assert!(p.ssh_options.as_ref().and_then(|options| options.user.as_ref()).is_none());
}

#[test]
fn ssh_profile_password_actions_use_the_secret_store_and_do_not_save_config() {
    let mut a = app();
    choose_template(&mut a, "SSH connection");
    focus_field(&mut a, "SSH", "SSH destination");
    typed(&mut a, "host");
    focus_field(&mut a, "SSH", "User");
    typed(&mut a, "ann");
    focus_field(&mut a, "SSH", "Password");
    typed(&mut a, "example-secret");
    let editor = a.settings.as_ref().unwrap().editor.as_ref().unwrap();
    assert!(!format!("{editor:?}").contains("example-secret"));
    key(&mut a, Key::Enter);
    let effects = a.take_effects();
    assert!(effects.iter().any(|effect| matches!(effect, Effect::SshPassword { destination, options, value: Some(value), .. }
        if destination == "host" && options.user.as_deref() == Some("ann") && value == "example-secret")));
    assert!(effects.iter().all(|effect| !matches!(effect, Effect::SaveConfig(_))));
    focus_field(&mut a, "SSH", "Forget password");
    key(&mut a, Key::Enter);
    assert!(a.take_effects().iter().any(|effect| matches!(effect, Effect::SshPassword { destination, value: None, .. } if destination == "host")));
}

#[test]
fn ctrl_enter_makes_a_profile_the_default_and_custom_ones_delete_after_a_second_press() {
    let mut a = app();
    select_profile(&mut a, "Beta");
    a.on_event(ctrl(Key::Enter));
    assert_eq!(a.config().terminal.profile, "local:b");
    assert!(a.settings.as_ref().unwrap().profiles.iter().any(|p| p.default && p.name == "Beta"));
    key(&mut a, Key::Delete);
    assert!(a.settings.as_ref().unwrap().notice.as_deref().is_some_and(|n| n.contains("cannot be deleted")));

    choose_template(&mut a, "Local shell");
    focus_field(&mut a, "General", "Name");
    a.on_event(ctrl(Key::Backspace));
    typed(&mut a, "Mine");
    a.on_event(ctrl(Key::Enter));
    saved(&mut a);
    let page = a.settings.as_ref().unwrap();
    assert_eq!(page.profile_at(page.shown[page.selected]).map(|p| p.name.as_str()), Some("Mine"));
    a.on_event(ctrl(Key::Enter));
    key(&mut a, Key::Delete);
    assert!(a.config().profiles.iter().any(|p| p.name == "Mine"), "the first press only asks");
    key(&mut a, Key::Delete);
    assert!(!a.config().profiles.iter().any(|p| p.name == "Mine"));
    assert_eq!(a.config().terminal.profile, "", "deleting the default profile falls back to automatic");
}

#[test]
fn a_serial_profile_needs_only_a_name_and_a_port() {
    let mut a = app();
    choose_template(&mut a, "Serial port");
    typed(&mut a, "COM3");
    focus_field(&mut a, "Serial", "Speed");
    typed(&mut a, "9600");
    focus_field(&mut a, "Serial", "Data bits");
    key(&mut a, Key::Left);
    focus_field(&mut a, "Serial", "Parity");
    key(&mut a, Key::Right);
    focus_field(&mut a, "Serial", "Stop bits");
    key(&mut a, Key::Right);
    a.on_event(ctrl(Key::Enter));
    let config = saved(&mut a).expect("saved");
    let p = config.profiles.iter().find(|p| p.name == "COM3").unwrap();
    assert_eq!(p.serial.as_deref(), Some("COM3:9600,8N1"));
    assert!(a.settings.as_ref().unwrap().profiles.iter().any(|r| r.detail == "serial COM3:9600,8N1"));
}

#[test]
fn switching_the_language_setting_translates_the_interface_at_once() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "language");
    key(&mut a, Key::Right);
    assert_eq!(a.config().appearance.language, "en");
    assert_eq!(saved(&mut a).map(|c| c.appearance.language), Some("en".to_owned()));
    key(&mut a, Key::Right);
    assert_eq!(a.config().appearance.language, "ru");
    assert_eq!(crate::i18n::language(), "ru");
    let setting = a.settings.as_ref().unwrap().current().unwrap();
    assert_eq!(a.setting_text(setting), "Русский");
    assert_eq!(a.shortcut_text(Action::SplitLeft), "Нет");
    key(&mut a, Key::Right);
    assert_eq!(crate::i18n::language(), "en", "automatic follows the English system language");
    assert_eq!(a.setting_text(setting), "Automatic");
    crate::i18n::set_language("en");
}

#[test]
fn a_network_profile_stores_a_normalized_address() {
    let mut a = app();
    let mut config = a.config().clone();
    config.groups.push(crate::profile::ProfileGroup { name: "Network".into(), ..Default::default() });
    a.set_config(config);
    choose_template(&mut a, "TCP");
    typed(&mut a, "switch");
    a.on_event(ctrl(Key::Enter));
    assert!(a.settings.as_ref().unwrap().editor.as_ref().unwrap().error.is_some(), "TCP needs a port");
    a.on_event(ctrl(Key::Backspace));
    typed(&mut a, "switch.lan");
    focus_field(&mut a, "General", "Group");
    key(&mut a, Key::Right);
    a.on_event(ctrl(Key::Enter));
    let config = saved(&mut a).expect("saved");
    let p = config.profiles.iter().find(|p| p.name == "switch.lan:23").unwrap();
    assert_eq!(p.socket.as_deref(), Some("telnet://switch.lan:23"));
    assert_eq!(p.group.as_deref(), Some("Network"));
    assert!(!p.is_local());
    assert!(a.settings.as_ref().unwrap().profiles.iter().any(|r| r.detail == "telnet://switch.lan:23"));
}

#[test]
fn the_first_letter_typed_after_opening_settings_by_shortcut_is_kept() {
    let mut a = app();
    a.on_event(ctrl(Key::Char(',')));
    assert!(a.settings.is_some());
    a.on_event(Event::Char('n'));
    a.on_event(Event::Char('e'));
    assert_eq!(a.settings.as_ref().unwrap().query, "ne");
    let mut b = app();
    b.on_event(Event::Key { key: Key::Char(','), mods: Mods { ctrl: true, ..Mods::NONE }, pressed: true });
    b.on_event(Event::Char(','));
    b.on_event(Event::Char('\u{1b}'));
    b.on_event(Event::Char('x'));
    assert_eq!(b.settings.as_ref().unwrap().query, "x", "the chord's own character is still swallowed");
}

#[test]
fn the_profile_editor_validates_and_canonicalizes_the_color_scheme() {
    let mut a = app();
    let scheme = a.scheme_names().into_iter().find(|name| *name != name.to_ascii_lowercase()).unwrap();
    choose_template(&mut a, "Local shell");
    focus_field(&mut a, "Colors", "Color scheme");
    typed(&mut a, &scheme.to_ascii_lowercase());
    a.on_event(ctrl(Key::Enter));
    assert_eq!(saved(&mut a).unwrap().profiles[0].scheme.as_deref(), Some(scheme.as_str()));

    key(&mut a, Key::Enter);
    focus_field(&mut a, "Colors", "Color scheme");
    let editor = a.settings.as_ref().unwrap().editor.as_ref().unwrap();
    assert_eq!(editor.tabs[editor.tab].fields[editor.focus].value, scheme);
    a.on_event(ctrl(Key::Backspace));
    typed(&mut a, "unknown scheme");
    a.on_event(ctrl(Key::Enter));
    assert_eq!(a.settings.as_ref().unwrap().editor.as_ref().unwrap().error.as_deref(), Some(t("There is no color scheme with this name")));
    assert!(saved(&mut a).is_none());

    a.on_event(ctrl(Key::Backspace));
    a.on_event(ctrl(Key::Enter));
    assert_eq!(saved(&mut a).unwrap().profiles[0].scheme, None);
}

#[test]
fn color_scheme_page_lists_settings_then_custom_and_builtin_palettes() {
    let mut a = app();
    let mut config = a.config().clone();
    let colors = std::array::from_fn(|i| 0x204060 + i as u32);
    config.color_schemes.push(crate::config::CustomScheme {
        name: "Sample".into(), foreground: "#123456".into(), background: "#abcdef".into(),
        colors: colors.iter().map(|c| format!("#{c:06x}")).collect(), ..Default::default()
    });
    a.set_config(config);
    a.run(Action::Settings);
    a.show_settings_page(PAGES.iter().position(|p| *p == COLORS).unwrap());
    let page = a.settings.as_ref().unwrap();
    assert_eq!(PAGES[page.page], COLORS);
    assert!(page.shown.iter().all(|&i| page.section(i) == COLORS));
    let controls: Vec<_> = page.shown.iter().filter_map(|&i| SETTINGS.get(i)).map(|setting| setting.key).collect();
    assert_eq!(controls, ["appearance.color-scheme-mode", "colors.generate-256", "colors.harmonious"]);
    let rows: Vec<_> = page.shown.iter().filter_map(|&i| page.scheme_at(i)).collect();
    assert_eq!(rows.len(), a.scheme_names().len());
    assert_eq!(*rows[0], SchemeRow { name: "Sample".into(), custom: true, source: None, foreground: 0x123456, background: 0xabcdef, colors });
    assert!(rows[1..].iter().all(|s| !s.custom));
    assert!(rows[1..].windows(2).all(|pair| pair[0].name.to_lowercase() <= pair[1].name.to_lowercase()));
}

#[test]
fn colors_tab_updates_the_other_mode_without_changing_the_active_palette() {
    use crate::settings_colors::SchemeTab;
    let mut a = app();
    a.run(Action::Settings);
    a.show_settings_page(PAGES.iter().position(|&p| p == COLORS).unwrap());
    assert_eq!(a.settings.as_ref().unwrap().scheme_tab, SchemeTab::Dark);
    let bar = a.settings_geometry().unwrap().scheme_bar.unwrap();
    a.settings_click(bar.tabs[1].x + 1.0, bar.tabs[1].y + 1.0);
    assert_eq!(a.settings.as_ref().unwrap().scheme_tab, SchemeTab::Light);
    typed(&mut a, "Dracula");
    let page = a.settings.as_ref().unwrap();
    assert!(page.shown.iter().all(|&i| page.section(i) == COLORS));
    assert_eq!(page.scheme_at(page.current_item().unwrap()).unwrap().name, "Dracula");
    key(&mut a, Key::Enter);
    let config = saved(&mut a).unwrap();
    assert_eq!(config.appearance.light_color_scheme, "Dracula");
    assert_eq!(a.current_scheme(), "Aster Dark");
}

#[test]
fn deleting_a_custom_scheme_requires_confirmation_and_restores_defaults() {
    let mut a = app();
    let mut config = a.config().clone();
    config.appearance.color_scheme = "Sample".into();
    config.appearance.light_color_scheme = "sample".into();
    config.color_schemes.push(crate::config::CustomScheme {
        name: "Sample".into(), foreground: "#123456".into(), background: "#abcdef".into(),
        colors: vec!["#204060".into(); 16], ..Default::default()
    });
    a.set_config(config);
    a.run(Action::Settings);
    a.show_settings_page(PAGES.iter().position(|&p| p == COLORS).unwrap());
    key(&mut a, Key::Delete);
    assert_eq!(a.settings.as_ref().unwrap().deleting_scheme.as_deref(), Some("Sample"));
    assert!(saved(&mut a).is_none());
    key(&mut a, Key::Escape);
    assert!(a.settings.as_ref().unwrap().deleting_scheme.is_none());
    key(&mut a, Key::Delete);
    key(&mut a, Key::Enter);
    let config = saved(&mut a).unwrap();
    assert!(config.color_schemes.is_empty());
    assert_eq!(config.appearance.color_scheme, "Aster Dark");
    assert_eq!(config.appearance.light_color_scheme, "Aster Light");
    assert!(a.settings.as_ref().unwrap().scheme_item("Sample").is_none());
}

#[test]
fn scheme_controls_respect_future_config_read_only_mode() {
    let mut a = app();
    let mut config = Config::parse("version = 42").unwrap();
    config.appearance.color_scheme = "Sample".into();
    config.color_schemes.push(crate::config::CustomScheme {
        name: "Sample".into(), foreground: "#123456".into(), background: "#abcdef".into(),
        colors: vec!["#204060".into(); 16], ..Default::default()
    });
    a.set_config(config.clone());
    a.run(Action::Settings);
    a.show_settings_page(PAGES.iter().position(|&p| p == COLORS).unwrap());
    a.apply_settings_scheme("Aster Dark".into());
    key(&mut a, Key::Delete);
    key(&mut a, Key::Enter);
    key(&mut a, Key::F(2));
    assert!(a.settings.as_ref().unwrap().scheme_editor.is_some());
    typed(&mut a, "Edited scheme");
    a.on_event(ctrl(Key::Enter));
    assert_eq!(a.config(), &config);
    assert!(saved(&mut a).is_none());
}

#[test]
fn copying_a_builtin_scheme_saves_only_after_validation() {
    let mut a = app();
    a.run(Action::Settings);
    a.show_settings_page(PAGES.iter().position(|&p| p == COLORS).unwrap());
    let bar = a.settings_geometry().unwrap().scheme_bar.unwrap();
    a.settings_click(bar.delete.x + 1.0, bar.delete.y + 1.0);
    let editor = a.settings.as_ref().unwrap().scheme_editor.as_ref().unwrap();
    assert!(editor.original.is_none());
    assert_eq!(editor.fields[0], "Aster Dark (copy)");
    typed(&mut a, "aster light");
    a.on_event(ctrl(Key::Enter));
    assert_eq!(a.settings.as_ref().unwrap().scheme_editor.as_ref().unwrap().error, Some(crate::scheme_editor::SchemeIssue::DuplicateName));
    assert!(saved(&mut a).is_none());
    typed(&mut a, "New scheme");
    key(&mut a, Key::Tab);
    typed(&mut a, "#123456");
    a.on_event(ctrl(Key::Enter));
    let config = saved(&mut a).unwrap();
    assert_eq!(config.appearance.color_scheme, "New scheme");
    assert_eq!(config.color_schemes[0].foreground, "#123456");
    assert!(a.settings.as_ref().unwrap().scheme_editor.is_none());
    key(&mut a, Key::F(2));
    assert_eq!(a.settings.as_ref().unwrap().scheme_editor.as_ref().unwrap().original.as_deref(), Some("New scheme"));
    typed(&mut a, "Discarded");
    key(&mut a, Key::Escape);
    assert_eq!(a.config().color_schemes[0].name, "New scheme");
    assert!(saved(&mut a).is_none());
}

#[test]
fn enter_applies_and_saves_the_selected_color_scheme() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "Aster Light");
    let page = a.settings.as_ref().unwrap();
    assert_eq!(page.scheme_at(page.current_item().unwrap()).unwrap().name, "Aster Light");
    assert!(page.current().is_none() && page.current_shortcut().is_none());
    for k in [Key::Left, Key::Right, Key::Delete] {
        key(&mut a, k);
    }
    assert_eq!(a.config().appearance.color_scheme, "Aster Dark");
    assert!(saved(&mut a).is_none());
    key(&mut a, Key::Enter);
    assert_eq!(a.config().appearance.color_scheme, "Aster Light");
    assert_eq!(saved(&mut a).unwrap().appearance.color_scheme, "Aster Light");
}

#[test]
fn partial_scheme_name_is_searchable_from_another_page() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "sTeR Li");
    let page = a.settings.as_ref().unwrap();
    assert_eq!(PAGES[page.page], SYSTEM);
    assert!(page.shown.iter().any(|&i| page.scheme_at(i).is_some_and(|s| s.name == "Aster Light")));
}

#[test]
fn opening_color_scheme_page_selects_and_reveals_current_scheme() {
    let mut a = app();
    a.set_viewport(1000, 360, 1.0);
    a.chrome = a.tab_chrome();
    let name = a.scheme_names().pop().unwrap();
    let mut config = a.config().clone();
    config.appearance.color_scheme = name.to_lowercase();
    a.set_config(config);
    a.run(Action::Settings);
    let k = PAGES.iter().position(|&p| p == COLORS).unwrap();
    let r = a.settings_geometry().unwrap().pages[k];
    a.settings_click(r.x + 1.0, r.y + 1.0);
    let assert_current_visible = |a: &App| {
        let page = a.settings.as_ref().unwrap();
        assert_eq!(page.scheme_at(page.current_item().unwrap()).unwrap().name, name);
        let geo = a.settings_geometry().unwrap();
        let row = geo.rows.iter().find(|r| r.kind == RowKind::Item(page.selected)).unwrap();
        assert!(row.rect.y >= geo.list.y && row.rect.bottom() <= geo.list.bottom());
    };
    assert_current_visible(&a);
    a.on_event(ctrl(Key::PageDown));
    a.on_event(ctrl(Key::PageUp));
    assert_current_visible(&a);
}

#[test]
fn clicking_a_scheme_updates_only_the_active_light_mode() {
    let mut a = app();
    a.chrome = a.tab_chrome();
    let mut config = a.config().clone();
    config.appearance.color_scheme_mode = crate::config::SchemeMode::Light;
    let name = a.scheme_names().pop().unwrap();
    a.set_config(config);
    a.run(Action::Settings);
    typed(&mut a, &name);
    let page = a.settings.as_ref().unwrap();
    let n = page.shown.iter().position(|&i| page.scheme_at(i).is_some_and(|s| s.name == name)).unwrap();
    let row = a.settings_geometry().unwrap().rows.into_iter().find(|r| r.kind == RowKind::Item(n)).unwrap();
    a.settings_click(row.control.x + 1.0, row.control.y + 1.0);
    let config = saved(&mut a).unwrap();
    assert_eq!(config.appearance.light_color_scheme, name);
    assert_eq!(config.appearance.color_scheme, "Aster Dark");
}

#[test]
fn scheme_search_matches_settings_and_only_matching_palette_names() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "scheme");
    let page = a.settings.as_ref().unwrap();
    let settings: Vec<_> = page.shown.iter().filter_map(|&i| SETTINGS.get(i)).filter(|s| s.section == COLORS).map(|s| s.key).collect();
    assert_eq!(settings, ["appearance.color-scheme-mode", "appearance.color-scheme", "appearance.light-color-scheme", "colors.generate-256", "colors.harmonious"]);
    let rows: Vec<_> = page.shown.iter().filter_map(|&i| page.scheme_at(i)).collect();
    let expected = a.scheme_names().iter().filter(|name| name.to_lowercase().contains("scheme")).count();
    assert_eq!(rows.len(), expected);
    assert!(rows.iter().all(|s| s.name.to_lowercase().contains("scheme")));

    key(&mut a, Key::Escape);
    typed(&mut a, "Aster Dark");
    let page = a.settings.as_ref().unwrap();
    assert!(page.shown.iter().filter_map(|&i| page.scheme_at(i)).any(|s| s.name == "Aster Dark"));
}

fn select_vault(a: &mut App) {
    a.run(Action::Settings);
    a.show_settings_page(PAGES.iter().position(|&p| p == VAULT).unwrap());
}

fn request_vault_paste(a: &mut App) -> u64 {
    a.on_event(ctrl(Key::Char('V')));
    a.take_effects().into_iter().find_map(|effect| match effect {
        Effect::RequestVaultPaste(input) => Some(input),
        _ => None,
    }).expect("paste for this passphrase input")
}

fn unlocked_vault() -> VaultView {
    VaultView {
        configured: true, unlocked: true, entries: vec![VaultEntry { kind: "other".into(), key: "sudo:host".into(), label: "sudo:host".into() }], error: None,
    }
}

fn vault_labels(a: &App) -> Vec<&str> {
    let page = a.settings.as_ref().unwrap();
    page.shown.iter().map(|&item| match page.vault_at(item) {
        Some(row) => row.labels(&page.vault).0,
        None => SETTINGS[item].label,
    }).collect()
}

#[test]
fn vault_page_lists_controls_for_each_state_and_filters_labels() {
    let mut a = app();
    select_vault(&mut a);
    assert_eq!(vault_labels(&a), ["Set a master passphrase", "Stay unlocked for (minutes)"]);
    a.vault_state(VaultView { configured: true, error: Some(VaultIssue::WrongPassphrase), ..Default::default() });
    assert_eq!(vault_labels(&a), ["Show vault contents", "Manage vault", "Encrypt the entire config", "Stay unlocked for (minutes)"]);
    assert_eq!(a.settings.as_ref().unwrap().vault_notice(), Some(t("Wrong master passphrase")));
    a.vault_state(unlocked_vault());
    assert_eq!(vault_labels(&a), ["Manage vault", "Import key file", "sudo:host", "Encrypt the entire config", "Stay unlocked for (minutes)"]);
    typed(&mut a, "sudo:host");
    assert_eq!(vault_labels(&a), ["sudo:host"]);
    a.vault_state(VaultView { configured: true, ..Default::default() });
    assert!(!vault_labels(&a).contains(&"sudo:host"));
}

#[test]
fn vault_password_dialog_masks_input_and_retries_after_wrong_password() {
    use crate::vault_settings::VaultReply;
    let mut a = app();
    select_vault(&mut a);
    key(&mut a, Key::Enter);
    typed(&mut a, "Aя🔐");
    assert_eq!(a.settings.as_ref().unwrap().vault_dialog.as_ref().unwrap().display(0), "•••");
    assert!(!format!("{:?}", a.settings).contains("Aя"));
    key(&mut a, Key::Backspace);
    let input = request_vault_paste(&mut a);
    a.on_event(Event::VaultPasted { input, text: " ё ".into() });
    assert_eq!(a.settings.as_ref().unwrap().vault_dialog.as_ref().unwrap().display(0), "•••••");
    key(&mut a, Key::Enter);
    let effects = a.take_effects();
    assert!(effects.iter().any(|e| matches!(e, Effect::VaultCreate { passphrase } if passphrase == "Aя ё ")));
    assert!(!effects.iter().any(|e| matches!(e, Effect::SaveConfig(_) | Effect::Write { .. })));
    a.vault_state(unlocked_vault());
    a.vault_reply(a.vault_input_id, Ok(VaultReply::Applied));
    a.vault_state(VaultView { configured: true, ..Default::default() });
    select_vault(&mut a);
    key(&mut a, Key::Enter);
    typed(&mut a, "first");
    key(&mut a, Key::Enter);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::VaultUnlock { passphrase } if passphrase == "first")));
    typed(&mut a, "while waiting");
    a.vault_reply(a.vault_input_id, Err(VaultIssue::WrongPassphrase));
    let page = a.settings.as_ref().unwrap();
    assert!(page.query.is_empty());
    assert_eq!(page.vault_dialog.as_ref().unwrap().display(0), "");
    typed(&mut a, "retry");
    key(&mut a, Key::Enter);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::VaultUnlock { passphrase } if passphrase == "retry")));
}

#[test]
fn vault_show_copies_only_on_request_and_locks_away_the_value() {
    use crate::vault_settings::{VaultAction, VaultReply};
    let mut a = app();
    a.vault_state(unlocked_vault());
    select_vault(&mut a);
    typed(&mut a, "sudo:host");
    key(&mut a, Key::Enter);
    key(&mut a, Key::Enter);
    let request = a.take_effects().into_iter().find_map(|effect| match effect {
        Effect::VaultManage { request, action: VaultAction::Show(entry), .. } if entry.key == "sudo:host" => Some(request),
        _ => None,
    }).unwrap();
    a.vault_reply(request, Ok(VaultReply::Value("secret".into())));
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::SetClipboard(_))));
    a.on_event(ctrl(Key::Char('C')));
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::SetClipboard(value) if value == "secret")));
    a.vault_state(VaultView { configured: true, ..Default::default() });
    a.vault_reply(request, Ok(VaultReply::Value("stale".into())));
    assert!(a.settings.as_ref().unwrap().vault_dialog.is_none());
}

#[test]
fn vault_delete_and_erase_require_confirmation() {
    use crate::vault_settings::{VaultAction, VaultReply};
    let mut a = app();
    a.vault_state(unlocked_vault());
    select_vault(&mut a);
    typed(&mut a, "sudo:host");
    key(&mut a, Key::Delete);
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::VaultManage { .. })));
    key(&mut a, Key::Enter);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::VaultManage { action: VaultAction::Delete(entry), .. } if entry.key == "sudo:host")));
    a.vault_reply(a.vault_input_id, Ok(VaultReply::Applied));
    key(&mut a, Key::Escape);
    key(&mut a, Key::Enter);
    key(&mut a, Key::Down);
    key(&mut a, Key::Down);
    key(&mut a, Key::Enter);
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::VaultErase { .. })));
    key(&mut a, Key::Enter);
    assert!(a.take_effects().iter().any(|e| matches!(e, Effect::VaultErase { .. })));
}

#[test]
fn leaving_a_vault_dialog_discards_its_clipboard_request() {
    let mut a = app();
    a.open_default_tab();
    select_vault(&mut a);
    key(&mut a, Key::Enter);
    let input = request_vault_paste(&mut a);
    key(&mut a, Key::Escape);
    a.on_event(Event::VaultPasted { input, text: "clipboard passphrase".into() });
    assert!(a.settings.as_ref().unwrap().query.is_empty());
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::VaultCreate { .. } | Effect::Write { .. } | Effect::SaveConfig(_))));

    key(&mut a, Key::Enter);
    typed(&mut a, "discarded");
    let input = request_vault_paste(&mut a);
    a.close_settings();
    a.on_event(Event::VaultPasted { input, text: "clipboard passphrase".into() });
    assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::VaultCreate { .. } | Effect::Write { .. } | Effect::SaveConfig(_))));
}

fn select_tree_row(a: &mut App, kind: crate::profiles_page::TreeKind) {
    let page = a.settings.as_mut().unwrap();
    page.selected = page.shown.iter().position(|&i| page.tree_at(i).is_some_and(|r| r.kind == kind)).expect("visible profile tree row");
}

#[test]
fn group_modal_creates_edits_cancels_and_confirms_deletion() {
    use crate::profiles_page::{Header, TreeKind};
    let mut a = app();
    let mut c = a.config.clone();
    c.groups.push(crate::profile::ProfileGroup { id: "p".into(), name: "Parent".into(), ..Default::default() });
    a.set_config(c);
    a.open_profile_settings();
    select_tree_row(&mut a, TreeKind::NewProfile);
    key(&mut a, Key::Enter);
    key(&mut a, Key::Down);
    key(&mut a, Key::Enter);
    typed(&mut a, "East");
    key(&mut a, Key::Tab);
    key(&mut a, Key::Right);
    key(&mut a, Key::Tab);
    typed(&mut a, "#345678");
    key(&mut a, Key::Tab);
    typed(&mut a, "E");
    a.on_event(ctrl(Key::Enter));
    let mut config = saved(&mut a).unwrap();
    let g = &config.groups[1];
    assert_eq!((g.name.as_str(), g.parent.as_deref(), g.color.as_deref(), g.icon.as_deref()), ("East", Some("p"), Some("#345678"), Some("E")));
    let header = TreeKind::Header(Header::Group(g.id.clone()));
    config.profiles.push(Profile { id: "custom:east".into(), name: "Build".into(), group: Some("East".into()), ..Default::default() });
    a.set_config(config);
    select_tree_row(&mut a, header.clone());
    key(&mut a, Key::Char('E'));
    a.on_event(ctrl(Key::Backspace));
    typed(&mut a, "West");
    key(&mut a, Key::Escape);
    assert_eq!(a.config.groups[1].name, "East");
    key(&mut a, Key::Char('E'));
    a.on_event(ctrl(Key::Backspace));
    typed(&mut a, "West");
    a.on_event(ctrl(Key::Enter));
    assert_eq!(saved(&mut a).unwrap().profiles[0].group.as_deref(), Some("West"));
    select_tree_row(&mut a, header);
    key(&mut a, Key::Delete);
    assert!(saved(&mut a).is_none());
    key(&mut a, Key::Delete);
    let config = saved(&mut a).unwrap();
    assert_eq!(config.groups.len(), 1);
    assert_eq!(config.profiles[0].group, None);
}

#[test]
fn type_and_group_defaults_save_reset_and_reach_the_pending_launch() {
    use crate::profiles_page::{Header, TreeKind};
    let mut a = app();
    let mut config = a.config.clone();
    config.groups.push(crate::profile::ProfileGroup { id: "lab".into(), name: "Lab".into(), ..Default::default() });
    let source = Profile { id: "custom:host".into(), name: "Host".into(), ssh: Some("lab.internal".into()), group: Some("Lab".into()), ..Profile::default() };
    config.profiles.push(source.clone());
    a.set_config(config);
    a.open_profile_settings();
    a.show_profiles_tab(true);
    select_tree_row(&mut a, TreeKind::Defaults("ssh"));
    key(&mut a, Key::Enter);
    focus_field(&mut a, "SSH", "User");
    typed(&mut a, "operator");
    a.on_event(ctrl(Key::Enter));
    let config = saved(&mut a).unwrap();
    assert_eq!(config.profile_defaults["ssh"].ssh_options.as_ref().unwrap().user.as_deref(), Some("operator"));
    a.show_profiles_tab(false);
    let group_row = TreeKind::Header(Header::Group("lab".into()));
    select_tree_row(&mut a, group_row.clone());
    key(&mut a, Key::Char('E'));
    for _ in 0..5 { key(&mut a, Key::Tab); }
    key(&mut a, Key::Enter);
    focus_field(&mut a, "SSH", "Port");
    typed(&mut a, "2222");
    a.on_event(ctrl(Key::Enter));
    assert!(saved(&mut a).is_none());
    a.on_event(ctrl(Key::Enter));
    let config = saved(&mut a).unwrap();
    assert_eq!(config.groups[0].defaults["ssh"].ssh_options.as_ref().unwrap().port, Some(2222));
    let pane = a.new_pane_in(source, None);
    let ssh = a.panes[&pane].pending.as_ref().unwrap().ssh_options.as_ref().unwrap();
    assert_eq!((ssh.user.as_deref(), ssh.port), (Some("operator"), Some(2222)));

    select_tree_row(&mut a, group_row);
    key(&mut a, Key::Char('E'));
    for _ in 0..5 { key(&mut a, Key::Tab); }
    key(&mut a, Key::Delete);
    assert!(a.settings.as_ref().unwrap().group_editor.as_ref().unwrap().defaults.contains_key("ssh"));
    key(&mut a, Key::Delete);
    a.on_event(ctrl(Key::Enter));
    assert!(saved(&mut a).unwrap().groups[0].defaults.is_empty());
    a.show_profiles_tab(true);
    select_tree_row(&mut a, TreeKind::Defaults("ssh"));
    key(&mut a, Key::Delete);
    assert!(saved(&mut a).is_none());
    key(&mut a, Key::Delete);
    assert!(saved(&mut a).unwrap().profile_defaults.is_empty());
}

#[test]
fn shortcut_capture_accepts_buttons_wheels_and_released_modifiers() {
    let mut a = app();
    select_shortcut(&mut a, "new tab");
    key(&mut a, Key::Enter);
    a.on_event(Event::MouseDown { x: 210.0, y: 120.0, button: crate::input::MouseButton::Middle, clicks: 1 });
    assert_eq!(a.shortcut_text(Action::NewTab), "MouseMiddle");

    key(&mut a, Key::Insert);
    a.on_event(Event::Modifiers(Mods { ctrl: true, ..Mods::NONE }));
    a.on_event(Event::Wheel { dx: 0.0, dy: 1.0 });
    a.on_event(Event::Modifiers(Mods::NONE));
    assert_eq!(a.shortcut_text(Action::NewTab), "MouseMiddle, Ctrl-WheelUp");

    key(&mut a, Key::Insert);
    a.on_event(Event::Modifiers(ctrl_shift('A')));
    a.on_event(Event::Modifiers(Mods { ctrl: true, ..Mods::NONE }));
    assert!(a.settings.as_ref().unwrap().capture.is_some());
    a.on_event(Event::Modifiers(Mods::NONE));
    assert_eq!(a.shortcut_text(Action::NewTab), "MouseMiddle, Ctrl-WheelUp, Ctrl-Shift");
}

#[test]
fn hidden_palette_commands_can_be_edited_and_saved_as_an_action_id_list() {
    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "hidden palette commands");
    assert_eq!(a.settings.as_ref().unwrap().current().unwrap().key, "palette.hidden-commands");
    key(&mut a, Key::Enter);
    typed(&mut a, "close-other-tabs, profile.shell");
    key(&mut a, Key::Enter);
    assert_eq!(saved(&mut a).unwrap().palette.hidden_commands, ["close-other-tabs", "profile.shell"]);
}

#[test]
fn edits_global_environment_rows_and_reads_the_legacy_table() {
    let old = Config::parse("[terminal.environment]\nLEGACY = \"previous\"").unwrap();
    assert_eq!(old.terminal.environment["LEGACY"], "previous");
    assert!(old.to_text().contains("[terminal.env]"));

    let mut a = app();
    a.run(Action::Settings);
    typed(&mut a, "environment variables");
    assert_eq!(a.settings.as_ref().unwrap().current().unwrap().key, "terminal.env");
    key(&mut a, Key::Enter);
    typed(&mut a, "FIRST=$PATH");
    key(&mut a, Key::Enter);
    typed(&mut a, "SECOND=a=b");
    a.on_event(Event::Key { key: Key::Enter, mods: Mods { ctrl: true, ..Mods::NONE }, pressed: true });
    let config = saved(&mut a).unwrap();
    assert_eq!(config.terminal.environment["FIRST"], "$PATH");
    assert_eq!(config.terminal.environment["SECOND"], "a=b");
    assert!(config.terminal.refresh_environment);
}

#[test]
fn encrypted_startup_prompts_before_content_and_retries_without_leaking_input() {
    let mut a = app();
    a.prompt_config_unlock();
    let setting = SETTINGS.iter().find(|setting| matches!(setting.control, Control::ReadOnly(Info::ConfigStatus))).unwrap();
    assert_eq!(a.setting_text(setting), "Encrypted");
    assert!(!a.settings_open());
    typed(&mut a, "wrong");
    assert_eq!(a.vault_prompt.as_ref().unwrap().display(0), "•••••");
    key(&mut a, Key::Enter);
    let request = a.take_effects().into_iter().find_map(|effect| match effect {
        Effect::UnlockConfig { request, passphrase } if passphrase == "wrong" => Some(request),
        _ => None,
    }).unwrap();
    a.config_unlock_error(request, VaultIssue::WrongPassphrase);
    assert_eq!(a.vault_prompt.as_ref().unwrap().issue, Some(VaultIssue::WrongPassphrase));
    typed(&mut a, "retry");
    key(&mut a, Key::Enter);
    assert!(a.take_effects().iter().any(|effect| matches!(effect, Effect::UnlockConfig { passphrase, .. } if passphrase == "retry")));
    key(&mut a, Key::Escape);
    assert!(a.take_effects().iter().any(|effect| matches!(effect, Effect::CancelConfigUnlock)));
    assert!(a.vault_prompt.is_none());
    assert_eq!(a.setting_text(setting), "Encrypted");
}

#[test]
fn vault_change_masks_both_fields_and_discards_interrupted_input() {
    use crate::vault_settings::DialogKind;

    let mut a = app();
    select_vault(&mut a);
    for auto_lock in [false, true] {
        a.vault_state(unlocked_vault());
        a.open_vault_dialog(DialogKind::Change);
        typed(&mut a, "first");
        key(&mut a, Key::Enter);
        typed(&mut a, "second");
        let dialog = a.settings.as_ref().unwrap().vault_dialog.as_ref().unwrap();
        assert_eq!(dialog.focus, 1);
        assert_eq!(dialog.display(0), "•••••");
        assert_eq!(dialog.display(1), "••••••");
        assert!(!format!("{dialog:?}").contains("first"));
        assert!(!format!("{dialog:?}").contains("second"));
        if !auto_lock {
            key(&mut a, Key::Enter);
            assert!(a.take_effects().iter().any(|e| matches!(e, Effect::VaultChange { old, new } if old == "first" && new == "second")));
            continue;
        }

        let input = request_vault_paste(&mut a);
        a.vault_state(VaultView { configured: true, ..Default::default() });
        typed(&mut a, "remainder");
        a.on_event(Event::VaultPasted { input, text: "clipboard passphrase".into() });
        let page = a.settings.as_ref().unwrap();
        assert!(page.query.is_empty() && page.vault_dialog.is_none() && page.editing());
        key(&mut a, Key::Escape);
        assert!(!a.settings.as_ref().unwrap().editing());
        assert!(!a.take_effects().iter().any(|e| matches!(e, Effect::VaultChange { .. } | Effect::Write { .. } | Effect::SaveConfig(_))));
    }
}

#[test]
fn profile_advanced_tab_holds_picker_settings_and_type_defaults() {
    use crate::profiles_page::TreeKind;
    let mut a = app();
    a.open_profile_settings();
    let advanced = a.settings_geometry().unwrap().profile_tabs.unwrap()[1];
    a.settings_click(advanced.x + 4.0, advanced.y + 4.0);
    let page = a.settings.as_ref().unwrap();
    assert!(page.profiles_advanced);
    let keys: Vec<_> = page.shown.iter().filter_map(|&i| SETTINGS.get(i).map(|s| s.key)).collect();
    assert_eq!(keys, ["terminal.recent-profiles", "picker.show-builtin-profiles", "picker.quick-connect"]);
    assert!(page.shown.iter().any(|&i| page.tree_at(i).is_some_and(|row| row.kind == TreeKind::Defaults("ssh"))));
    assert!(page.shown.iter().all(|&i| page.profile_at(i).is_none()));
    a.on_event(ctrl(Key::Left));
    assert!(!a.settings.as_ref().unwrap().profiles_advanced);
}
