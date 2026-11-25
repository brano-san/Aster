use aster_platform::host::persistent::{Connection, Listener};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use std::time::Duration;
use windows::core::{HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_NO_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, ERROR_PIPE_NOT_CONNECTED, GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL, INVALID_HANDLE_VALUE, LocalFree, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Security::Authorization::{ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser};
use windows::Win32::Storage::FileSystem::{CreateFileW, ReadFile, WriteFile, CREATE_NEW, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_MODE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT};
use windows::Win32::System::Threading::{CreateEventW, GetCurrentProcess, OpenProcess, OpenProcessToken, ResetEvent, WaitForSingleObject, CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, DETACHED_PROCESS, INFINITE, PROCESS_QUERY_LIMITED_INFORMATION};

const IO_TIMEOUT_MS: u32 = 5000;

fn os_error(error: windows::core::Error) -> io::Error {
    let code = error.code().0 as u32;
    if code & 0xffff_0000 == 0x8007_0000 {
        io::Error::from_raw_os_error((code & 0xffff) as i32)
    } else {
        io::Error::other(error)
    }
}

fn raw(handle: &OwnedHandle) -> HANDLE { HANDLE(handle.as_raw_handle()) }

pub(super) fn sid(process: HANDLE) -> io::Result<String> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.map_err(os_error)?;
    let token = unsafe { OwnedHandle::from_raw_handle(token.0) };
    let mut len = 0;
    let _ = unsafe { GetTokenInformation(raw(&token), TokenUser, None, 0, &mut len) };
    if len < size_of::<TOKEN_USER>() as u32 { return Err(io::Error::last_os_error()); }
    let mut data = vec![0usize; (len as usize).div_ceil(size_of::<usize>())];
    unsafe { GetTokenInformation(raw(&token), TokenUser, Some(data.as_mut_ptr().cast()), len, &mut len) }.map_err(os_error)?;
    let user = unsafe { &*data.as_ptr().cast::<TOKEN_USER>() };
    let mut text = PWSTR::null();
    unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) }.map_err(os_error)?;
    let result = unsafe { text.to_string() }.map_err(io::Error::other);
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    result
}

pub fn endpoint() -> io::Result<String> {
    let hash = sid(unsafe { GetCurrentProcess() })?.bytes().fold(0xcbf29ce484222325u64, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3));
    Ok(format!(r"\\.\pipe\aster-pty-{hash:016x}"))
}

pub(super) struct Security(PSECURITY_DESCRIPTOR);

impl Security {
    pub(super) fn current_user() -> io::Result<Self> {
        let sddl = HSTRING::from(format!("D:P(A;;GA;;;{})", sid(unsafe { GetCurrentProcess() })?));
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe { ConvertStringSecurityDescriptorToSecurityDescriptorW(&sddl, SDDL_REVISION_1, &mut descriptor, None) }.map_err(os_error)?;
        Ok(Self(descriptor))
    }

    pub(super) fn attributes(&self) -> SECURITY_ATTRIBUTES {
        SECURITY_ATTRIBUTES { nLength: size_of::<SECURITY_ATTRIBUTES>() as u32, lpSecurityDescriptor: self.0.0, ..Default::default() }
    }
}

impl Drop for Security {
    fn drop(&mut self) { unsafe { LocalFree(Some(HLOCAL(self.0.0))) }; }
}

pub fn create_token(path: &Path) -> io::Result<File> {
    let security = Security::current_user()?;
    let handle = unsafe { CreateFileW(&HSTRING::from(path.as_os_str()), GENERIC_WRITE.0, FILE_SHARE_MODE(0), Some(&security.attributes()), CREATE_NEW, FILE_ATTRIBUTE_NORMAL, None) }.map_err(os_error)?;
    Ok(unsafe { File::from_raw_handle(handle.0) })
}

