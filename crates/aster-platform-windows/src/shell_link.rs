use std::mem::ManuallyDrop;
use std::path::Path;
use windows::core::{w, HSTRING, PCWSTR, Result};
use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::{IShellLinkW, SetCurrentProcessExplicitAppUserModelID, ShellLink, SHStrDupW, PropertiesSystem::IPropertyStore};

pub const APP_ID: PCWSTR = w!("Aster.Terminal");

pub fn set_process_id() -> Result<()> {
    unsafe { SetCurrentProcessExplicitAppUserModelID(APP_ID) }
}

pub struct Apartment;

impl Apartment {
    pub fn enter() -> Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

pub fn new() -> Result<IShellLinkW> {
    unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }
}

pub fn for_exe(exe: &Path) -> Result<IShellLinkW> {
    let link = new()?;
    let exe = HSTRING::from(exe);
    unsafe {
        link.SetPath(&exe)?;
        link.SetIconLocation(&exe, 0)?;
    }
    Ok(link)
}

pub fn set_string(store: &IPropertyStore, key: &PROPERTYKEY, text: PCWSTR) -> Result<()> {
    let mut value = PROPVARIANT_0_0 { vt: VT_LPWSTR, ..Default::default() };
    value.Anonymous.pwszVal = unsafe { SHStrDupW(text)? };
    let value = PROPVARIANT { Anonymous: PROPVARIANT_0 { Anonymous: ManuallyDrop::new(value) } };
    unsafe { store.SetValue(key, &value) }
}

pub fn set_app_id(store: &IPropertyStore) -> Result<()> {
    set_string(store, &PKEY_AppUserModel_ID, APP_ID)
}
