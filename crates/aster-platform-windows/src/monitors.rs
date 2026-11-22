use aster_platform::monitors::{choose_monitor, correct_offscreen, monitor_at, Bounds, Monitor};
use aster_platform::{dock_rect, DockSlide, DockSpec, Edge};
use windows::core::BOOL;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows::Win32::UI::WindowsAndMessaging::*;

fn bounds(rect: RECT) -> Bounds {
    (rect.left, rect.top, rect.right - rect.left, rect.bottom - rect.top)
}

unsafe extern "system" fn collect_monitor(monitor: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let monitors = &mut *(data.0 as *mut Vec<Monitor>);
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    if GetMonitorInfoW(monitor, &mut info).as_bool() {
        monitors.push(Monitor { bounds: bounds(info.rcMonitor), work: bounds(info.rcWork), primary: info.dwFlags & MONITORINFOF_PRIMARY != 0 });
    }
    BOOL(1)
}

fn monitors() -> Vec<Monitor> {
    let mut monitors: Vec<Monitor> = Vec::new();
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect_monitor), LPARAM(&mut monitors as *mut Vec<Monitor> as isize));
    }
    monitors.sort_by_key(|monitor| (monitor.bounds.0, monitor.bounds.1));
    monitors
}

pub(crate) fn place_dock(hwnd: HWND, spec: Option<DockSpec>) -> Option<DockSlide> {
    unsafe {
        let Some(spec) = spec else {
            let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            return None;
        };
        let monitors = monitors();
        let mut point = POINT::default();
        let current = GetCursorPos(&mut point).ok().and_then(|_| monitor_at(&monitors, (point.x, point.y)));
        let monitor = choose_monitor(&monitors, current, spec.screen)?;
        if IsZoomed(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let (x, y, w, h) = dock_rect(monitor.work, spec);
        let after = if spec.on_top { HWND_TOPMOST } else { HWND_NOTOPMOST };
        SetWindowPos(hwnd, Some(after), x, y, w, h, SWP_NOACTIVATE).ok()?;
        if spec.edge != Edge::Top || IsIconic(hwnd).as_bool() { return None; }
        let mut rect = RECT::default();
        GetWindowRect(hwnd, &mut rect).ok()?;
        Some(DockSlide::top(bounds(rect), monitor.bounds.1))
    }
}

pub(crate) fn recover_normal_window(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() || IsZoomed(hwnd).as_bool() {
            return;
        }
        let mut rect = RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_err() {
            return;
        }
        if let Some((x, y, w, h)) = correct_offscreen(bounds(rect), &monitors()) {
            let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOACTIVATE | SWP_NOZORDER);
        }
    }
}
