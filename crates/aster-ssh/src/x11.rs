use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

const PROTOCOL: &[u8] = b"MIT-MAGIC-COOKIE-1";
const LIMIT: Duration = Duration::from_secs(10);
const MAX_AUTHORITY: u64 = 4 * 1024 * 1024;
type Cookie = [u8; 16];

#[derive(Clone)]
struct Display {
    number: u16,
    screen: u32,
    unix: bool,
}

impl Display {
    fn parse(value: &str) -> Result<Self, String> {
        let (host, tail) = value.trim().split_once(':').ok_or("Expected a local X11 display such as :0 or localhost:0")?;
        let unix = match host {
            "" | "unix" | "unix/" => cfg!(target_os = "linux"),
            "localhost" | "127.0.0.1" | "tcp/localhost" | "tcp/127.0.0.1" => false,
            _ => return Err("The X11 display must be local".into()),
        };
        let (number, screen) = tail.split_once('.').unwrap_or((tail, "0"));
        let number = number.parse::<u16>().ok().filter(|n| unix || n.checked_add(6000).is_some()).ok_or("Invalid X11 display number")?;
        let screen = screen.parse().map_err(|_| "Invalid X11 screen number")?;
        Ok(Self { number, screen, unix })
    }

    async fn connect(&self) -> io::Result<Box<dyn crate::forward::Stream>> {
        #[cfg(target_os = "linux")]
        if self.unix {
            let path = format!("/tmp/.X11-unix/X{}", self.number);
            return Ok(Box::new(tokio::net::UnixStream::connect(path).await?));
        }
        Ok(Box::new(tokio::net::TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, 6000 + self.number)).await?))
    }
}

pub(crate) struct X11 {
    display: Display,
    cookie: Cookie,
    local_cookie: Option<Cookie>,
}

impl X11 {
    pub(crate) fn prepare(setting: &str, home: &Path) -> Result<Self, String> {
        let value = if setting.trim().is_empty() {
            std::env::var("DISPLAY").ok().filter(|v| !v.trim().is_empty()).or_else(|| cfg!(windows).then(|| "localhost:0".into())).ok_or("No local X11 display configured")?
        } else {
            setting.to_owned()
        };
        let display = Display::parse(&value)?;
        let path = std::env::var_os("XAUTHORITY").filter(|s| !s.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".Xauthority"));
        let local_cookie = read_authority(&path, &display).map_err(|e| format!("Cannot read X11 authority: {e}"))?;
        let mut cookie = [0; 16];
        getrandom::fill(&mut cookie).map_err(|_| "Cannot generate an X11 cookie")?;
        Ok(Self { display, cookie, local_cookie })
    }

    pub(crate) fn cookie_hex(&self) -> String {
        self.cookie.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub(crate) fn screen(&self) -> u32 {
        self.display.screen
    }
}

fn read_authority(path: &Path, display: &Display) -> io::Result<Option<Cookie>> {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut data = Vec::new();
    file.take(MAX_AUTHORITY + 1).read_to_end(&mut data)?;
    if data.len() as u64 > MAX_AUTHORITY {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "authority file too large"));
    }
    let mut hostname = std::env::var("XAUTHLOCALHOSTNAME").ok().filter(|s| !s.is_empty());
    #[cfg(target_os = "linux")]
    if hostname.is_none() {
        hostname = std::fs::read_to_string("/proc/sys/kernel/hostname").ok();
    }
    if hostname.is_none() {
        hostname = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).ok();
    }
    authority(&data, display, hostname.unwrap_or_default().trim_end().as_bytes())
}

fn field<'a>(input: &mut &'a [u8]) -> Option<&'a [u8]> {
    let (prefix, rest) = input.split_at_checked(2)?;
    let n = u16::from_be_bytes(prefix.try_into().ok()?) as usize;
    let (field, rest) = rest.split_at_checked(n)?;
    *input = rest;
    Some(field)
}

