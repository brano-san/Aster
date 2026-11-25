use crate::pty_host::{sid, Security};
use aster_core::default_terminal::State;
use aster_platform::PlatformError;
use std::ffi::{OsStr, OsString};
use std::io;
use std::marker::PhantomData;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::Path;
use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_INSUFFICIENT_BUFFER, ERROR_MORE_DATA, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT, WIN32_ERROR};
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, REG_OPTION_NON_VOLATILE, REG_SAM_FLAGS, REG_SZ, REG_VALUE_TYPE,
};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Threading::{CreateMutexW, GetCurrentProcess, ReleaseMutex, WaitForSingleObject};

const STARTUP: &str = r"Console\%%Startup";
const CHOICE: [&str; 2] = ["DelegationConsole", "DelegationTerminal"];
const CONSOLE_FIRST: [usize; 2] = [0, 1];
const TERMINAL_FIRST: [usize; 2] = [1, 0];
const CONSOLE: &str = "{2EACA947-7F5F-4CFA-BA87-8F7FBEEFBE69}";
const TERMINAL: &str = "{0C611DF3-4012-4AA4-B6E6-29D4673DB10F}";
const COM_SERVER: &str = r"Software\Classes\CLSID\{0C611DF3-4012-4AA4-B6E6-29D4673DB10F}\LocalServer32";
const SERVER: [&str; 2] = ["ServerExecutable", ""];
const BACKUP: &str = r"Software\Aster\DefaultTerminal";
const PREVIOUS: &str = "Previous";
const MAX_VALUE_SIZE: u32 = 2 * 1024 * 1024;
const LOCK_TIMEOUT_MS: u32 = 5_000;

fn open_mutex(name: &str) -> io::Result<OwnedHandle> {
    let security = Security::current_user()?;
    let mutex = unsafe { CreateMutexW(Some(&security.attributes()), false, &HSTRING::from(name)) }.map_err(io::Error::other)?;
    Ok(unsafe { OwnedHandle::from_raw_handle(mutex.0) })
}

struct RegistrationLock(OwnedHandle, PhantomData<*mut ()>);

impl RegistrationLock {
    fn acquire() -> io::Result<Self> {
        let user = sid(unsafe { GetCurrentProcess() })?;
        Self::named(&format!(r"Global\Aster.DefaultTerminal-{user}"), LOCK_TIMEOUT_MS)
    }

