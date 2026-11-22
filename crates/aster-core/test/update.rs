use super::*;

fn manifest(platforms: &str) -> String {
    format!(r#"{{"version":"0.9.0","notes":" Faster scrolling \n","pub_date":"2026-11-21T10:00:00Z","platforms":{{{platforms}}}}}"#)
}

#[test]
fn manifest_gives_the_release_for_this_platform() {
    let text = manifest(r#""windows-x86_64":{"url":"https://example.com/aster-0.9.0-x86_64-windows.zip","signature":"AAAA"},"linux-x86_64":{"url":"https://example.com/aster.tar.gz","signature":"QUJD"}"#);
    let win = parse_manifest(&text, WINDOWS).unwrap();
    assert_eq!(win, Release { version: "0.9.0".into(), notes: "Faster scrolling".into(), url: "https://example.com/aster-0.9.0-x86_64-windows.zip".into(), signature: "AAAA".into() });
    assert_eq!(parse_manifest(&text, LINUX).unwrap().url, "https://example.com/aster.tar.gz");
}

#[test]
fn manifest_without_this_platform_or_broken_is_an_error() {
    let only_linux = manifest(r#""linux-x86_64":{"url":"https://example.com/a.tar.gz","signature":"QUJD"}"#);
    let err = parse_manifest(&only_linux, WINDOWS).unwrap_err();
    assert!(err.contains("0.9.0") && err.contains(WINDOWS), "{err}");
    assert!(parse_manifest("not json", WINDOWS).is_err());
    assert!(parse_manifest(r#"{"version":"soon","platforms":{}}"#, WINDOWS).is_err());
    let plain_http = manifest(r#""windows-x86_64":{"url":"http://example.com/a.zip","signature":"QUJD"}"#);
    assert!(parse_manifest(&plain_http, WINDOWS).is_err(), "only https downloads");
    let bad_signature = manifest(r#""windows-x86_64":{"url":"https://example.com/a.zip","signature":"*!"}"#);
    assert!(parse_manifest(&bad_signature, WINDOWS).is_err());
}

#[test]
fn local_files_are_allowed_only_from_a_local_description() {
    let text = manifest(r#""windows-x86_64":{"url":"file:///C:/updates/aster.zip","signature":"QUJD"}"#);
    assert!(parse_manifest(&text, WINDOWS).is_err());
    assert_eq!(parse_manifest_since(&text, WINDOWS, "0.1.0", true).unwrap().url, "file:///C:/updates/aster.zip");
    assert_eq!(local_path("file:///C:/updates/aster.zip"), Some("C:/updates/aster.zip"));
    assert_eq!(local_path("file:///tmp/aster.tar.gz"), Some("/tmp/aster.tar.gz"));
    assert_eq!(local_path("https://example.com/a.zip"), None);
}

#[test]
fn skipped_releases_show_each_version_once_newest_first() {
    let text = r#"{"version":"0.11.0","notes":"latest","history":[
        {"version":"v0.9.0","notes":"nine"},{"version":"0.8.0","notes":"already installed"},
        {"version":"0.11.0","notes":"duplicate"},{"version":"0.10.0","notes":"ten"},
        {"version":"0.12.0","notes":"future"},{"version":"invalid","notes":"bad"}],
        "platforms":{"linux-x86_64":{"url":"https://x/a","signature":"QUJD"}}}"#;
    let notes = |installed| parse_manifest_since(text, LINUX, installed, false).unwrap().notes;
    assert_eq!(notes("0.8.0"), "## 0.11.0\n\nlatest\n\n## 0.10.0\n\nten\n\n## 0.9.0\n\nnine");
    assert_eq!(notes("0.10.0"), "latest");
}

#[test]
fn rendered_manifest_parses_back() {
    let text = render_manifest("1.2.3", "Notes\nline two", "2026-11-21T10:00:00Z", &[(WINDOWS, "https://x/a.zip".into(), "QUJD".into()), (LINUX, "https://x/a.tar.gz".into(), "REVG".into())]);
    let r = parse_manifest(&text, LINUX).unwrap();
    assert_eq!((r.version.as_str(), r.notes.as_str(), r.url.as_str(), r.signature.as_str()), ("1.2.3", "Notes\nline two", "https://x/a.tar.gz", "REVG"));
    assert!(text.contains("\"pub_date\": \"2026-11-21T10:00:00Z\""));
}

#[test]
fn release_assets_map_to_platform_keys() {
    assert_eq!(platform_of_asset("aster-0.9.0-x86_64-windows.zip"), Some(WINDOWS));
    assert_eq!(platform_of_asset("aster-0.9.0-x86_64-linux.tar.gz"), Some(LINUX));
    assert_eq!(platform_of_asset("aster_0.9.0_amd64.deb"), None);
    assert_eq!(platform_of_asset("other-0.9.0-x86_64-windows.zip"), None);
}

#[test]
fn only_strictly_newer_versions_count() {
    assert!(newer("0.8.1", "0.8.0"));
    assert!(newer("0.10.0", "0.9.9"), "numeric, not text order");
    assert!(newer("1.0", "0.99.99"));
    assert!(newer("v0.9.0", "0.8.0"));
    assert!(newer("0.9.0", "0.9.0-rc.1"), "a release beats its pre-release");
    assert!(!newer("0.9.0-rc.1", "0.9.0"));
    assert!(!newer("0.8.0", "0.8.0"));
    assert!(!newer("0.8.0.0", "0.8.0"));
    assert!(!newer("0.7.9", "0.8.0"));
    assert!(!newer("garbage", "0.8.0"));
    assert!(!newer("1.0.0", "garbage"));
}

#[test]
fn base64_round_trips_and_rejects_noise() {
    for bytes in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", &[0, 255, 128, 7]] {
        let text = base64_encode(bytes);
        assert_eq!(base64_decode(&text).as_deref(), Some(bytes), "{text}");
    }
    assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    assert_eq!(base64_encode(b"fo"), "Zm8=");
    assert_eq!(base64_decode("Zm8").as_deref(), Some(&b"fo"[..]));
    assert_eq!(base64_decode("Zm9v YmFy"), None);
    assert_eq!(base64_decode("Zm9=x"), None);
}

#[test]
fn percent_needs_a_known_total() {
    assert_eq!(percent(42, Some(100)), Some(42));
    assert_eq!(percent(150, Some(100)), Some(100));
    assert_eq!(percent(5, None), None);
    assert_eq!(percent(5, Some(0)), None);
}

mod notice {
    use super::super::*;
    use crate::app::{App, Effect};
    use crate::config::Config;
    use crate::input::{Event, MouseButton};
    use aster_gfx::text::{FontMetrics, FontStyle, Gfx, GlyphBitmap, GlyphKey, ShapedGlyph, ShapedText, TextBackend};
    use std::sync::Arc;

    struct Mono;

    impl TextBackend for Mono {
        fn shape(&mut self, text: &str, _: FontStyle, out: &mut ShapedText) {
            out.glyphs.clear();
            for (i, (b, c)) in text.char_indices().enumerate() {
                out.glyphs.push(ShapedGlyph { font: 0, id: c as u16, x: i as f32 * 8.0, y: 0.0, cluster: b as u32, cluster_x: i as f32 * 8.0 });
            }
            out.width = out.glyphs.len() as f32 * 8.0;
        }
        fn rasterize(&mut self, _: GlyphKey, out: &mut GlyphBitmap) -> bool {
            *out = GlyphBitmap { left: 0, top: 10, width: 6, height: 10, data: vec![255; 60], color: false };
            true
        }
        fn metrics(&mut self, _: FontStyle) -> FontMetrics {
            FontMetrics { ascent: 12.0, descent: 4.0 }
        }
    }

    fn app_with(config: Config) -> (App, Gfx<Mono>) {
        let mut a = App::new(config, Vec::new(), true, None, Arc::new(|| {}));
        a.set_viewport(1000, 700, 1.0);
        let mut g = Gfx::new(Mono, 1024);
        a.frame(&mut g, false);
        a.take_effects();
        (a, g)
    }

    fn app() -> (App, Gfx<Mono>) {
        app_with(Config::default())
    }

    fn release(notes: &str) -> Release {
        Release { version: "9.1.0".into(), notes: notes.into(), url: "https://example.com/aster.zip".into(), signature: "QUJD".into() }
    }

    fn requests(a: &mut App) -> Vec<UpdateRequest> {
        a.take_effects().into_iter().filter_map(|e| if let Effect::Update(r) = e { Some(r) } else { None }).collect()
    }

    fn click(a: &mut App, g: &mut Gfx<Mono>, button: NoticeButton) {
        a.frame(g, false);
        let r = a.update_geometry().unwrap().buttons.into_iter().find(|(b, _)| *b == button).map(|(_, r)| r).unwrap_or_else(|| panic!("{button:?} is not shown"));
        let (x, y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        a.on_event(Event::MouseDown { x, y, button: MouseButton::Left, clicks: 1 });
        a.on_event(Event::MouseUp { x, y, button: MouseButton::Left });
    }

    #[test]
    fn the_start_check_follows_the_setting() {
        let (mut a, _) = app();
        a.check_updates_on_start();
        assert_eq!(requests(&mut a), [UpdateRequest::Check { manual: false }]);
        let mut config = Config::default();
        config.updates.check_automatically = false;
        let (mut off, _) = app_with(config);
        off.check_updates_on_start();
        assert!(requests(&mut off).is_empty());
    }

    #[test]
    fn a_found_release_shows_a_notice_and_install_downloads_it() {
        let (mut a, mut g) = app();
        a.on_update(UpdateEvent::Checked { manual: false, found: Ok(Some(release("## 9.1.0\n\n- Faster"))) });
        let notice = a.update_notice().unwrap();
        assert_eq!((notice.stage, notice.buttons()), (Stage::Offer, vec![NoticeButton::Install, NoticeButton::Notes, NoticeButton::Later]));
        assert!(notice.title().contains("9.1.0"));
        click(&mut a, &mut g, NoticeButton::Install);
        assert_eq!(requests(&mut a), [UpdateRequest::Install(release("## 9.1.0\n\n- Faster"))]);
        assert_eq!(a.update_notice().unwrap().stage, Stage::Downloading);
        a.on_update(UpdateEvent::Progress { received: 50, total: Some(200) });
        let notice = a.update_notice().unwrap();
        assert_eq!((notice.fraction(), notice.status()), (Some(0.25), "Downloading… 25%".to_owned()));
        a.on_update(UpdateEvent::Installing);
        assert_eq!(a.update_notice().unwrap().stage, Stage::Installing);
        assert!(a.update_notice().unwrap().buttons().is_empty());
    }

    #[test]
    fn cancel_waits_for_the_worker_and_offers_a_retry() {
        let (mut a, mut g) = app();
        a.on_update(UpdateEvent::Checked { manual: false, found: Ok(Some(release(""))) });
        assert_eq!(a.update_notice().unwrap().buttons(), [NoticeButton::Install, NoticeButton::Later]);
        click(&mut a, &mut g, NoticeButton::Install);
        requests(&mut a);
        click(&mut a, &mut g, NoticeButton::Cancel);
        assert_eq!(requests(&mut a), [UpdateRequest::Cancel]);
        assert!(a.update_notice().unwrap().buttons().is_empty() && a.update_notice().unwrap().status() == "Cancelling…");
        a.on_update(UpdateEvent::Failed(error_cancelled()));
        let notice = a.update_notice().unwrap();
        assert_eq!((notice.stage, notice.status()), (Stage::Failed, error_cancelled()));
        click(&mut a, &mut g, NoticeButton::Retry);
        assert!(matches!(requests(&mut a)[..], [UpdateRequest::Install(_)]));
        a.on_update(UpdateEvent::ReadOnly("C:\\Program Files\\Aster".into()));
        assert_eq!(a.update_notice().unwrap().buttons(), [NoticeButton::OpenReleases, NoticeButton::Later]);
        click(&mut a, &mut g, NoticeButton::OpenReleases);
        assert!(a.take_effects().iter().any(|e| matches!(e, Effect::Open(url) if url == RELEASES_URL)));
        assert!(a.update_notice().is_none());
    }

    #[test]
    fn later_hides_the_notice_and_a_new_check_does_not_interrupt_a_download() {
        let (mut a, mut g) = app();
        a.on_update(UpdateEvent::Checked { manual: false, found: Ok(Some(release(""))) });
        click(&mut a, &mut g, NoticeButton::Later);
        assert!(a.update_notice().is_none());
        a.on_update(UpdateEvent::Checked { manual: true, found: Ok(Some(release(""))) });
        click(&mut a, &mut g, NoticeButton::Install);
        assert_eq!(requests(&mut a), [UpdateRequest::Install(release(""))]);
        a.on_update(UpdateEvent::Checked { manual: true, found: Ok(Some(release("other"))) });
        assert_eq!(a.update_notice().unwrap().stage, Stage::Downloading);
        a.run(crate::keymap::Action::CheckForUpdates);
        assert!(requests(&mut a).is_empty(), "no second check while downloading");
    }

    #[test]
    fn a_manual_check_says_when_aster_is_up_to_date_or_the_check_failed() {
        let (mut a, _) = app();
        a.run(crate::keymap::Action::CheckForUpdates);
        assert_eq!(requests(&mut a), [UpdateRequest::Check { manual: true }]);
        assert_eq!(a.tab_toast.as_ref().map(|t| t.text.as_str()), Some("Checking for updates…"));
        a.on_update(UpdateEvent::Checked { manual: true, found: Ok(None) });
        assert_eq!(a.tab_toast.as_ref().map(|t| t.text.clone()), Some(format!("Aster {CURRENT} is up to date")));
        a.on_update(UpdateEvent::Checked { manual: true, found: Err("offline".into()) });
        assert_eq!(a.tab_toast.as_ref().map(|t| t.text.as_str()), Some("Could not check for updates: offline"));
        a.tab_toast = None;
        a.on_update(UpdateEvent::Checked { manual: false, found: Ok(None) });
        a.on_update(UpdateEvent::Checked { manual: false, found: Err("offline".into()) });
        assert!(a.tab_toast.is_none() && a.update_notice().is_none(), "automatic checks stay silent");
    }

    #[test]
    fn release_notes_open_wrapped_inside_the_notice_and_scroll() {
        let (mut a, mut g) = app();
        let notes = (1..=30).map(|n| format!("- change number {n} with a long description that needs more than one line to fit")).collect::<Vec<_>>().join("\n");
        a.on_update(UpdateEvent::Checked { manual: false, found: Ok(Some(release(&format!("## 9.1.0\n\n{notes}")))) });
        click(&mut a, &mut g, NoticeButton::Notes);
        a.frame(&mut g, false);
        let notice = a.update_notice().unwrap();
        assert!(notice.notes_shown() && notice.wrapped.len() > 30);
        assert_eq!(notice.wrapped[0], "9.1.0");
        assert!(notice.wrapped[2].starts_with("\u{2022} change number 1"));
        let geo = a.update_geometry().unwrap();
        assert!(a.chrome.content.contains(geo.frame.x, geo.frame.y) && geo.frame.right() <= a.chrome.content.right());
        assert_eq!(notice.visible_notes().len(), NOTES_VISIBLE);
        let notes_rect = geo.notes.unwrap();
        a.on_event(Event::MouseMove { x: notes_rect.x + 5.0, y: notes_rect.y + 5.0 });
        a.on_event(Event::Wheel { dx: 0.0, dy: -1.0 });
        assert_eq!(a.update_notice().unwrap().notes_scroll, 3);
        assert_eq!(a.update_notice().unwrap().button_widths.len(), 3);
        assert_eq!(a.update_notice().unwrap().buttons()[1].label(true), "Hide notes");
    }

    #[test]
    fn markdown_notes_become_plain_lines() {
        assert_eq!(note_lines("## 1.0\n\n\n* **Bold** item\n- `code` item\nplain\n\n"), ["1.0", "", "\u{2022} Bold item", "\u{2022} code item", "plain"]);
    }
}
