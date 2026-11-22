use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn http_connect_preserves_the_ssh_banner() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut peer, _) = listener.accept().await.unwrap();
        let request = b"CONNECT [::1]:22 HTTP/1.1\r\nHost: [::1]:22\r\n\r\n";
        let mut received = vec![0; request.len()];
        peer.read_exact(&mut received).await.unwrap();
        assert_eq!(received, request);
        peer.write_all(b"HTTP/1.1 200 Connection Established\r\nProxy-Agent: Test\r\n\r\nSSH-2.0-test\r\n").await.unwrap();
    });
    let mut stream = TcpStream::connect(address).await.unwrap();
    http_connect(&mut stream, "::1", 22).await.unwrap();
    let mut banner = String::new();
    stream.read_to_string(&mut banner).await.unwrap();
    assert_eq!(banner, "SSH-2.0-test\r\n");
    server.await.unwrap();
}

#[tokio::test]
async fn http_connect_rejects_proxy_failure() {
    let (mut stream, mut peer) = tokio::io::duplex(256);
    peer.write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n").await.unwrap();
    assert!(http_connect(&mut stream, "host", 22).await.is_err());
}

#[tokio::test]
async fn socks5_connect_passes_the_domain_and_preserves_tunnel_data() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        let (mut peer, _) = listener.accept().await.unwrap();
        let mut greeting = [0; 3];
        peer.read_exact(&mut greeting).await.unwrap();
        assert_eq!(greeting, [5, 1, 0]);
        peer.write_all(&[5, 0]).await.unwrap();
        let mut request = [0; 11];
        peer.read_exact(&mut request).await.unwrap();
        assert_eq!(&request, b"\x05\x01\x00\x03\x04host\x08\xae");
        peer.write_all(b"\x05\x00\x00\x03\x02ok\x00\x00SSH").await.unwrap();
    });
    let mut stream = TcpStream::connect(address).await.unwrap();
    socks5_connect(&mut stream, "host", 2222).await.unwrap();
    let mut banner = String::new();
    stream.read_to_string(&mut banner).await.unwrap();
    assert_eq!(banner, "SSH");
    server.await.unwrap();
}

#[tokio::test]
async fn socks5_connect_rejects_an_unoffered_authentication_method() {
    let (mut stream, mut peer) = tokio::io::duplex(256);
    peer.write_all(&[5, 2]).await.unwrap();
    assert!(socks5_connect(&mut stream, "host", 22).await.is_err());
}
