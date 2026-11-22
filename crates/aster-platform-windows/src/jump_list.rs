use aster_core::jump_list::{Item, JumpList};
use aster_platform::PlatformError;
use std::collections::HashSet;
use std::path::Path;
use windows::core::{HSTRING, Interface, PCWSTR, Result};
use windows::Win32::Foundation::E_ACCESSDENIED;
use windows::Win32::Storage::EnhancedStorage::PKEY_Title;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::UI::Shell::{DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, Common::{IObjectArray, IObjectCollection}, PropertiesSystem::IPropertyStore};
use crate::shell_link::{self, Apartment, APP_ID};

fn context<T>(result: Result<T>, operation: &str) -> Result<T> {
    result.map_err(|error| windows::core::Error::new(error.code(), format!("{operation}: {}", error.message())))
}

fn removed_arguments(removed: &IObjectArray) -> Result<HashSet<String>> {
    let mut args = HashSet::new();
    let mut buffer = vec![0; 32768];
    for index in 0..unsafe { removed.GetCount()? } {
        let Ok(link) = (unsafe { removed.GetAt::<IShellLinkW>(index) }) else { continue };
        unsafe { link.GetArguments(&mut buffer)? };
        let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        args.insert(String::from_utf16_lossy(&buffer[..end]));
    }
    Ok(args)
}

fn link(exe: &Path, title: &str, args: &str) -> Result<IShellLinkW> {
    let link = shell_link::for_exe(exe)?;
    let store: IPropertyStore = link.cast()?;
    let title = HSTRING::from(title);
    shell_link::set_app_id(&store)?;
    shell_link::set_string(&store, &PKEY_Title, PCWSTR(title.as_ptr()))?;
    unsafe {
        link.SetArguments(&HSTRING::from(args))?;
        store.Commit()?;
    }
    Ok(link)
}

fn items(exe: &Path, entries: &[Item], removed: &HashSet<String>, limit: usize) -> Result<IObjectArray> {
    let collection: IObjectCollection = unsafe { CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)? };
    let entries = entries.iter().map(|item| (item, crate::update::command_line(&item.args)))
        .filter(|(_, args)| !removed.contains(args)).take(limit);
    for (item, args) in entries {
        unsafe { collection.AddObject(&link(exe, &item.title, &args)?)? };
    }
    collection.cast()
}

fn populate(list: &ICustomDestinationList, exe: &Path, model: &JumpList, removed: &IObjectArray, slots: usize) -> Result<()> {
    let removed = removed_arguments(removed)?;
    let [profiles, folders] = &model.categories;
    let profile_limit = if slots > 1 && !folders.items.is_empty() { slots - 1 } else { usize::MAX };
    for (category, limit) in [(profiles, profile_limit), (folders, usize::MAX)] {
        let items = items(exe, &category.items, &removed, limit)?;
        unsafe {
            if items.GetCount()? != 0 {
                match list.AppendCategory(&HSTRING::from(category.title), &items) {
                    Err(error) if error.code() == E_ACCESSDENIED => {}
                    result => context(result, "AppendCategory")?,
                }
            }
        }
    }
    let tasks = items(exe, std::slice::from_ref(&model.task), &HashSet::new(), 1)?;
    unsafe {
        context(list.AddUserTasks(&tasks), "AddUserTasks")?;
        context(list.CommitList(), "CommitList")
    }
}

fn replace(exe: &Path, model: &JumpList) -> Result<()> {
    let _com = context(Apartment::enter(), "CoInitializeEx")?;
    let list: ICustomDestinationList = context(unsafe { CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER) }, "Create DestinationList")?;
    let mut slots = 0;
    let removed = unsafe {
        context(list.SetAppID(APP_ID), "SetAppID")?;
        context(list.BeginList(&mut slots), "BeginList")?
    };
    let result = populate(&list, exe, model, &removed, slots as usize);
    if result.is_err() {
        let _ = unsafe { list.AbortList() };
    }
    result
}

pub fn apply(model: &JumpList) -> std::result::Result<(), PlatformError> {
    let exe = std::env::current_exe().map_err(|error| PlatformError::new(error.raw_os_error().unwrap_or_default().into(), error.to_string()))?;
    replace(&exe, model).map_err(crate::err)
}

pub fn remove() -> std::result::Result<(), PlatformError> {
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND};
    let _com = Apartment::enter().map_err(crate::err)?;
    let list: ICustomDestinationList = unsafe { CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER) }.map_err(crate::err)?;
    match unsafe { list.DeleteList(APP_ID) } {
        Err(error) if [ERROR_FILE_NOT_FOUND.to_hresult(), ERROR_PATH_NOT_FOUND.to_hresult()].contains(&error.code()) => Ok(()),
        result => result.map_err(crate::err),
    }
}
