use super::*;

#[test]
fn lines_append_and_a_large_log_moves_aside() {
    let dir = std::env::temp_dir().join(format!("aster-log-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let path = dir.join("aster.log");
    let mut log = open(Some(&path));
    log.write_all(b"first\n").unwrap();
    let mut again = open(Some(&path));
    again.write_all(b"second\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "first\nsecond\n");
    std::fs::write(&path, vec![b'x'; (LIMIT + 1) as usize]).unwrap();
    let mut fresh = open(Some(&path));
    fresh.write_all(b"new\n").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
    assert_eq!(std::fs::metadata(dir.join("aster.log.1")).unwrap().len(), LIMIT + 1);
    let _ = std::fs::remove_dir_all(&dir);
}
