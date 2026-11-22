use super::*;

#[test]
fn busy_progress_retargets_from_the_current_value_and_settles() {
    let now = Instant::now();
    let mut busy = Busy::default();
    let key = ProgressBar::Transfer(42);
    busy.begin(now, true);
    assert_eq!(busy.progress(key, 0.8), 0.0);
    assert!(busy.finish());

    busy.begin(now + SLIDE / 2, true);
    let before = busy.progress(key, 0.8);
    assert!(before > 0.0 && before < 0.8);
    assert_eq!(busy.progress(key, 0.5), before);
    assert!(busy.finish());

    busy.begin(now + SLIDE * 2, true);
    assert_eq!(busy.progress(key, 0.5), 0.5);
    assert!(!busy.finish());

    busy.begin(now + SLIDE * 2, false);
    assert_eq!(busy.progress(key, 0.9), 0.9);
    assert_eq!(busy.phase(), 0.0);
    assert!(!busy.finish());
}