fn event() -> io::Result<OwnedHandle> {
    let handle = unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(os_error)?;
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

fn wait_event(event: &OwnedHandle, ms: u32) -> io::Result<bool> {
    match unsafe { WaitForSingleObject(raw(event), ms) } {
        WAIT_OBJECT_0 => Ok(true),
        WAIT_TIMEOUT => Ok(false),
        _ => Err(io::Error::last_os_error()),
    }
}

fn cancel(handle: HANDLE, io: &OVERLAPPED) {
    unsafe {
        let _ = CancelIoEx(handle, Some(io));
        let _ = GetOverlappedResult(handle, io, &mut 0, true);
    }
}

fn disconnected(error: &io::Error) -> bool {
    matches!(error.raw_os_error().map(|code| code as u32), Some(code) if [ERROR_BROKEN_PIPE, ERROR_NO_DATA, ERROR_PIPE_NOT_CONNECTED].iter().any(|e| e.0 == code))
}

struct PipeIo {
    pipe: Arc<OwnedHandle>,
    event: OwnedHandle,
}

impl PipeIo {
    fn transfer(&self, ms: u32, start: impl FnOnce(HANDLE, &mut OVERLAPPED) -> windows::core::Result<()>) -> io::Result<usize> {
        let handle = raw(&self.pipe);
        unsafe { ResetEvent(raw(&self.event)) }.map_err(os_error)?;
        let mut overlapped = OVERLAPPED { hEvent: raw(&self.event), ..Default::default() };
        match start(handle, &mut overlapped) {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => match wait_event(&self.event, ms) {
                Ok(true) => {}
                result => {
                    cancel(handle, &overlapped);
                    return Err(result.err().unwrap_or_else(|| io::ErrorKind::TimedOut.into()));
                }
            },
            Err(error) => return Err(os_error(error)),
        }
        let mut transferred = 0;
        unsafe { GetOverlappedResult(handle, &overlapped, &mut transferred, false) }.map_err(os_error)?;
        Ok(transferred as usize)
    }
}

struct PipeReader(PipeIo, Arc<AtomicBool>);

impl Read for PipeReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let ms = if self.1.load(Ordering::Relaxed) { INFINITE } else { IO_TIMEOUT_MS };
        match self.0.transfer(ms, |pipe, io| unsafe { ReadFile(pipe, Some(bytes), None, Some(io)) }) {
            Err(error) if disconnected(&error) => Ok(0),
            result => result,
        }
    }
}

impl Write for PipeIo {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.transfer(IO_TIMEOUT_MS, |pipe, io| unsafe { WriteFile(pipe, Some(bytes), None, Some(io)) })
    }

    fn flush(&mut self) -> io::Result<()> { Ok(()) }
}

fn connection(pipe: OwnedHandle, server: bool) -> io::Result<Connection> {
    let pipe = Arc::new(pipe);
    let streaming = Arc::new(AtomicBool::new(false));
    let reader = PipeReader(PipeIo { pipe: pipe.clone(), event: event()? }, streaming.clone());
    let writer = PipeIo { pipe: pipe.clone(), event: event()? };
    Ok(Connection::new(Box::new(reader), Box::new(writer), move || {
        streaming.store(true, Ordering::Relaxed);
        Ok(())
    }).with_abort(move || unsafe {
        if server { let _ = DisconnectNamedPipe(raw(&pipe)); }
        else { let _ = CancelIoEx(raw(&pipe), None); }
    }))
}

struct Pending {
    pipe: Option<OwnedHandle>,
    event: OwnedHandle,
    overlapped: Box<OVERLAPPED>,
    waiting: bool,
}

