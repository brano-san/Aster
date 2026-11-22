use aster_core::notifications::Notification;
use windows::core::{HSTRING, IInspectable};
use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
use windows::Win32::UI::Shell::GetCurrentProcessExplicitAppUserModelID;

struct Apartment;

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { RoUninitialize() }
    }
}

fn xml_text(text: &str) -> String {
    let mut escaped = String::new();
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            c if c.is_control() || matches!(c, '\u{FFFE}' | '\u{FFFF}') => escaped.push(' '),
            c => escaped.push(c),
        }
    }
    escaped
}

pub(crate) fn toast(notification: &Notification, clicked: impl Fn() + Send + 'static) -> windows::core::Result<Option<ToastNotification>> {
    let Ok(raw_id) = (unsafe { GetCurrentProcessExplicitAppUserModelID() }) else { return Ok(None) };
    let id = unsafe { HSTRING::from_wide(raw_id.as_wide()) };
    unsafe { CoTaskMemFree(Some(raw_id.0.cast())) };
    if id.is_empty() {
        return Ok(None);
    }

    unsafe { RoInitialize(RO_INIT_MULTITHREADED)?; }
    let _apartment = Apartment;
    let doc = XmlDocument::new()?;
    let title = xml_text(&notification.title);
    let body = xml_text(&notification.body);
    doc.LoadXml(&HSTRING::from(format!("<toast><visual><binding template=\"ToastGeneric\"><text>{title}</text><text>{body}</text></binding></visual></toast>")))?;
    let toast = ToastNotification::CreateToastNotification(&doc)?;
    toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(move |_, _| {
        clicked();
        Ok(())
    }))?;
    ToastNotificationManager::CreateToastNotifierWithId(&id)?.Show(&toast)?;
    Ok(Some(toast))
}

#[cfg(test)]
#[path = "../test/notifications.rs"]
mod tests;