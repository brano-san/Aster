use aster_platform::host::persistent::{Connection, Listener};
use std::fs::{DirBuilder, File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

const IO_TIMEOUT: Duration = Duration::from_secs(5);

fn directory() -> PathBuf {
    let uid = unsafe { libc::getuid() };
    std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from)
        .filter(|dir| dir.is_absolute() && dir.metadata().is_ok_and(|info| info.is_dir() && info.uid() == uid && info.mode() & 0o077 == 0))
        .map_or_else(|| PathBuf::from(format!("/tmp/aster-{uid}")), |dir| dir.join("aster"))
}

pub fn endpoint() -> io::Result<String> {
    directory().join("pty.sock").into_os_string().into_string().map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "terminal socket path is not UTF-8"))
}

pub fn create_token(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}

fn connection(stream: UnixStream) -> io::Result<Connection> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    let abort = stream.try_clone()?;
    Ok(Connection::new(Box::new(stream.try_clone()?), Box::new(stream.try_clone()?), move || stream.set_read_timeout(None)).with_abort(move || {
        let _ = abort.shutdown(std::net::Shutdown::Both);
    }))
}

struct SocketListener {
    name: PathBuf,
    socket: UnixListener,
    _lock: File,
}

impl Listener for SocketListener {
    fn accept(&mut self, timeout: Duration) -> io::Result<Option<Connection>> {
        let mut events = libc::pollfd { fd: self.socket.as_raw_fd(), events: libc::POLLIN, revents: 0 };
        let ready = unsafe { libc::poll(&mut events, 1, timeout.as_millis().min(i32::MAX as u128) as i32) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::Interrupted { Ok(None) } else { Err(error) };
        }
        if ready == 0 { return Ok(None); }
        match self.socket.accept() {
            Ok((stream, _)) => connection(stream).map(Some),
            Err(error) if matches!(error.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted | io::ErrorKind::ConnectionAborted) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

impl Drop for SocketListener {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.name); }
}

fn require_private(dir: &Path) -> io::Result<()> {
    let info = dir.symlink_metadata()?;
    if info.is_dir() && info.uid() == unsafe { libc::getuid() } && info.mode() & 0o077 == 0 {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::PermissionDenied, "terminal socket directory is not private"))
    }
}

pub fn listen(name: &str) -> io::Result<Box<dyn Listener>> {
    if name != endpoint()? { return Err(io::ErrorKind::InvalidInput.into()); }
    let name = PathBuf::from(name);
    let dir = name.parent().unwrap();
    match DirBuilder::new().mode(0o700).create(dir) {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        result => result?,
    }
    require_private(dir)?;
    let lock = OpenOptions::new().write(true).create(true).truncate(false).mode(0o600).custom_flags(libc::O_NOFOLLOW).open(dir.join("pty.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } < 0 {
        let error = io::Error::last_os_error();
        return Err(if error.kind() == io::ErrorKind::WouldBlock { io::ErrorKind::AddrInUse.into() } else { error });
    }
    match std::fs::remove_file(&name) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        result => result?,
    }
    let socket = UnixListener::bind(&name)?;
    let listener = SocketListener { name, socket, _lock: lock };
    listener.socket.set_nonblocking(true)?;
    std::fs::set_permissions(&listener.name, std::fs::Permissions::from_mode(0o600))?;
    Ok(Box::new(listener))
}

pub fn connect(name: &str) -> io::Result<Connection> {
    let path = Path::new(name);
    require_private(path.parent().ok_or(io::ErrorKind::InvalidInput)?)?;
    connection(UnixStream::connect(path)?)
}

pub fn start(endpoint: &str) -> io::Result<()> {
    let mut command = Command::new(std::env::current_exe()?);
    command.args(["--pty-host", endpoint]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    unsafe {
        command.pre_exec(|| if libc::setsid() < 0 { Err(io::Error::last_os_error()) } else { Ok(()) });
    }
    let mut child = command.spawn()?;
    std::thread::Builder::new().name("aster-pty-host-reap".into()).spawn(move || { let _ = child.wait(); })?;
    Ok(())
}
