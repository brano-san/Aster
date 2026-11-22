use super::*;

fn display(x: i32, primary: bool) -> Monitor {
    Monitor { bounds: (x, 0, 1920, 1080), work: (x, 0, 1920, 1040), primary }
}

#[test]
fn dock_screen_selects_current_primary_or_one_based_monitor() {
    let screens = [display(-1920, false), display(0, true)];
    assert_eq!(monitor_at(&screens, (-20, 100)), Some(0));
    assert_eq!(choose_monitor(&screens, Some(0), DockScreen::Current), Some(screens[0]));
    assert_eq!(choose_monitor(&screens, Some(0), DockScreen::Primary), Some(screens[1]));
    assert_eq!(choose_monitor(&screens, Some(1), DockScreen::Number(1)), Some(screens[0]));
    assert_eq!(choose_monitor(&screens, Some(1), DockScreen::Number(9)), Some(screens[1]));
    assert_eq!(choose_monitor(&[], None, DockScreen::Primary), None);
}

#[test]
fn offscreen_correction_preserves_accessible_windows_and_recovers_removed_screens() {
    let screens = [display(0, true)];
    assert_eq!(correct_offscreen((-100, 20, 800, 600), &screens), None);
    assert_eq!(correct_offscreen((-1800, 20, 800, 600), &screens), Some((0, 20, 800, 600)));
    assert_eq!(correct_offscreen((2400, 850, 1200, 700), &screens), Some((720, 340, 1200, 700)));
    assert_eq!(correct_offscreen((125, -100, 2000, 1400), &screens), Some((0, 0, 1920, 1040)));
    assert_eq!(correct_offscreen((2400, 850, 1200, 700), &[]), None);
}