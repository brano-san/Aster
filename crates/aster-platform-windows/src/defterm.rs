use crate::{instance::process_path, pty_host::{sid, Security}, shortcut::same_path};
use aster_platform::{PlatformError, Pty, Spawned};
use std::ffi::c_void;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::{atomic::{AtomicBool, Ordering}, Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{DuplicateHandle, DUPLICATE_SAME_ACCESS, CLASS_E_NOAGGREGATION, E_FAIL, E_INVALIDARG, E_POINTER, ERROR_IO_PENDING, ERROR_PIPE_CONNECTED, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows::Win32::Storage::FileSystem::{CreateFileW, ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, FILE_SHARE_MODE, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT};
use windows::Win32::System::Com::{CoInitializeEx, CoRegisterClassObject, CoRegisterPSClsid, CoRevokeClassObject, CoUninitialize, IClassFactory, IClassFactory_Impl, CLSCTX_INPROC_SERVER, CLSCTX_LOCAL_SERVER, COINIT_MULTITHREADED, REGCLS_MULTIPLEUSE, REGCLS_SINGLEUSE};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Pipes::{ConnectNamedPipe, CreateNamedPipeW, CreatePipe, GetNamedPipeClientProcessId, GetNamedPipeServerProcessId, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_WAIT};
use windows::Win32::System::Threading::{CreateEventW, GetCurrentProcess, GetExitCodeProcess, GetProcessId, OpenProcess, SetEvent, WaitForSingleObject, DETACHED_PROCESS, INFINITE, PROCESS_ACCESS_RIGHTS, PROCESS_DUP_HANDLE, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows_core::{implement, Interface, IUnknown, Ref, BOOL, GUID, HRESULT, HSTRING, PCWSTR};

pub use aster_core::default_terminal::valid_token;
pub const TERMINAL_CLSID: GUID = GUID::from_u128(0x0c611df3_4012_4aa4_b6e6_29d4673db10f);
pub const SERVER_ARG: &str = "--defterm-server";
pub const CLIENT_ARG: &str = "--defterm-attach";

const TRANSFER_TIMEOUT: Duration = Duration::from_secs(10);
const SERVER_TIMEOUT_MS: u32 = 30_000;
const OFFER_MAGIC: &[u8; 8] = b"asterdt1";
const OFFER_LEN: usize = OFFER_MAGIC.len() + 4 * size_of::<u64>();
const ACK: [u8; 2] = *b"OK";
const PROXY_CLSID: GUID = GUID::from_u128(0xf10cc1ad_2771_4baf_a981_9637ec3159db);
const PROXY_FACTORY_IID: GUID = GUID::from_u128(0xd5f569d0_593b_101a_b569_08002b2dbf7a);

extern "system" {
    fn AsterDeftermProxyGetClassObject(clsid: *const GUID, iid: *const GUID, result: *mut *mut c_void) -> HRESULT;
}

fn os(error: windows_core::Error) -> io::Error {
    let code = error.code().0 as u32;
    if code & 0xffff_0000 == 0x8007_0000 {
        io::Error::from_raw_os_error((code & 0xffff) as i32)
    } else {
        io::Error::other(error)
    }
}

fn platform(error: io::Error) -> PlatformError {
    PlatformError::new(i64::from(error.raw_os_error().unwrap_or_default()), error.to_string())
}

fn raw(handle: &OwnedHandle) -> HANDLE { HANDLE(handle.as_raw_handle()) }

unsafe fn owned(handle: HANDLE) -> OwnedHandle { OwnedHandle::from_raw_handle(handle.0) }

fn pipe_name(token: &str) -> HSTRING { HSTRING::from(format!(r"\\.\pipe\aster-defterm-{token}")) }

fn random_token() -> io::Result<String> {
    let mut bytes = [0; 16];
    getrandom::fill(&mut bytes).map_err(io::Error::other)?;
    Ok(format!("{:032x}", u128::from_le_bytes(bytes)))
}

fn encode_offer(handles: [usize; 4]) -> [u8; OFFER_LEN] {
    let mut offer = [0; OFFER_LEN];
    offer[..OFFER_MAGIC.len()].copy_from_slice(OFFER_MAGIC);
    for (field, handle) in offer[OFFER_MAGIC.len()..].chunks_exact_mut(8).zip(handles) {
        field.copy_from_slice(&(handle as u64).to_le_bytes());
    }
    offer
}

fn decode_offer(offer: [u8; OFFER_LEN]) -> io::Result<[usize; 4]> {
    if !offer.starts_with(OFFER_MAGIC) { return Err(io::ErrorKind::InvalidData.into()); }
    let mut handles = [0; 4];
    for (handle, field) in handles.iter_mut().zip(offer[OFFER_MAGIC.len()..].chunks_exact(8)) {
        *handle = usize::try_from(u64::from_le_bytes(field.try_into().unwrap())).map_err(|_| io::ErrorKind::InvalidData)?;
        if *handle == 0 || *handle == usize::MAX { return Err(io::ErrorKind::InvalidData.into()); }
    }
    Ok(handles)
}

fn manual_event() -> io::Result<OwnedHandle> {
    unsafe { CreateEventW(None, true, false, PCWSTR::null()).map(|handle| owned(handle)) }.map_err(os)
}

fn transfer(
    pipe: HANDLE,
    deadline: Instant,
    start: impl FnOnce(&mut OVERLAPPED) -> windows_core::Result<()>,
) -> io::Result<usize> {
    if Instant::now() >= deadline { return Err(io::ErrorKind::TimedOut.into()); }
    let event = manual_event()?;
    let mut overlapped = OVERLAPPED { hEvent: raw(&event), ..Default::default() };
    match start(&mut overlapped) {
        Ok(()) => {}
        Err(error) if error.code() == ERROR_PIPE_CONNECTED.to_hresult() => return Ok(0),
        Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {
            let left = deadline.saturating_duration_since(Instant::now());
            let result = unsafe { WaitForSingleObject(raw(&event), left.as_millis() as u32) };
            if result != WAIT_OBJECT_0 {
                let error = if result == WAIT_TIMEOUT { io::ErrorKind::TimedOut.into() } else { io::Error::last_os_error() };
                unsafe {
                    let _ = CancelIoEx(pipe, Some(&overlapped));
                    let _ = GetOverlappedResult(pipe, &overlapped, &mut 0, true);
                }
                return Err(error);
            }
        }
        Err(error) => return Err(os(error)),
    }
    let mut transferred = 0;
    unsafe { GetOverlappedResult(pipe, &overlapped, &mut transferred, false) }.map_err(os)?;
    Ok(transferred as usize)
}

fn receive_exact(pipe: HANDLE, deadline: Instant, mut bytes: &mut [u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        let read = transfer(pipe, deadline, |overlapped| unsafe { ReadFile(pipe, Some(bytes), None, Some(overlapped)) })?;
        if read == 0 { return Err(io::ErrorKind::UnexpectedEof.into()); }
        bytes = &mut bytes[read..];
    }
    Ok(())
}

fn send_exact(pipe: HANDLE, deadline: Instant, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        let written = transfer(pipe, deadline, |overlapped| unsafe { WriteFile(pipe, Some(bytes), None, Some(overlapped)) })?;
        if written == 0 { return Err(io::ErrorKind::WriteZero.into()); }
        bytes = &bytes[written..];
    }
    Ok(())
}

fn peer(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> io::Result<OwnedHandle> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | access, false, pid).map(|handle| owned(handle)) }.map_err(os)?;
    if sid(raw(&process))? != sid(unsafe { GetCurrentProcess() })?
        || !same_path(&process_path(raw(&process)).map_err(os)?, &std::env::current_exe()?)
    {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "unexpected default terminal peer"));
    }
    Ok(process)
}

