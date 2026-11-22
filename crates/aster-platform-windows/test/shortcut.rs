use super::*;
use crate::shell_link::APP_ID;
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

#[test]
fn saves_refreshes_and_removes_a_shortcut_with_the_app_id() {
    let _com = Apartment::enter().unwrap();
    let dir = std::env::temp_dir().join(format!("aster-shortcut-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let shortcut = dir.join("Aster.lnk");
    let first = std::env::current_exe().unwrap();
    let second = dir.join("aster-moved.exe");
    std::fs::copy(&first, &second).unwrap();
    assert!(!update(&shortcut, &first, None).unwrap());
    assert!(update(&shortcut, &first, Some(true)).unwrap());
    let link = load(&shortcut).unwrap();
    assert!(same_path(&target(&link).unwrap(), &first));
    let store: IPropertyStore = link.cast().unwrap();
    let app_id = unsafe { store.GetValue(&PKEY_AppUserModel_ID).unwrap() };
    assert_eq!(app_id.to_string(), unsafe { APP_ID.to_string().unwrap() });
    assert!(update(&shortcut, &second, None).unwrap());
    assert!(same_path(&target(&load(&shortcut).unwrap()).unwrap(), &second));
    let other_spelling = PathBuf::from(second.display().to_string().to_uppercase());
    assert!(same_path(&second, &other_spelling));
    assert!(!same_path(&first, &second));
    assert!(!update(&shortcut, &second, Some(false)).unwrap());
    assert!(!shortcut.exists());
    std::fs::remove_file(second).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
