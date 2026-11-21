use super::*;
use aster_core::update::{base64_encode, render_manifest, LINUX, WINDOWS};
use ed25519_dalek::{Signer, SigningKey};

struct Temp(PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn temp(tag: &str) -> Temp {
    let d = std::env::temp_dir().join(format!("aster-update-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    Temp(d)
}

fn keys(seed: u8) -> (SigningKey, String) {
    let key = SigningKey::from_bytes(&[seed; 32]);
    let public = base64_encode(key.verifying_key().as_bytes());
    (key, public)
}

fn sign(key: &SigningKey, bytes: &[u8]) -> String {
    base64_encode(&key.sign(bytes).to_bytes())
}

#[cfg(windows)]
fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;
    let mut zip = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[cfg(not(windows))]
fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
    for (name, bytes) in files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, format!("./aster-9.0.0-x86_64-linux/{name}"), *bytes).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

fn full_archive() -> Vec<u8> {
    archive(&[(BINARY, b"new build"), ("LICENSE", b"MIT"), ("README.md", b"readme")])
}

fn release(signature: String) -> Release {
    Release { version: "9.0.0".into(), notes: String::new(), url: "https://example.com/aster.zip".into(), signature }
}

fn installed(dir: &Path) -> PathBuf {
    fs::create_dir_all(dir.join("data")).unwrap();
    fs::write(dir.join("data").join("config.toml"), b"[terminal]").unwrap();
    fs::write(dir.join(BINARY), b"old build").unwrap();
    dir.join(BINARY)
}

fn contents(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut all: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap()).filter(|e| e.path().is_file()).map(|e| (e.file_name().to_string_lossy().into_owned(), fs::read(e.path()).unwrap())).collect();
    all.sort();
    all
}

fn names(dir: &Path) -> Vec<String> {
    let mut all: Vec<String> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    all.sort();
    all
}

#[test]
fn signatures_verify_only_the_signed_bytes_with_the_right_key() {
    let (key, public) = keys(7);
    let (_, other) = keys(8);
    let bytes = b"aster release archive".to_vec();
    let signature = sign(&key, &bytes);
    assert_eq!(verify(&bytes, &signature, &public), Ok(()));
    let mut tampered = bytes.clone();
    tampered[0] ^= 1;
    assert!(verify(&tampered, &signature, &public).is_err());
    assert!(verify(&bytes, &signature, &other).is_err(), "another key");
    assert!(verify(&bytes, "not base64!", &public).is_err());
    assert!(verify(&bytes, &signature, "QUJD").is_err());
}

#[test]
fn shipped_public_key_is_a_valid_key() {
    let key: [u8; 32] = base64_decode(PUBLIC_KEY).unwrap().try_into().unwrap();
    assert!(VerifyingKey::from_bytes(&key).is_ok());
}

#[test]
fn check_with_a_fake_description_offers_only_newer_versions() {
    let manifest = |v: &str| render_manifest(v, "notes", "2026-11-21T00:00:00Z", &[(WINDOWS, "https://x/a.zip".into(), "QUJD".into()), (LINUX, "https://x/a.tar.gz".into(), "QUJD".into())]);
    assert_eq!(decide(manifest("0.9.0").as_bytes(), PLATFORM, "0.8.0", false).unwrap().map(|r| r.version), Some("0.9.0".into()));
    assert_eq!(decide(manifest("0.8.0").as_bytes(), PLATFORM, "0.8.0", false).unwrap(), None);
    assert_eq!(decide(manifest("0.7.1").as_bytes(), PLATFORM, "0.8.0", false).unwrap(), None);
    assert!(decide(b"<html>rate limited</html>", PLATFORM, "0.8.0", false).is_err());
}

#[test]
fn the_switch_variable_turns_network_checks_off() {
    std::env::set_var("ASTER_NO_UPDATE", "1");
    assert!(!network_allowed());
    std::env::remove_var("ASTER_NO_UPDATE");
}

#[test]
fn local_archives_are_read_with_progress_and_can_be_cancelled() {
    let t = temp("local");
    let file = t.0.join("aster.zip");
    fs::write(&file, vec![7u8; CHUNK * 2 + 5]).unwrap();
    let mut seen = Vec::new();
    let bytes = read_local(&file, &mut |r, total| {
        seen.push((r, total));
        true
    })
    .unwrap();
    assert_eq!(bytes.len(), CHUNK * 2 + 5);
    assert_eq!(seen.last(), Some(&((CHUNK * 2 + 5) as u64, Some((CHUNK * 2 + 5) as u64))));
    assert_eq!(read_local(&file, &mut |_, _| false), Err(error_cancelled()));
}

#[test]
fn install_swaps_the_binary_keeps_settings_and_the_old_copy_until_next_start() {
    let t = temp("swap");
    let exe = installed(&t.0);
    let (key, public) = keys(7);
    let bytes = full_archive();
    let (mut asked, mut installing) = (Vec::new(), 0);
    let r = install(&release(sign(&key, &bytes)), &exe, &public, &mut |url| {
        asked.push(url.to_owned());
        Ok(bytes.clone())
    }, &mut || installing += 1, None);
    assert_eq!(r, Ok(()));
    assert_eq!((asked, installing), (vec!["https://example.com/aster.zip".to_owned()], 1));
    assert_eq!(fs::read(&exe).unwrap(), b"new build");
    assert_eq!(fs::read(t.0.join(format!("{BINARY}.old"))).unwrap(), b"old build");
    assert!(!t.0.join("LICENSE").exists(), "only the program is replaced");
    assert_eq!(fs::read(t.0.join("data").join("config.toml")).unwrap(), b"[terminal]", "portable settings stay");
    remove_old(&exe);
    assert_eq!(names(&t.0), [BINARY.to_owned(), "data".to_owned()]);
}

#[test]
fn bad_signature_wrong_key_or_missing_binary_leave_the_install_untouched() {
    let t = temp("untouched");
    let exe = installed(&t.0);
    let before = contents(&t.0);
    let (key, public) = keys(7);
    let (stranger, _) = keys(9);
    let bytes = full_archive();
    let mut tampered = bytes.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xFF;
    let incomplete = archive(&[("README.md", b"y")]);
    let cases = [(tampered, sign(&key, &bytes)), (bytes.clone(), sign(&stranger, &bytes)), (incomplete.clone(), sign(&key, &incomplete))];
    for (served, signature) in cases {
        let r = install(&release(signature), &exe, &public, &mut |_| Ok(served.clone()), &mut || {}, None);
        assert!(matches!(r, Err(Failure::Failed(_))), "{r:?}");
        assert_eq!(contents(&t.0), before);
    }
    let r = install(&release(sign(&key, &incomplete)), &exe, &public, &mut |_| Ok(incomplete.clone()), &mut || {}, None);
    assert_eq!(r, Err(Failure::Failed(error_missing(BINARY))));
    let r = install(&release(sign(&key, &bytes)), &exe, &public, &mut |_| Err("offline".into()), &mut || {}, None);
    assert_eq!(r, Err(Failure::Failed("offline".into())));
    assert_eq!(contents(&t.0), before);
}

#[test]
fn missing_folder_is_reported_as_not_writable() {
    let t = temp("readonly");
    let exe = t.0.join("gone").join(BINARY);
    let r = install(&release(String::new()), &exe, PUBLIC_KEY, &mut |_| panic!("no download into a folder Aster cannot write"), &mut || {}, None);
    assert_eq!(r, Err(Failure::ReadOnly(t.0.join("gone").display().to_string())));
}

#[test]
fn running_exe_keeps_its_own_name() {
    let t = temp("renamed");
    let exe = t.0.join(if cfg!(windows) { "aster-dev.exe" } else { "aster-dev" });
    fs::write(&exe, b"old build").unwrap();
    let (key, public) = keys(7);
    let bytes = full_archive();
    install(&release(sign(&key, &bytes)), &exe, &public, &mut |_| Ok(bytes.clone()), &mut || {}, None).unwrap();
    assert_eq!(fs::read(&exe).unwrap(), b"new build");
    assert!(!t.0.join(BINARY).exists());
}

#[test]
fn failed_swap_rolls_back_what_was_already_replaced() {
    let t = temp("rollback");
    fs::write(t.0.join("a"), b"old a").unwrap();
    fs::write(t.0.join("b"), b"old b").unwrap();
    for k in 0..=OLD_SLOTS {
        let blocked = old_name(&t.0.join("b"), k);
        fs::create_dir(&blocked).unwrap();
        fs::write(blocked.join("busy"), b"").unwrap();
    }
    let r = swap(&t.0, &[("a".into(), b"new a".to_vec()), ("b".into(), b"new b".to_vec())]);
    assert!(r.is_err());
    assert_eq!(fs::read(t.0.join("a")).unwrap(), b"old a");
    assert_eq!(fs::read(t.0.join("b")).unwrap(), b"old b");
    assert!(!t.0.join("a.new").exists() && !t.0.join("b.new").exists() && !t.0.join("a.old").exists());
}

#[test]
fn a_folder_aster_cannot_write_is_updated_through_an_elevated_copy_of_itself() {
    let t = temp("elevated");
    let exe = installed(&t.0);
    let (key, public) = keys(7);
    let bytes = full_archive();
    let signature = sign(&key, &bytes);
    let mut handed = Vec::new();
    let mut elevate = |archive: &[u8], signature: &str| {
        handed.push((archive.to_vec(), signature.to_owned()));
        Ok(true)
    };
    let r = install_into(&release(signature.clone()), &exe, &public, &mut |_| Ok(bytes.clone()), &mut || {}, Some(&mut elevate), false);
    assert_eq!(r, Ok(()));
    assert_eq!(handed, [(bytes.clone(), signature.clone())], "the verified download goes to the elevated copy");
    assert_eq!(fs::read(&exe).unwrap(), b"old build", "this process does not touch the folder itself");
    let archive = t.0.join("download.zip");
    fs::write(&archive, &bytes).unwrap();
    install_elevated(&archive, &signature, &public, &exe, &mut |p| panic!("{} could be removed", p.display())).unwrap();
    assert_eq!(fs::read(&exe).unwrap(), b"new build");
}

#[test]
fn a_declined_prompt_or_a_tampered_download_leaves_the_folder_alone() {
    let t = temp("declined");
    let exe = installed(&t.0);
    let before = contents(&t.0);
    let (key, public) = keys(7);
    let bytes = full_archive();
    let r = install_into(&release(sign(&key, &bytes)), &exe, &public, &mut |_| Ok(bytes.clone()), &mut || {}, Some(&mut |_: &[u8], _: &str| Ok(false)), false);
    assert_eq!(r, Err(Failure::ReadOnly(t.0.display().to_string())), "declining the prompt explains the folder is read-only");
    let r = install_into(&release(sign(&key, &bytes)), &exe, &public, &mut |_| panic!("no download without a way to write"), &mut || {}, None, false);
    assert_eq!(r, Err(Failure::ReadOnly(t.0.display().to_string())));
    let archive = t.0.join("download.zip");
    let mut tampered = bytes.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 0xFF;
    fs::write(&archive, &tampered).unwrap();
    assert!(install_elevated(&archive, &sign(&key, &bytes), &public, &exe, &mut |p| panic!("{} scheduled", p.display())).is_err(), "the elevated copy checks the signature again");
    fs::remove_file(&archive).unwrap();
    assert_eq!(contents(&t.0), before);
}

#[test]
fn an_elevated_update_removes_old_copies_and_leaves_the_ones_in_use_for_the_next_restart() {
    let t = temp("elevated-old");
    let exe = installed(&t.0);
    let (key, public) = keys(7);
    let bytes = full_archive();
    let in_use = t.0.join(format!("{BINARY}.old"));
    fs::create_dir_all(in_use.join("held")).unwrap();
    let archive = std::env::temp_dir().join(format!("aster-update-elevated-old-{}.zip", std::process::id()));
    fs::write(&archive, &bytes).unwrap();
    let mut scheduled = Vec::new();
    let r = install_elevated(&archive, &sign(&key, &bytes), &public, &exe, &mut |p| scheduled.push(p.to_path_buf()));
    fs::remove_file(&archive).unwrap();
    assert_eq!(r, Ok(()));
    assert_eq!(scheduled, std::slice::from_ref(&in_use), "only what cannot be removed now waits for the restart");
    assert_eq!(fs::read(&exe).unwrap(), b"new build");
    assert_eq!(names(&t.0), [BINARY.to_owned(), format!("{BINARY}.old"), "data".to_owned()]);
}

#[test]
fn a_rejected_elevated_update_neither_removes_nor_schedules_anything() {
    let t = temp("elevated-rejected");
    let exe = installed(&t.0);
    let leftover = t.0.join(format!("{BINARY}.old"));
    fs::write(&leftover, b"older build").unwrap();
    let (_, public) = keys(7);
    let (stranger, _) = keys(9);
    let bytes = full_archive();
    let archive = std::env::temp_dir().join(format!("aster-update-elevated-rejected-{}.zip", std::process::id()));
    fs::write(&archive, &bytes).unwrap();
    let mut scheduled = Vec::new();
    let r = install_elevated(&archive, &sign(&stranger, &bytes), &public, &exe, &mut |p| scheduled.push(p.to_path_buf()));
    fs::remove_file(&archive).unwrap();
    assert!(r.is_err());
    assert!(scheduled.is_empty());
    assert_eq!(fs::read(&leftover).unwrap(), b"older build", "nothing is cleaned before the signature is trusted");
    assert_eq!(fs::read(&exe).unwrap(), b"old build");
}