fn checked_peer(pid: u32, access: PROCESS_ACCESS_RIGHTS) -> io::Result<OwnedHandle> {
    peer(pid, access).inspect_err(|error| tracing::warn!(pid, %error, "default terminal pipe peer rejected"))
}

fn local_pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let mut read = HANDLE::default();
    let mut write = HANDLE::default();
    unsafe {
        CreatePipe(&mut read, &mut write, None, 0).map_err(os)?;
        Ok((owned(read), owned(write)))
    }
}

fn duplicate_from(process: HANDLE, handle: usize) -> io::Result<OwnedHandle> {
    let mut copy = HANDLE::default();
    unsafe {
        DuplicateHandle(process, HANDLE(handle as _), GetCurrentProcess(), &mut copy, 0, false, DUPLICATE_SAME_ACCESS).map_err(os)?;
        Ok(owned(copy))
    }
}

fn relay(signal: HANDLE, client: HANDLE, launch: impl FnOnce(&str) -> io::Result<()>) -> io::Result<(OwnedHandle, OwnedHandle)> {
    let token = random_token()?;
    let security = Security::current_user()?;
    let flags = PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE;
    let mode = PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS;
    let pipe = unsafe { CreateNamedPipeW(&pipe_name(&token), flags, mode, 1, 4096, 4096, 0, Some(&security.attributes())) };
    if pipe == INVALID_HANDLE_VALUE { return Err(io::Error::last_os_error()); }
    let pipe = unsafe { owned(pipe) };
    let (host_input, app_input) = local_pipe()?;
    let (app_output, host_output) = local_pipe()?;
    let offer = encode_offer([raw(&app_output).0 as usize, raw(&app_input).0 as usize, signal.0 as usize, client.0 as usize]);
    launch(&token)?;
    tracing::info!("default terminal attachment process started");
    let deadline = Instant::now() + TRANSFER_TIMEOUT;
    let handle = raw(&pipe);
    transfer(handle, deadline, |overlapped| unsafe { ConnectNamedPipe(handle, Some(overlapped)) })?;
    let mut pid = 0;
    unsafe { GetNamedPipeClientProcessId(handle, &mut pid) }.map_err(os)?;
    let _target = checked_peer(pid, PROCESS_ACCESS_RIGHTS(0))?;
    tracing::info!(pid, "default terminal attachment peer authenticated");
    send_exact(handle, deadline, &offer)?;
    let mut ack = [0; ACK.len()];
    receive_exact(handle, deadline, &mut ack)?;
    if ack != ACK { return Err(io::ErrorKind::InvalidData.into()); }
    Ok((host_input, host_output))
}

