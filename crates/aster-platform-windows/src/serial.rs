use aster_proto::serial::{Parity, Settings, StopBits};
use std::io;
use std::os::windows::io::RawHandle;
use windows::Win32::Devices::Communication::{DCB, GetCommState, SetCommState, MARKPARITY, ONE5STOPBITS, SPACEPARITY};
use windows::Win32::Foundation::HANDLE;

pub fn configure(handle: RawHandle, settings: &Settings) -> io::Result<()> {
    let parity = match settings.parity {
        Parity::Mark => Some(MARKPARITY),
        Parity::Space => Some(SPACEPARITY),
        _ => None,
    };
    let fractional = settings.stop_bits == StopBits::OneAndHalf;
    if parity.is_none() && !fractional {
        return Ok(());
    }
    let handle = HANDLE(handle);
    let mut dcb = DCB { DCBlength: std::mem::size_of::<DCB>() as u32, ..DCB::default() };
    unsafe { GetCommState(handle, &mut dcb) }.map_err(io::Error::other)?;
    if let Some(parity) = parity {
        dcb.Parity = parity;
    }
    if fractional {
        dcb.StopBits = ONE5STOPBITS;
    }
    unsafe { SetCommState(handle, &dcb) }.map_err(io::Error::other)
}
