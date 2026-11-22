use super::*;
use crate::config::Config;
use crate::input::Event;
use crate::profile::Profile;
use std::sync::Arc;

#[test]
fn activity_waits_for_prior_quiet_and_announces_once_between_visits() {
    let start = Instant::now();
    let t = |seconds| start + Duration::from_secs(seconds);
    let silence = Duration::from_secs(10);
    let mut activity = Activity::new(start);
    assert!(!activity.observe(t(3), true, true, silence));
    assert!(!activity.observe(t(12), true, true, silence));
    assert!(!activity.observe(t(21), true, false, silence));
    assert!(activity.observe(t(22), true, true, silence));
    assert!(!activity.observe(t(32), true, true, silence));
    assert!(!activity.observe(t(33), false, false, silence));
    assert!(activity.observe(t(42), true, true, silence));
}

#[test]
fn background_notification_click_selects_the_tab_after_it_moves() {
    let mut config = Config::default();
    config.notifications.activity = true;
    let mut app = App::new(config, vec![Profile::new("local", "Shell", "shell", &[])], true, None, Arc::new(|| {}));
    app.open_default_tab();
    app.open_default_tab();
    let pane = app.tabs[0].focus;
    let tab = app.tabs[0].key;
    let now = Instant::now();
    app.poll_background_activity(now);
    app.terminal(pane).unwrap().lock().feed(b"finished");
    app.poll_background_activity(now + Duration::from_secs(12));
    assert!(app.take_effects().into_iter().any(|effect| matches!(effect, Effect::Notify(notification) if notification.tab == tab)));
    app.tabs.swap(0, 1);
    app.active = 0;
    app.on_event(Event::NotificationClicked(tab));
    assert_eq!(app.focused_pane(), Some(pane));
}