    fn named(name: &str, timeout_ms: u32) -> io::Result<Self> {
        let mutex = open_mutex(name)?;
        match unsafe { WaitForSingleObject(HANDLE(mutex.as_raw_handle()), timeout_ms) } {
            WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Self(mutex, PhantomData)),
            WAIT_TIMEOUT => Err(io::Error::new(io::ErrorKind::TimedOut, "Timed out waiting for default terminal settings")),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

impl Drop for RegistrationLock {
    fn drop(&mut self) {
        let _ = unsafe { ReleaseMutex(HANDLE(self.0.as_raw_handle())) };
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn GetPackagesByPackageFamily(family: PCWSTR, count: *mut u32, names: *mut PWSTR, length: *mut u32, buffer: PWSTR) -> i32;
}

#[link(name = "ntdll")]
extern "system" {
    fn RtlGetVersion(version: *mut OSVERSIONINFOW) -> i32;
}

fn supported() -> bool {
    let mut version = OSVERSIONINFOW { dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32, ..Default::default() };
    if unsafe { RtlGetVersion(&mut version) } < 0 || version.dwBuildNumber < 22000 {
        return false;
    }
    let family = w!("Microsoft.WindowsTerminal_8wekyb3d8bbwe");
    let (mut count, mut length) = (0, 0);
    let result = unsafe { GetPackagesByPackageFamily(family, &mut count, std::ptr::null_mut(), &mut length, PWSTR::null()) };
    if result as u32 != ERROR_INSUFFICIENT_BUFFER.0 || count == 0 { return false; }
    let mut names = vec![PWSTR::null(); count as usize];
    let mut buffer = vec![0u16; length as usize];
    let result = unsafe { GetPackagesByPackageFamily(family, &mut count, names.as_mut_ptr(), &mut length, PWSTR(buffer.as_mut_ptr())) };
    result as u32 == ERROR_SUCCESS.0 && names.iter().take(count as usize).any(|name| {
        !name.is_null() && unsafe { name.to_string() }.is_ok_and(|name| compatible_provider(&name))
    })
}

fn compatible_provider(full_name: &str) -> bool {
    let Some(version) = full_name.split('_').nth(1) else { return false; };
    let parts: Option<Vec<u16>> = version.split('.').map(|part| part.parse().ok()).collect();
    matches!(parts.as_deref(), Some([major, minor, _, _]) if (*major, *minor) >= (1, 22))
}

fn status(result: WIN32_ERROR) -> io::Result<()> {
    result.ok().map_err(|_| io::Error::from_raw_os_error(result.0 as i32))
}

fn missing(code: WIN32_ERROR) -> bool {
    matches!(code, ERROR_FILE_NOT_FOUND | ERROR_PATH_NOT_FOUND)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Raw {
    kind: u32,
    bytes: Vec<u8>,
}

impl Raw {
    fn text(value: impl AsRef<OsStr>) -> Self {
        Self { kind: REG_SZ.0, bytes: value.as_ref().encode_wide().chain([0]).flat_map(u16::to_le_bytes).collect() }
    }

    fn matches_guid(&self, value: &str) -> bool {
        if self.kind != REG_SZ.0 || !self.bytes.len().is_multiple_of(2) {
            return false;
        }
        let words: Vec<_> = self.bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).take_while(|&word| word != 0).collect();
        OsString::from_wide(&words).to_str().is_some_and(|stored| stored.eq_ignore_ascii_case(value))
    }
}

type Pair = [Option<Raw>; 2];

struct Key(HKEY);

impl Key {
    fn open(path: &str, access: REG_SAM_FLAGS) -> io::Result<Option<Self>> {
        let mut key = HKEY::default();
        let result = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), None, access, &mut key) };
        if missing(result) { return Ok(None); }
        status(result)?;
        Ok(Some(Self(key)))
    }

    fn create(path: &str) -> io::Result<Self> {
        let mut key = HKEY::default();
        status(unsafe { RegCreateKeyExW(HKEY_CURRENT_USER, &HSTRING::from(path), None, None, REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut key, None) })?;
        Ok(Self(key))
    }

    fn read(&self, name: &str) -> io::Result<Option<Raw>> {
        let name = HSTRING::from(name);
        let mut size = 0;
        let mut kind = REG_VALUE_TYPE::default();
        let result = unsafe { RegQueryValueExW(self.0, &name, None, Some(&mut kind), None, Some(&mut size)) };
        if missing(result) { return Ok(None); }
        status(result)?;
        if size > MAX_VALUE_SIZE { return Err(invalid_backup()); }
        let mut bytes = vec![0; size as usize];
        let result = unsafe { RegQueryValueExW(self.0, &name, None, Some(&mut kind), Some(bytes.as_mut_ptr()), Some(&mut size)) };
        match result {
            ERROR_SUCCESS => {
                bytes.truncate(size as usize);
                Ok(Some(Raw { kind: kind.0, bytes }))
            }
            ERROR_MORE_DATA => Err(changed()),
            error if missing(error) => Ok(None),
            error => Err(io::Error::from_raw_os_error(error.0 as i32)),
        }
    }
}

impl Drop for Key {
    fn drop(&mut self) {
        let _ = unsafe { RegCloseKey(self.0) };
    }
}

fn write_value(path: &str, name: &str, value: Option<&Raw>) -> io::Result<()> {
    let name = HSTRING::from(name);
    if let Some(value) = value {
        if value.bytes.len() > MAX_VALUE_SIZE as usize { return Err(invalid_backup()); }
        let key = Key::create(path)?;
        return status(unsafe { RegSetValueExW(key.0, &name, None, REG_VALUE_TYPE(value.kind), Some(&value.bytes)) });
    }
    if let Some(key) = Key::open(path, KEY_SET_VALUE)? {
        let result = unsafe { RegDeleteValueW(key.0, &name) };
        if !missing(result) { status(result)?; }
    }
    Ok(())
}