fn launch_attachment(token: &str) -> io::Result<()> {
    Command::new(std::env::current_exe()?).args([CLIENT_ARG, token])
        .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(DETACHED_PROCESS.0).spawn().map(|_| ())
}

mod handoff_abi {
    #![allow(clippy::too_many_arguments)]

    use super::{c_void, HANDLE, HRESULT};
    use windows_core::{interface, IUnknown, IUnknown_Vtbl};

    #[interface("6F23DA90-15C5-4203-9DB0-64E73F1B1B00")]
    pub(super) unsafe trait ITerminalHandoff3: IUnknown {
        unsafe fn establish_pty_handoff(
            &self,
            input: *mut HANDLE,
            output: *mut HANDLE,
            signal: HANDLE,
            reference: HANDLE,
            server: HANDLE,
            client: HANDLE,
            startup_info: *const c_void,
        ) -> HRESULT;
    }
}

use handoff_abi::{ITerminalHandoff3, ITerminalHandoff3_Impl};

#[implement(ITerminalHandoff3, Agile = false)]
struct TerminalHandoff {
    lifetime: Arc<ServerLifetime>,
    used: AtomicBool,
}

impl ITerminalHandoff3_Impl for TerminalHandoff_Impl {
    unsafe fn establish_pty_handoff(
        &self,
        input: *mut HANDLE,
        output: *mut HANDLE,
        signal: HANDLE,
        _reference: HANDLE,
        _server: HANDLE,
        client: HANDLE,
        _startup_info: *const c_void,
    ) -> HRESULT {
        tracing::info!("default terminal broker received a console handoff");
        if input.is_null() || output.is_null() { return E_POINTER; }
        *input = HANDLE::default();
        *output = HANDLE::default();
        if signal.is_invalid() || client.is_invalid() { return E_INVALIDARG; }
        if self.used.swap(true, Ordering::AcqRel) { return E_FAIL; }
        match relay(signal, client, launch_attachment) {
            Ok((read, write)) => {
                *input = HANDLE(read.into_raw_handle());
                *output = HANDLE(write.into_raw_handle());
                tracing::info!("default terminal broker transferred the console streams");
                HRESULT(0)
            }
            Err(error) => {
                tracing::warn!(%error, "default terminal handoff failed");
                error.raw_os_error().map_or(E_FAIL, |code| HRESULT::from_win32(code as u32))
            }
        }
    }
}

impl Drop for TerminalHandoff {
    fn drop(&mut self) { let _ = unsafe { SetEvent(raw(&self.lifetime.finished)) }; }
}

struct ServerLifetime {
    started: OwnedHandle,
    finished: OwnedHandle,
}

#[implement(IClassFactory)]
struct Factory {
    lifetime: Arc<ServerLifetime>,
    created: AtomicBool,
}

