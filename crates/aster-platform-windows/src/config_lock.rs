use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Storage::FileSystem::{LockFileEx, LOCKFILE_EXCLUSIVE_LOCK};
use windows::Win32::System::IO::OVERLAPPED;

pub fn lock(path: &Path) -> io::Result<File> {
    let file = OpenOptions::new().write(true).create(true).truncate(false).open(path)?;
    let mut offset = OVERLAPPED::default();
    unsafe { LockFileEx(HANDLE(file.as_raw_handle()), LOCKFILE_EXCLUSIVE_LOCK, None, 1, 0, &mut offset) }
        .map_err(io::Error::other)?;
    Ok(file)
}
