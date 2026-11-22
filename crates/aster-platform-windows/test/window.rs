use super::*;

#[test]
fn capture_recognizes_shortcut_editing_keys() {
    assert!(matches!(capture_key("Insert"), Event::Key { key: Key::Insert, mods: Mods::NONE, pressed: true }));
    assert!(matches!(capture_key("Shift+Delete"), Event::Key { key: Key::Delete, mods: Mods { shift: true, .. }, pressed: true }));
}

#[test]
fn clipboard_html_offsets_delimit_utf8_document_and_fragment() {
    let fragment = "<pre>界🙂&amp;</pre>";
    let data = clipboard_html(fragment);
    let offset = |field: &str| data.lines().find_map(|line| line.strip_prefix(field)).unwrap().parse::<usize>().unwrap();
    assert!(data[offset("StartHTML:")..offset("EndHTML:")].starts_with("<html>"));
    assert_eq!(&data[offset("StartFragment:")..offset("EndFragment:")], fragment);
    assert_eq!(&data[offset("EndHTML:")..], "\0");
}

#[test]
fn tray_routes_show_open_and_quit_with_or_without_windows() {
    use crate::tray::{Action, Command};
    for (windows, show, close) in [(None, Action::Open, Action::Exit), (Some(17), Action::Show(17), Action::Close(17))] {
        assert_eq!(Command::Show.resolve(windows), show);
        assert_eq!(Command::Quit.resolve(windows), close);
        assert_eq!(Command::NewWindow.resolve(windows), Action::Open);
    }
}

#[test]
fn tray_prefers_last_focus_among_surviving_windows() {
    let (tx, _events) = mpsc::channel();
    let queue = HubQueue { hwnd: 0, tx };
    let mut nodes: Vec<Rc<Node<()>>> = (1..=3).map(|id| Rc::new(Node {
        window: Win32Window::new(id, HWND::default(), queue.clone()), queue: RefCell::default(),
        ready: Cell::new(false), focused_at: Cell::new(None), shared: std::ptr::null(),
    })).collect();
    assert_eq!(recent_window(&nodes).unwrap().window.id, 3);
    let now = std::time::Instant::now();
    nodes[0].focused_at.set(Some(now));
    nodes[1].focused_at.set(Some(now + std::time::Duration::from_secs(1)));
    assert_eq!(recent_window(&nodes).unwrap().window.id, 2);
    nodes.remove(1);
    assert_eq!(recent_window(&nodes).unwrap().window.id, 1);
    nodes.clear();
    assert!(recent_window(&nodes).is_none());
}

#[test]
fn capture_can_hold_move_and_release_a_pane_drag() {
    assert_eq!(capture_batch("Down@10,15;Move@40,45;Up@80,85"), vec![
        Event::MouseMove { x: 10.0, y: 15.0 },
        Event::MouseDown { x: 10.0, y: 15.0, button: MouseButton::Left, clicks: 1 },
        Event::MouseMove { x: 40.0, y: 45.0 },
        Event::MouseUp { x: 80.0, y: 85.0, button: MouseButton::Left },
    ]);
}