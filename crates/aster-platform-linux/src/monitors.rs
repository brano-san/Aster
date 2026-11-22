use aster_platform::monitors::Monitor;
use winit::monitor::MonitorHandle;

pub(crate) fn collect(monitors: impl Iterator<Item = MonitorHandle>, primary: Option<MonitorHandle>) -> Vec<Monitor> {
    let mut monitors: Vec<_> = monitors.filter_map(|monitor| {
        let at = monitor.position()?;
        let size = monitor.current_video_mode()?.size();
        let bounds = (at.x, at.y, size.width as i32, size.height as i32);
        Some(Monitor { bounds, work: bounds, primary: primary.as_ref() == Some(&monitor) })
    }).collect();
    monitors.sort_by_key(|monitor| (monitor.bounds.0, monitor.bounds.1));
    monitors
}
