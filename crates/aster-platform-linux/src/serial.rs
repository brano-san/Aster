use aster_proto::serial::{Flow, Settings};
use std::io;
use std::os::fd::RawFd;

pub fn configure(fd: RawFd, settings: &Settings) -> io::Result<()> {
    if settings.flow != Flow::Xany {
        return Ok(());
    }
    let mut attributes = std::mem::MaybeUninit::<libc::termios>::uninit();
    if unsafe { libc::tcgetattr(fd, attributes.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut attributes = unsafe { attributes.assume_init() };
    attributes.c_iflag |= libc::IXANY;
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &attributes) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
