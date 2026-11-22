use aster_proto::ssh::{proxy_endpoint, SshProxy};
use std::io::{self, ErrorKind};
use std::net::IpAddr;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

pub(crate) async fn connect(proxy: &SshProxy, host: &str, port: u16) -> Result<TcpStream, String> {
    let address = match proxy {
        SshProxy::Socks { address } | SshProxy::Http { address } => address,
        _ => return Err("invalid proxy transport".into()),
    };
    let endpoint = proxy_endpoint(address).ok_or("the proxy address must be host:port")?;
    let mut stream = TcpStream::connect(endpoint).await.map_err(|e| format!("cannot connect to the proxy {address}: {e}"))?;
    let handshake = match proxy {
        SshProxy::Socks { .. } => socks5_connect(&mut stream, host, port).await,
        _ => http_connect(&mut stream, host, port).await,
    };
    handshake.map_err(|e| format!("proxy {address} could not open {host}:{port}: {e}"))?;
    Ok(stream)
}

async fn http_connect(stream: &mut (impl AsyncRead + AsyncWrite + Unpin), host: &str, port: u16) -> io::Result<()> {
    let authority = if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") };
    if proxy_endpoint(&authority).is_none() {
        return Err(io::Error::new(ErrorKind::InvalidInput, "invalid CONNECT authority"));
    }
    stream.write_all(format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n").as_bytes()).await?;
    stream.flush().await?;
    let mut header = Vec::with_capacity(256);
    for _ in 0..16_384 {
        header.push(stream.read_u8().await?);
        if !header.ends_with(b"\r\n\r\n") { continue; }
        let status = header.split(|b| *b == b'\r').next().and_then(|line| std::str::from_utf8(line).ok());
        let Some((version, code)) = status.and_then(|line| line.split_once(' ')) else { break };
        let code = code.split(' ').next().filter(|c| c.len() == 3).and_then(|c| c.parse::<u16>().ok());
        if !matches!(version, "HTTP/1.0" | "HTTP/1.1") { break; }
        match code {
            Some(100..=199) => header.clear(),
            Some(200..=299) => return Ok(()),
            Some(code) => return Err(io::Error::other(format!("HTTP CONNECT returned {code}"))),
            None => break,
        }
    }
    Err(io::Error::new(ErrorKind::InvalidData, "invalid or oversized HTTP CONNECT response"))
}

async fn socks5_connect(stream: &mut (impl AsyncRead + AsyncWrite + Unpin), host: &str, port: u16) -> io::Result<()> {
    stream.write_all(&[5, 1, 0]).await?;
    stream.flush().await?;
    let mut selection = [0; 2];
    stream.read_exact(&mut selection).await?;
    if selection != [5, 0] {
        return Err(io::Error::new(ErrorKind::InvalidData, "the SOCKS5 proxy does not accept unauthenticated connections"));
    }
    let mut request = vec![5, 1, 0];
    match host.parse() {
        Ok(IpAddr::V4(ip)) => { request.push(1); request.extend(ip.octets()); }
        Ok(IpAddr::V6(ip)) => { request.push(4); request.extend(ip.octets()); }
        Err(_) if !host.is_empty() && host.len() <= 255 && !host.chars().any(|c| c.is_control() || c.is_whitespace()) => {
            request.extend([3, host.len() as u8]);
            request.extend(host.bytes());
        }
        Err(_) => return Err(io::Error::new(ErrorKind::InvalidInput, "invalid SOCKS5 host")),
    }
    request.extend(port.to_be_bytes());
    stream.write_all(&request).await?;
    stream.flush().await?;
    let mut reply = [0; 4];
    stream.read_exact(&mut reply).await?;
    if reply[0] != 5 || reply[2] != 0 {
        return Err(io::Error::new(ErrorKind::InvalidData, "invalid SOCKS5 CONNECT response"));
    }
    if reply[1] != 0 {
        return Err(io::Error::other(format!("SOCKS5 CONNECT returned {}", reply[1])));
    }
    let length = match reply[3] {
        1 => 4,
        3 => stream.read_u8().await? as usize,
        4 => 16,
        _ => return Err(io::Error::new(ErrorKind::InvalidData, "invalid SOCKS5 bound address")),
    };
    let mut address = [0; 257];
    stream.read_exact(&mut address[..length + 2]).await?;
    Ok(())
}

#[cfg(test)]
#[path = "../test/proxy.rs"]
mod tests;