impl Pending {
    fn new(name: &HSTRING, security: &Security, first: bool) -> io::Result<Self> {
        let event = event()?;
        let flags = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | if first { FILE_FLAG_FIRST_PIPE_INSTANCE } else { FILE_FLAGS_AND_ATTRIBUTES(0) };
        let handle = unsafe { CreateNamedPipeW(name, flags, PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, PIPE_UNLIMITED_INSTANCES, 64 * 1024, 64 * 1024, 0, Some(&security.attributes())) };
        if handle == INVALID_HANDLE_VALUE {
            let error = io::Error::last_os_error();
            return Err(if first && error.raw_os_error() == Some(ERROR_ACCESS_DENIED.0 as i32) { io::ErrorKind::AddrInUse.into() } else { error });
        }
        let pipe = unsafe { OwnedHandle::from_raw_handle(handle.0) };
        let mut overlapped = Box::new(OVERLAPPED { hEvent: raw(&event), ..Default::default() });
        let waiting = loop {
            match unsafe { ConnectNamedPipe(handle, Some(&mut *overlapped)) } {
                Ok(()) => break false,
                Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => break false,
                Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => break true,
                Err(error) if error.code() == ERROR_NO_DATA.to_hresult() => unsafe { DisconnectNamedPipe(handle) }.map_err(os_error)?,
                Err(error) => return Err(os_error(error)),
            }
        };
        Ok(Self { pipe: Some(pipe), event, overlapped, waiting })
    }

    fn ready(&mut self, timeout: Duration) -> io::Result<bool> {
        if !self.waiting { return Ok(true); }
        if !wait_event(&self.event, timeout.as_millis().min(u128::from(INFINITE - 1)) as u32)? { return Ok(false); }
        self.waiting = false;
        unsafe { GetOverlappedResult(raw(self.pipe.as_ref().unwrap()), &*self.overlapped, &mut 0, false) }.map_err(os_error)?;
        Ok(true)
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        if self.waiting { cancel(raw(self.pipe.as_ref().unwrap()), &self.overlapped); }
    }
}

struct PipeListener {
    name: HSTRING,
    security: Security,
    pending: Pending,
}

impl Listener for PipeListener {
    fn accept(&mut self, timeout: Duration) -> io::Result<Option<Connection>> {
        let ready = self.pending.ready(timeout);
        if matches!(ready, Ok(false)) { return Ok(None); }
        let next = Pending::new(&self.name, &self.security, false)?;
        let mut accepted = std::mem::replace(&mut self.pending, next);
        match ready {
            Ok(_) => connection(accepted.pipe.take().unwrap(), true).map(Some),
            Err(error) if disconnected(&error) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

pub fn listen(name: &str) -> io::Result<Box<dyn Listener>> {
    if name != endpoint()? { return Err(io::ErrorKind::InvalidInput.into()); }
    let name = HSTRING::from(name);
    let security = Security::current_user()?;
    let pending = Pending::new(&name, &security, true)?;
    Ok(Box::new(PipeListener { name, security, pending }))
}

pub fn connect(name: &str) -> io::Result<Connection> {
    let flags = FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION;
    let handle = unsafe { CreateFileW(&HSTRING::from(name), GENERIC_READ.0 | GENERIC_WRITE.0, FILE_SHARE_MODE(0), None, OPEN_EXISTING, flags, None) }.map_err(os_error)
        .map_err(|error| if error.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32) { io::ErrorKind::WouldBlock.into() } else { error })?;
    let pipe = unsafe { OwnedHandle::from_raw_handle(handle.0) };
    let mut pid = 0;
    unsafe { GetNamedPipeServerProcessId(handle, &mut pid) }.map_err(os_error)?;
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.map_err(os_error)?;
    let process = unsafe { OwnedHandle::from_raw_handle(process.0) };
    if sid(raw(&process))? != sid(unsafe { GetCurrentProcess() })? {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "terminal host belongs to a different user"));
    }
    connection(pipe, false)
}

pub fn start(endpoint: &str) -> io::Result<()> {
    let mut command = Command::new(std::env::current_exe()?);
    command.args(["--pty-host", endpoint]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    let flags = DETACHED_PROCESS.0 | CREATE_NEW_PROCESS_GROUP.0;
    match command.creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB.0).spawn() {
        Err(error) if error.raw_os_error() == Some(ERROR_ACCESS_DENIED.0 as i32) => command.creation_flags(flags).spawn().map(|_| ()),
        result => result.map(|_| ()),
    }
}