impl IClassFactory_Impl for Factory_Impl {
    fn CreateInstance(&self, outer: Ref<IUnknown>, iid: *const GUID, object: *mut *mut c_void) -> windows_core::Result<()> {
        if iid.is_null() || object.is_null() { return Err(E_POINTER.into()); }
        tracing::info!(iid = ?unsafe { &*iid }, "default terminal COM activation requested");
        unsafe { *object = std::ptr::null_mut() };
        if outer.as_ref().is_some() { return Err(CLASS_E_NOAGGREGATION.into()); }
        if self.created.swap(true, Ordering::AcqRel) { return Err(E_FAIL.into()); }
        unsafe { SetEvent(raw(&self.lifetime.started))? };
        let handoff: IUnknown = TerminalHandoff { lifetime: self.lifetime.clone(), used: AtomicBool::new(false) }.into();
        unsafe { handoff.query(iid, object).ok() }
    }

    fn LockServer(&self, _: BOOL) -> windows_core::Result<()> { Ok(()) }
}

struct Apartment;

impl Apartment {
    fn enter() -> windows_core::Result<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok()? };
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) { unsafe { CoUninitialize() }; }
}

struct ClassRegistration(u32);

impl Drop for ClassRegistration {
    fn drop(&mut self) { let _ = unsafe { CoRevokeClassObject(self.0) }; }
}

fn register_proxy() -> windows_core::Result<ClassRegistration> {
    let mut raw_factory = std::ptr::null_mut();
    unsafe { AsterDeftermProxyGetClassObject(&PROXY_CLSID, &PROXY_FACTORY_IID, &mut raw_factory).ok()? };
    if raw_factory.is_null() { return Err(E_POINTER.into()); }
    let factory = unsafe { IUnknown::from_raw(raw_factory) };
    let cookie = unsafe { CoRegisterClassObject(&PROXY_CLSID, &factory, CLSCTX_INPROC_SERVER, REGCLS_MULTIPLEUSE)? };
    let registration = ClassRegistration(cookie);
    unsafe { CoRegisterPSClsid(&ITerminalHandoff3::IID, &PROXY_CLSID)? };
    Ok(registration)
}

pub fn run_server() -> Result<(), PlatformError> {
    let _apartment = Apartment::enter().map_err(crate::err)?;
    tracing::info!("default terminal broker COM initialized");
    let _proxy = register_proxy().map_err(crate::err)?;
    tracing::info!("default terminal broker proxy registered");
    let lifetime = Arc::new(ServerLifetime { started: manual_event().map_err(platform)?, finished: manual_event().map_err(platform)? });
    let factory: IClassFactory = Factory { lifetime: lifetime.clone(), created: AtomicBool::new(false) }.into();
    let cookie = unsafe { CoRegisterClassObject(&TERMINAL_CLSID, &factory, CLSCTX_LOCAL_SERVER, REGCLS_SINGLEUSE) }.map_err(crate::err)?;
    let _server = ClassRegistration(cookie);
    tracing::info!("default terminal COM class registered");
    let mut waited = unsafe { WaitForSingleObject(raw(&lifetime.started), SERVER_TIMEOUT_MS) };
    if waited == WAIT_OBJECT_0 {
        waited = unsafe { WaitForSingleObject(raw(&lifetime.finished), SERVER_TIMEOUT_MS) };
    }
    if waited == WAIT_TIMEOUT { tracing::warn!("default terminal broker timed out"); }
    if waited == WAIT_OBJECT_0 || waited == WAIT_TIMEOUT { Ok(()) } else { Err(platform(io::Error::last_os_error())) }
}

fn resize_packet(cols: u16, rows: u16) -> [u8; 6] {
    let [col_low, col_high] = cols.clamp(1, i16::MAX as u16).to_le_bytes();
    let [row_low, row_high] = rows.clamp(1, i16::MAX as u16).to_le_bytes();
    [8, 0, col_low, col_high, row_low, row_high]
}

struct Resize {
    cols: u16,
    rows: u16,
    repaint: bool,
}

#[derive(Default)]
struct SignalState {
    pending: Option<Resize>,
    stopped: bool,
}

#[derive(Default)]
struct Signal {
    state: Mutex<SignalState>,
    wake: Condvar,
}

impl Signal {
    fn resize(&self, cols: u16, rows: u16, repaint: bool) {
        if let Ok(mut state) = self.state.lock() {
            if !state.stopped {
                state.pending = Some(Resize { cols, rows, repaint });
                self.wake.notify_one();
            }
        }
    }

