use super::*;
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;

const MANIFEST_TEXT: &str = "id='hello'\nname='Hello'\nversion='1.2'\ncommand='bin/hello'\n";

fn zip(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for &(name, text) in entries {
        zip.start_file(name, SimpleFileOptions::default()).unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn validates_wrapped_package_and_uses_manifest_metadata() {
    let archive = zip(&[("hello-1.2/extension.toml", MANIFEST_TEXT), ("hello-1.2/bin/hello", "executable")]);
    assert_eq!(Package::read(&archive).unwrap().manifest.id, "hello");
}

#[test]
fn rejects_traversal_links_and_case_collisions() {
    let base = [("extension.toml", MANIFEST_TEXT), ("bin/hello", "executable")];
    for name in ["../outside", "C:/outside", "bin/../outside", "bin/name:stream", "BIN/hello", "BIN/different"] {
        let archive = zip(&[base[0], base[1], (name, "unexpected")]);
        assert!(Package::read(&archive).is_err(), "{name}");
    }
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    archive.start_file("extension.toml", SimpleFileOptions::default()).unwrap();
    archive.write_all(MANIFEST_TEXT.as_bytes()).unwrap();
    archive.add_symlink("bin/hello", "../outside", SimpleFileOptions::default()).unwrap();
    assert!(Package::read(&archive.finish().unwrap().into_inner()).is_err());
}

#[test]
fn rejects_missing_executable_and_invalid_manifest() {
    assert!(Package::read(&zip(&[("extension.toml", MANIFEST_TEXT)])).is_err());
    assert!(Package::read(&zip(&[("extension.toml", "id='INVALID'\nname='Invalid'\nversion='1'")])).is_err());
}

#[test]
fn sample_scheme_installs_without_an_executable() {
    let archive = zip(&[
        ("extension.toml", include_str!("../../aster-ext/test/fixtures/hello/extension.toml")),
        ("schemes/night.toml", include_str!("../../aster-ext/test/fixtures/hello/schemes/night.toml")),
    ]);
    let package = Package::read(&archive).unwrap();
    assert!(package.manifest.command.is_none());
    assert_eq!(package.manifest.schemes.len(), 1);
}

#[test]
fn update_must_be_newer_and_match_the_index() {
    let archive = zip(&[("extension.toml", MANIFEST_TEXT), ("bin/hello", "executable")]);
    let package = Package::read(&archive).unwrap();
    assert!(package.check_version(Some("1.1"), Some(("hello", "1.2"))).is_ok());
    assert!(package.check_version(Some("1.2"), None).is_err());
    assert!(package.check_version(Some("2.0"), None).is_err());
    assert!(package.check_version(None, Some(("other", "1.2"))).is_err());
    assert!(package.check_version(None, Some(("hello", "1.3"))).is_err());
}