fn authority(mut data: &[u8], display: &Display, hostname: &[u8]) -> io::Result<Option<Cookie>> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid authority record");
    let number = display.number.to_string();
    let mut selected = None;
    let mut best = 0;
    while !data.is_empty() {
        let (family, rest) = data.split_at_checked(2).ok_or_else(invalid)?;
        data = rest;
        let address = field(&mut data).ok_or_else(invalid)?;
        let display_number = field(&mut data).ok_or_else(invalid)?;
        let name = field(&mut data).ok_or_else(invalid)?;
        let secret = field(&mut data).ok_or_else(invalid)?;
        if name != PROTOCOL || secret.len() != 16 || (!display_number.is_empty() && display_number != number.as_bytes()) {
            continue;
        }
        let rank = match u16::from_be_bytes([family[0], family[1]]) {
            0 if !display.unix && address == [127, 0, 0, 1] => 5,
            256 if !hostname.is_empty() && address == hostname => 3,
            65535 => 1,
            _ => continue,
        } + u8::from(!display_number.is_empty());
        if rank > best {
            best = rank;
            selected = secret.try_into().ok();
        }
    }
    Ok(selected)
}

fn same_cookie(a: &Cookie, b: &Cookie) -> bool {
    a.iter().zip(b).fold(0u8, |diff, (x, y)| diff | (x ^ y)) == 0
}

#[derive(Clone, Default)]
pub(crate) struct Grants(Arc<Mutex<Vec<Weak<X11>>>>);

impl Grants {
    pub(crate) fn register(&self, session: &Arc<X11>) {
        let mut entries = self.0.lock().unwrap();
        entries.retain(|entry| entry.strong_count() != 0);
        entries.push(Arc::downgrade(session));
    }

    pub(crate) fn active(&self) -> bool {
        self.0.lock().unwrap().iter().any(|entry| entry.strong_count() != 0)
    }

    fn lookup(&self, cookie: &Cookie) -> Option<(Display, Option<Cookie>)> {
        let mut entries = self.0.lock().unwrap();
        entries.retain(|entry| entry.strong_count() != 0);
        entries.iter().filter_map(Weak::upgrade).find(|g| same_cookie(&g.cookie, cookie)).map(|g| (g.display.clone(), g.local_cookie))
    }

    pub(crate) async fn relay(&self, mut incoming: impl AsyncRead + AsyncWrite + Unpin) -> io::Result<()> {
        let timed_out = |_| io::Error::new(io::ErrorKind::TimedOut, "X11 setup timed out");
        let (order, cookie) = tokio::time::timeout(LIMIT, read_setup(&mut incoming)).await.map_err(timed_out)??;
        let (display, local_cookie) = self.lookup(&cookie).ok_or_else(|| io::Error::new(io::ErrorKind::PermissionDenied, "X11 cookie rejected"))?;
        let mut local = tokio::time::timeout(LIMIT, display.connect()).await.map_err(timed_out)??;
        local.write_all(&setup(order, local_cookie.as_ref())).await?;
        crate::forward::pump(&mut incoming, &mut local).await
    }
}

async fn read_setup(input: &mut (impl AsyncRead + Unpin)) -> io::Result<(u8, Cookie)> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidData, "invalid X11 connection setup");
    let mut header = [0; 12];
    input.read_exact(&mut header).await?;
    let word = match header[0] {
        b'l' => u16::from_le_bytes,
        b'B' => u16::from_be_bytes,
        _ => return Err(invalid()),
    };
    let get = |i: usize| word([header[i], header[i + 1]]);
    if (get(2), get(4), get(6), get(8)) != (11, 0, PROTOCOL.len() as u16, 16) {
        return Err(invalid());
    }
    let mut name = [0; 20];
    let mut cookie = [0; 16];
    input.read_exact(&mut name).await?;
    if &name[..PROTOCOL.len()] != PROTOCOL {
        return Err(invalid());
    }
    input.read_exact(&mut cookie).await?;
    Ok((header[0], cookie))
}

fn setup(order: u8, cookie: Option<&Cookie>) -> Vec<u8> {
    let word = if order == b'l' { u16::to_le_bytes } else { u16::to_be_bytes };
    let mut data = vec![0; 12];
    data[0] = order;
    data[2..4].copy_from_slice(&word(11));
    if let Some(cookie) = cookie {
        data[6..8].copy_from_slice(&word(PROTOCOL.len() as u16));
        data[8..10].copy_from_slice(&word(cookie.len() as u16));
        data.extend_from_slice(PROTOCOL);
        data.resize(32, 0);
        data.extend_from_slice(cookie);
    }
    data
}

#[cfg(test)]
#[path = "../test/x11.rs"]
mod tests;