    fn stop(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.stopped = true;
            self.wake.notify_one();
        }
    }

    fn next(&self) -> Option<Resize> {
        let mut state = self.wake.wait_while(self.state.lock().ok()?, |state| !state.stopped && state.pending.is_none()).ok()?;
        if state.stopped { None } else { state.pending.take() }
    }

    fn run(&self, mut pipe: File) -> io::Result<()> {
        while let Some(Resize { cols, rows, repaint }) = self.next() {
            if repaint {
                pipe.write_all(&resize_packet(if cols > 2 { cols - 1 } else { cols + 1 }, rows))?;
            }
            pipe.write_all(&resize_packet(cols, rows))?;
        }
        Ok(())
    }
}

struct AttachedPty {
    signal: Arc<Signal>,
    client: OwnedHandle,
    pid: u32,
}

impl Drop for AttachedPty {
    fn drop(&mut self) { self.signal.stop(); }
}

struct PtyHandle(Arc<AttachedPty>);

impl Pty for PtyHandle {
    fn resize(&self, cols: u16, rows: u16) { self.0.signal.resize(cols, rows, false); }
    fn force_resize(&self, cols: u16, rows: u16) { self.0.signal.resize(cols, rows, true); }
    fn kill(&self) { self.0.signal.stop(); }
    fn pid(&self) -> u32 { self.0.pid }
}

struct OutputReader {
    pipe: File,
    finished: Arc<OwnedHandle>,
}

impl Read for OutputReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        let result = self.pipe.read(bytes);
        if !bytes.is_empty() && matches!(result, Ok(0) | Err(_)) {
            let _ = unsafe { SetEvent(raw(&self.finished)) };
        }
        result
    }
}

impl Drop for OutputReader {
    fn drop(&mut self) { let _ = unsafe { SetEvent(raw(&self.finished)) }; }
}

fn attach_from_pipe(token: &str) -> io::Result<Spawned> {
    if !valid_token(token) { return Err(io::ErrorKind::InvalidInput.into()); }
    let access = GENERIC_READ.0 | GENERIC_WRITE.0;
    let flags = FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION;
    let pipe = unsafe { CreateFileW(&pipe_name(token), access, FILE_SHARE_MODE(0), None, OPEN_EXISTING, flags, None).map(|handle| owned(handle)) }.map_err(os)?;
    let handle = raw(&pipe);
    let mut broker_pid = 0;
    unsafe { GetNamedPipeServerProcessId(handle, &mut broker_pid) }.map_err(os)?;
    let broker = checked_peer(broker_pid, PROCESS_DUP_HANDLE)?;
    let deadline = Instant::now() + TRANSFER_TIMEOUT;
    let mut offer = [0; OFFER_LEN];
    receive_exact(handle, deadline, &mut offer)?;
    let [output, input, signal, client] = decode_offer(offer)?;
    let output = duplicate_from(raw(&broker), output)?;
    let input = duplicate_from(raw(&broker), input)?;
    let signal_pipe = duplicate_from(raw(&broker), signal)?;
    let client = duplicate_from(raw(&broker), client)?;
    let pid = unsafe { GetProcessId(raw(&client)) };
    if pid == 0 { return Err(io::Error::last_os_error()); }
    let finished = Arc::new(manual_event()?);
    let reader = OutputReader { pipe: File::from(output), finished: finished.clone() };
    let signal = Arc::new(Signal::default());
    let inner = Arc::new(AttachedPty { signal: signal.clone(), client, pid });
    std::thread::Builder::new().name("aster-defterm-signal".into())
        .spawn(move || { let _ = signal.run(File::from(signal_pipe)); })?;
    send_exact(handle, deadline, &ACK)?;
    let waiter = PtyHandle(inner.clone());
    let wait = Box::new(move || {
        let process = raw(&waiter.0.client);
        let drained = unsafe { WaitForSingleObject(raw(&finished), INFINITE) } == WAIT_OBJECT_0;
        let code = if drained && unsafe { WaitForSingleObject(process, 0) } == WAIT_OBJECT_0 {
            let mut code = 0;
            unsafe { GetExitCodeProcess(process, &mut code) }.ok().map(|_| code)
        } else {
            None
        };
        waiter.kill();
        code
    });
    Ok(Spawned { pty: Box::new(PtyHandle(inner)), reader: Box::new(reader), writer: Box::new(File::from(input)), raw: None, wait })
}

pub fn attach(token: &str) -> Result<Spawned, PlatformError> { attach_from_pipe(token).map_err(platform) }

#[cfg(test)]
#[path = "../test/defterm.rs"]
mod tests;