fn read_pair(path: &str, names: [&str; 2]) -> io::Result<Pair> {
    let Some(key) = Key::open(path, KEY_QUERY_VALUE)? else { return Ok([None, None]); };
    Ok([key.read(names[0])?, key.read(names[1])?])
}

fn changed() -> io::Error {
    io::Error::new(io::ErrorKind::AlreadyExists, "The default terminal selection changed during the operation")
}

fn replace_pair(
    order: [usize; 2],
    previous: &Pair,
    next: &Pair,
    read: impl Fn() -> io::Result<Pair>,
    write: impl Fn(usize, Option<&Raw>) -> io::Result<()>,
) -> io::Result<()> {
    if &read()? != previous { return Err(changed()); }
    let mut expected = previous.clone();
    let [first, second] = order;
    for index in order {
        if previous[index] == next[index] { continue; }
        if let Err(error) = write(index, next[index].as_ref()) {
            if index == second && expected[first] != previous[first] && read().is_ok_and(|current| current == expected) {
                if let Err(restore) = write(first, previous[first].as_ref()) {
                    return Err(io::Error::other(format!("{error}\n{restore}")));
                }
            }
            return Err(error);
        }
        expected[index] = next[index].clone();
        if read()? != expected { return Err(changed()); }
    }
    Ok(())
}

fn write_pair(path: &str, names: [&str; 2], order: [usize; 2], previous: &Pair, next: &Pair) -> io::Result<()> {
    replace_pair(order, previous, next, || read_pair(path, names), |index, value| write_value(path, names[index], value))
}

fn invalid_backup() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "The saved default terminal selection is invalid")
}

fn encode_backup(previous: &Pair) -> Vec<u8> {
    let mut bytes = Vec::new();
    for value in previous {
        bytes.push(u8::from(value.is_some()));
        if let Some(value) = value {
            bytes.extend(value.kind.to_le_bytes());
            bytes.extend((value.bytes.len() as u32).to_le_bytes());
            bytes.extend(&value.bytes);
        }
    }
    bytes
}

fn decode_backup(mut bytes: &[u8]) -> io::Result<Pair> {
    let mut pair = [None, None];
    for slot in &mut pair {
        let (&tag, tail) = bytes.split_first().ok_or_else(invalid_backup)?;
        bytes = tail;
        match tag {
            0 => {},
            1 => {
                let (kind, tail) = bytes.split_first_chunk::<4>().ok_or_else(invalid_backup)?;
                let (len, tail) = tail.split_first_chunk::<4>().ok_or_else(invalid_backup)?;
                let (data, tail) = tail.split_at_checked(u32::from_le_bytes(*len) as usize).ok_or_else(invalid_backup)?;
                *slot = Some(Raw { kind: u32::from_le_bytes(*kind), bytes: data.to_vec() });
                bytes = tail;
            }
            _ => return Err(invalid_backup()),
        }
    }
    if !bytes.is_empty() { return Err(invalid_backup()); }
    Ok(pair)
}

fn previous() -> io::Result<Option<Pair>> {
    let Some(key) = Key::open(BACKUP, KEY_QUERY_VALUE)? else { return Ok(None); };
    match key.read(PREVIOUS)? {
        Some(value) if value.kind == REG_BINARY.0 => decode_backup(&value.bytes).map(Some),
        Some(_) => Err(invalid_backup()),
        None => Ok(None),
    }
}

fn selection() -> Pair {
    [Some(Raw::text(CONSOLE)), Some(Raw::text(TERMINAL))]
}

fn is_aster_terminal(pair: &Pair) -> bool {
    pair[1].as_ref().is_some_and(|value| value.matches_guid(TERMINAL))
}

fn owns_choice(pair: &Pair, previous: Option<&Pair>) -> bool {
    is_aster_terminal(pair) && (pair[0].as_ref().is_some_and(|value| value.matches_guid(CONSOLE))
        || previous.is_some_and(|old| !is_aster_terminal(old) && old[0] == pair[0]))
}

fn is_selected(pair: &Pair) -> bool {
    owns_choice(pair, None)
}

