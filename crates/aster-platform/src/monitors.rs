use aster_core::config::DockScreen;

pub type Bounds = (i32, i32, i32, i32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Monitor {
    pub bounds: Bounds,
    pub work: Bounds,
    pub primary: bool,
}

pub fn choose_monitor(monitors: &[Monitor], current: Option<usize>, screen: DockScreen) -> Option<Monitor> {
    let primary = monitors.iter().find(|monitor| monitor.primary);
    let current = current.and_then(|index| monitors.get(index));
    let selected = match screen {
        DockScreen::Current => current,
        DockScreen::Primary => primary,
        DockScreen::Number(number) => number.checked_sub(1).and_then(|index| monitors.get(index as usize)),
    };
    selected.or(current).or(primary).or(monitors.first()).copied()
}

pub fn monitor_at(monitors: &[Monitor], point: (i32, i32)) -> Option<usize> {
    monitors.iter().enumerate().min_by_key(|(_, monitor)| distance(monitor.bounds, (i64::from(point.0), i64::from(point.1)))).map(|(index, _)| index)
}

fn distance(bounds: Bounds, point: (i64, i64)) -> i128 {
    let (x, y, w, h) = bounds;
    let dx = point.0 - point.0.clamp(i64::from(x), i64::from(x) + i64::from(w.max(1) - 1));
    let dy = point.1 - point.1.clamp(i64::from(y), i64::from(y) + i64::from(h.max(1) - 1));
    i128::from(dx).pow(2) + i128::from(dy).pow(2)
}

fn overlap(start: i32, size: i32, other: i32, other_size: i32) -> i64 {
    (i64::from(start) + i64::from(size)).min(i64::from(other) + i64::from(other_size)) - i64::from(start.max(other))
}

pub fn correct_offscreen(window: Bounds, monitors: &[Monitor]) -> Option<Bounds> {
    let (x, y, w, h) = window;
    if w <= 0 || h <= 0 {
        return None;
    }
    let title_height = h.min(32);
    let title_width = w.min(96);
    let usable = monitors.iter().filter(|monitor| monitor.work.2 > 0 && monitor.work.3 > 0);
    if usable.clone().any(|monitor| {
        let (left, top, width, height) = monitor.work;
        overlap(x, w, left, width) >= i64::from(title_width)
            && overlap(y, title_height, top, height) >= i64::from(title_height)
    }) {
        return None;
    }
    let center = (i64::from(x) + i64::from(w) / 2, i64::from(y) + i64::from(h) / 2);
    let monitor = usable.min_by_key(|monitor| (distance(monitor.work, center), !monitor.primary))?;
    let (left, top, width, height) = monitor.work;
    let (w, h) = (w.min(width), h.min(height));
    let x = x.clamp(left, left.saturating_add(width - w));
    let y = y.clamp(top, top.saturating_add(height - h));
    Some((x, y, w, h))
}

#[cfg(test)]
#[path = "../test/monitors.rs"]
mod tests;