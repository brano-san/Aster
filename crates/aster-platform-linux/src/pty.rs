use aster_platform::{PlatformError, Pty, SpawnRequest, Spawned};
use std::ffi::CString;
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::time::Duration;

const TERM_AFTER: Duration = Duration::from_secs(1);
const KILL_AFTER: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

fn hang_up(mut signal: impl FnMut(libc::c_int)) {
    signal(libc::SIGHUP);
    signal(libc::SIGCONT);
}

fn escalate(mut alive: impl FnMut() -> bool, mut signal: impl FnMut(libc::c_int), mut sleep: impl FnMut(Duration)) {
    for (delay, next_signal) in [(TERM_AFTER, libc::SIGTERM), (KILL_AFTER, libc::SIGKILL)] {
        let mut remaining = delay;
        loop {
            if !alive() {
                return;
            }
            if remaining.is_zero() {
                break;
            }
            let interval = remaining.min(POLL_INTERVAL);
            sleep(interval);
            remaining -= interval;
        }
        signal(next_signal);
    }
}

fn last_error(what: &str) -> PlatformError {
    let e = std::io::Error::last_os_error();
    PlatformError::new(e.raw_os_error().unwrap_or(-1) as i64, format!("{what}: {e}"))
}

fn winsize(cols: u16, rows: u16) -> libc::winsize {
    libc::winsize { ws_row: rows.max(1), ws_col: cols.max(1), ws_xpixel: 0, ws_ypixel: 0 }
}

struct UnixPty {
    master: OwnedFd,
    pid: libc::pid_t,
}

impl Pty for UnixPty {
    fn resize(&self, cols: u16, rows: u16) {
        let ws = winsize(cols, rows);
        unsafe {
            libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &ws);
        }
    }

    fn kill(&self) {
        let pid = self.pid;
        let signal = move |sig| unsafe { libc::kill(-pid, sig); };
        hang_up(signal);
        let reaper = std::thread::Builder::new().name("aster-pty-reaper".into()).spawn(move || {
            escalate(|| unsafe { libc::kill(-pid, 0) == 0 }, signal, std::thread::sleep);
        });
        if reaper.is_err() {
            signal(libc::SIGKILL);
        }
    }

    fn pid(&self) -> u32 {
        self.pid as u32
    }
}

pub fn environment(overrides: &[(String, String)]) -> Vec<CString> {
    let mut vars: Vec<(Vec<u8>, Vec<u8>)> = std::env::vars_os().filter(|(k, _)| !matches!(k.as_bytes(), b"TERM_PROGRAM" | b"TERM_PROGRAM_VERSION"))
        .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec())).collect();
    for (k, v) in overrides {
        vars.retain(|(name, _)| name != k.as_bytes());
        vars.push((k.as_bytes().to_vec(), v.as_bytes().to_vec()));
    }
    vars.into_iter()
        .filter_map(|(mut k, v)| {
            k.push(b'=');
            k.extend_from_slice(&v);
            CString::new(k).ok()
        })
        .collect()
}

fn cstr(s: &str) -> Result<CString, PlatformError> {
    CString::new(s).map_err(|_| PlatformError::new(-1, format!("argument contains a NUL byte: {s:?}")))
}

fn cloexec(fd: RawFd) {
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC);
    }
}

pub fn spawn(req: &SpawnRequest) -> Result<Spawned, PlatformError> {
    let program = cstr(&req.command)?;
    let mut env = if let Some(spec) = &req.local_environment {
        let inherited = std::env::vars_os().filter(|(key, _)| !matches!(key.as_bytes(), b"TERM_PROGRAM" | b"TERM_PROGRAM_VERSION"))
            .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?))).collect();
        let vars: Vec<_> = spec.apply(inherited, &req.env, false).into_iter().collect();
        environment(&vars)
    } else {
        environment(&req.env)
    };
    let mut args = req.args.clone();
    let startup = crate::shells::startup(req, &mut args, &mut env);
    let mut argv: Vec<CString> = vec![program.clone()];
    for a in &args {
        argv.push(cstr(a)?);
    }
    let argv_ptrs: Vec<*const libc::c_char> = argv.iter().map(|a| a.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();
    let env_ptrs: Vec<*const libc::c_char> = env.iter().map(|e| e.as_ptr()).chain(std::iter::once(std::ptr::null())).collect();
    let cwd = req.cwd.as_ref().filter(|p| p.is_dir()).and_then(|p| CString::new(p.as_os_str().as_bytes()).ok());
    let mut master: RawFd = -1;
    let mut slave: RawFd = -1;
    let ws = winsize(req.cols, req.rows);
    if unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null(), &ws) } != 0 {
        return Err(last_error("openpty"));
    }
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        unsafe {
            libc::close(master);
            libc::close(slave);
        }
        return Err(last_error("fork"));
    }
    if pid == 0 {
        unsafe {
            libc::setsid();
            libc::ioctl(slave, libc::TIOCSCTTY, 0);
            libc::dup2(slave, 0);
            libc::dup2(slave, 1);
            libc::dup2(slave, 2);
            if slave > 2 {
                libc::close(slave);
            }
            libc::close(master);
            for sig in [libc::SIGCHLD, libc::SIGINT, libc::SIGQUIT, libc::SIGTERM, libc::SIGHUP, libc::SIGPIPE, libc::SIGALRM] {
                libc::signal(sig, libc::SIG_DFL);
            }
            let mut empty: libc::sigset_t = std::mem::zeroed();
            libc::sigemptyset(&mut empty);
            libc::sigprocmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut());
            if let Some(dir) = &cwd {
                libc::chdir(dir.as_ptr());
            }
            libc::execvpe(program.as_ptr(), argv_ptrs.as_ptr(), env_ptrs.as_ptr());
            libc::_exit(127);
        }
    }
    unsafe { libc::close(slave) };
    cloexec(master);
    let master = unsafe { OwnedFd::from_raw_fd(master) };
    let reader = master.try_clone().map_err(|e| PlatformError::new(-1, format!("dup pty: {e}")))?;
    let writer = master.try_clone().map_err(|e| PlatformError::new(-1, format!("dup pty: {e}")))?;
    let wait = Box::new(move || {
        let _startup = startup;
        let mut status = 0;
        loop {
            let r = unsafe { libc::waitpid(pid, &mut status, 0) };
            if r == pid {
                break;
            }
            if r < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EINTR) {
                return None;
            }
        }
        if libc::WIFEXITED(status) {
            Some(libc::WEXITSTATUS(status) as u32)
        } else {
            Some(128 + libc::WTERMSIG(status) as u32)
        }
    });
    Ok(Spawned { pty: Box::new(UnixPty { master, pid }), reader: Box::new(File::from(reader)), writer: Box::new(File::from(writer)), raw: None, wait })
}

#[cfg(test)]
#[path = "../test/pty.rs"]
mod tests;
