use super::*;

fn input(folder: &str, id: &str) -> (PathBuf, String) {
    (PathBuf::from(folder), format!("id='{id}'\nname='Example'\nversion='1'"))
}

#[test]
fn scans_manifests_and_resolves_disabled_state() {
    let registry = Registry::scan(vec![input("b", "b"), input("a", "a")], &["b".into()]);
    assert!(registry.errors.is_empty());
    assert_eq!(registry.entries[0].manifest.id, "a");
    assert!(registry.entries[0].enabled);
    assert!(!registry.entries[1].enabled);
}

#[test]
fn rejects_every_copy_of_duplicate_ids_and_reports_invalid_files() {
    let registry = Registry::scan(vec![input("b", "same"), input("a", "same"), input("c", "INVALID"), input("d", "unique")], &[]);
    assert_eq!(registry.entries.len(), 1);
    assert_eq!(registry.entries[0].manifest.id, "unique");
    assert_eq!(registry.errors.len(), 3);
    assert!(registry.errors.iter().filter(|e| e.error.contains("same")).count() == 2);
}