fn registration(executable: &Path) -> Pair {
    let mut command = OsString::from("\"");
    command.push(executable);
    command.push("\" ");
    command.push(super::defterm::SERVER_ARG);
    [Some(Raw::text(executable)), Some(Raw::text(command))]
}

fn compatible_registration(actual: &Pair, expected: &Pair) -> bool {
    actual.iter().zip(expected).all(|(actual, expected)| actual.is_none() || actual == expected)
}

fn state_for(executable: &Path) -> io::Result<State> {
    let reg = read_pair(COM_SERVER, SERVER)?;
    if !compatible_registration(&reg, &registration(executable)) {
        return Ok(State::Unavailable);
    }
    if reg != [None, None] {
        let active = read_pair(STARTUP, CHOICE)?;
        if is_aster_terminal(&active) {
            let recoverable = is_selected(&active) || owns_choice(&active, previous()?.as_ref());
            return Ok(if recoverable { State::Selected } else { State::Unavailable });
        }
    }
    Ok(if supported() { State::Available } else { State::Unavailable })
}

fn activate(executable: &Path) -> io::Result<State> {
    if !supported() { return Err(io::Error::new(io::ErrorKind::Unsupported, "Default terminal delegation is unavailable")); }
    let registered = read_pair(COM_SERVER, SERVER)?;
    let registration = registration(executable);
    if !compatible_registration(&registered, &registration) { return Err(changed()); }

    let active = read_pair(STARTUP, CHOICE)?;
    if is_aster_terminal(&active) {
        if !is_selected(&active) && !owns_choice(&active, previous()?.as_ref()) { return Err(changed()); }
    } else {
        let previous = Raw { kind: REG_BINARY.0, bytes: encode_backup(&active) };
        write_value(BACKUP, PREVIOUS, Some(&previous))?;
    }
    write_pair(COM_SERVER, SERVER, CONSOLE_FIRST, &registered, &registration)?;
    if !is_selected(&active) {
        if let Err(error) = write_pair(STARTUP, CHOICE, TERMINAL_FIRST, &active, &selection()) {
            if read_pair(STARTUP, CHOICE).is_ok_and(|choice| !is_aster_terminal(&choice)) {
                if let Err(restore) = write_pair(COM_SERVER, SERVER, CONSOLE_FIRST, &registration, &registered) {
                    return Err(io::Error::other(format!("{error}\n{restore}")));
                }
            }
            return Err(error);
        }
    }
    Ok(State::Selected)
}

fn deactivate(executable: &Path) -> io::Result<()> {
    let registered = read_pair(COM_SERVER, SERVER)?;
    if registered == [None, None] || !compatible_registration(&registered, &registration(executable)) { return Ok(()); }

    let active = read_pair(STARTUP, CHOICE)?;
    if is_aster_terminal(&active) {
        let previous = previous()?;
        if !owns_choice(&active, previous.as_ref()) { return Err(changed()); }
        let previous = previous.unwrap_or([None, None]);
        if is_aster_terminal(&previous) { return Err(invalid_backup()); }
        write_pair(STARTUP, CHOICE, CONSOLE_FIRST, &active, &previous)?;
    }
    write_pair(COM_SERVER, SERVER, CONSOLE_FIRST, &registered, &[None, None])?;
    write_value(BACKUP, PREVIOUS, None)
}

pub fn remove(executable: &Path) -> io::Result<()> {
    let _lock = RegistrationLock::acquire()?;
    deactivate(executable)
}

fn platform(error: io::Error) -> PlatformError {
    PlatformError::new(error.raw_os_error().unwrap_or(0) as i64, error.to_string())
}

pub fn state() -> Result<State, PlatformError> {
    std::env::current_exe().and_then(|executable| {
        let _lock = RegistrationLock::acquire()?;
        state_for(&executable)
    }).map_err(platform)
}

pub fn set(enabled: bool) -> Result<State, PlatformError> {
    let result = std::env::current_exe().and_then(|executable| {
        let _lock = RegistrationLock::acquire()?;
        if enabled { activate(&executable) } else { deactivate(&executable).and_then(|()| state_for(&executable)) }
    });
    result.map_err(platform)
}

#[cfg(test)]
#[path = "../test/defterm_registration.rs"]
mod tests;
