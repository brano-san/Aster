use super::*;
use std::time::Duration;
use tokio::sync::mpsc;

struct Tcp;
impl Opener for Tcp {
    fn open(&self, host: String, port: u16) -> Opened {
        Box::pin(async move { TcpStream::connect((host.as_str(), port)).await.map(|s| Box::new(s) as Box<dyn Stream>).map_err(|e| e.to_string()) })
    }
}
struct Refuse;
impl Opener for Refuse { fn open(&self, _: String, _: u16) -> Opened { Box::pin(async { Err("refused by test".into()) }) } }
fn run(f: impl Future<Output = ()>) {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async { tokio::time::timeout(Duration::from_secs(5), f).await.unwrap(); });
}
async fn echo() -> u16 {
    let listener=TcpListener::bind("127.0.0.1:0").await.unwrap(); let port=listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _=tokio::time::timeout(Duration::from_secs(5), async {
            loop { let (mut s,_)=listener.accept().await.unwrap(); tokio::spawn(async move {
                let _=tokio::time::timeout(Duration::from_secs(5),async { let (mut r,mut w)=s.split(); let _=tokio::io::copy(&mut r,&mut w).await; }).await;
            }); }
        }).await;
    }); port
}
fn local(port:u16) -> Forward { Forward::Local { bind:"127.0.0.1".into(),port:0,host:"127.0.0.1".into(),host_port:port } }
fn dynamic() -> Forward { Forward::Dynamic { bind:"127.0.0.1".into(),port:0 } }
async fn launch(f:Forward,opener:Arc<dyn Opener>) -> (Running,TcpStream,mpsc::UnboundedReceiver<Event>) {
    let (tx,mut rx)=mpsc::unbounded_channel(); let running=start(f,opener,tx).await;
    let Event::Listening { port,.. }=rx.recv().await.unwrap() else { panic!("not listening") };
    let socket=TcpStream::connect(("127.0.0.1",port)).await.unwrap(); (running,socket,rx)
}
async fn check_echo(s:&mut TcpStream) {
    s.write_all(b"hello").await.unwrap(); let mut b=[0;5]; s.read_exact(&mut b).await.unwrap(); assert_eq!(&b,b"hello");
}
fn request(port:u16) -> Vec<u8> { let mut b=b"\x05\x01\x00\x03\x09localhost".to_vec(); b.extend(port.to_be_bytes()); b }

#[test]
fn parses_both_local_forms() {
    let expected=Forward::Local{bind:"127.0.0.1".into(),port:8080,host:"host".into(),host_port:80};
    assert_eq!(parse("LOCAL","8080 host:80").unwrap(),expected); assert_eq!(parse("local","8080:host:80").unwrap(),expected);
}
#[test]
fn parses_both_remote_forms() {
    let expected=Forward::Remote{bind:"localhost".into(),port:8080,host:"host".into(),host_port:80};
    assert_eq!(parse("remote","localhost:8080   host:80").unwrap(),expected); assert_eq!(parse("remote","localhost:8080:host:80").unwrap(),expected);
}
#[test]
fn parses_wildcard_and_ipv6() {
    assert_eq!(parse("dynamic","*:0").unwrap(),Forward::Dynamic{bind:"0.0.0.0".into(),port:0});
    let expected=Forward::Local{bind:"::1".into(),port:8080,host:"fe80::2".into(),host_port:80};
    assert_eq!(parse("local","[::1]:8080 [fe80::2]:80").unwrap(),expected); assert_eq!(parse("local","[::1]:8080:[fe80::2]:80").unwrap(),expected);
}
#[test]
fn parses_dynamic_with_or_without_bind() {
    assert_eq!(parse("dynamic","0").unwrap(),dynamic()); assert_eq!(parse("dynamic","localhost:65535").unwrap(),Forward::Dynamic{bind:"localhost".into(),port:65535});
}
#[test]
fn rejects_invalid_forward_fields() {
    for (kind,spec) in [("local","70000 h:80"),("local","80"),("unknown","80"),("local","80 h:0"),("dynamic","-1"),("dynamic","[::1:80"),("local","80 :80"),("dynamic","80 extra"),("local","80 h:80:90")] { assert!(parse(kind,spec).is_err(),"{kind} {spec}"); }
}
#[test]
fn local_forwards_bytes_and_reports_opened() { run(async {
    let port=echo().await; let (_running,mut s,mut events)=launch(local(port),Arc::new(Tcp)).await;
    check_echo(&mut s).await; assert!(matches!(events.recv().await.unwrap(),Event::Opened{port:p,..} if p==port));
}); }
#[test]
fn local_refusal_closes_the_socket() { run(async {
    let (_running,mut s,mut events)=launch(local(80),Arc::new(Refuse)).await;
    assert!(matches!(events.recv().await.unwrap(),Event::Refused{..})); assert_eq!(s.read(&mut [0;1]).await.unwrap(),0);
}); }
#[test]
fn dynamic_five_forwards_domain_requests() { run(async {
    let port=echo().await; let (_running,mut s,mut events)=launch(dynamic(),Arc::new(Tcp)).await;
    s.write_all(&[5,1,0]).await.unwrap(); let mut greeting=[0;2]; s.read_exact(&mut greeting).await.unwrap(); assert_eq!(greeting,[5,0]);
    s.write_all(&request(port)).await.unwrap(); let mut reply=[0;10]; s.read_exact(&mut reply).await.unwrap(); assert_eq!(reply[1],0);
    check_echo(&mut s).await; assert!(matches!(events.recv().await.unwrap(),Event::Opened{host,..} if host=="localhost"));
}); }
#[test]
fn dynamic_preserves_pipelined_payload() { run(async {
    let port=echo().await; let (_running,mut s,_events)=launch(dynamic(),Arc::new(Tcp)).await;
    let mut b=vec![5,1,0]; b.extend(request(port)); b.extend(b"hello"); s.write_all(&b).await.unwrap();
    let mut response=[0;17]; s.read_exact(&mut response).await.unwrap(); assert_eq!(&response[..2],&[5,0]); assert_eq!(&response[12..],b"hello");
}); }
#[test]
fn dynamic_four_a_forwards_domain_requests() { run(async {
    let port=echo().await; let (_running,mut s,_events)=launch(dynamic(),Arc::new(Tcp)).await;
    let mut b=vec![4,1]; b.extend(port.to_be_bytes()); b.extend(b"\x00\x00\x00\x01\x00localhost\x00"); s.write_all(&b).await.unwrap();
    let mut reply=[0;8]; s.read_exact(&mut reply).await.unwrap(); assert_eq!(reply,[0,90,0,0,0,0,0,0]); check_echo(&mut s).await;
}); }
#[test]
fn dynamic_refusal_returns_failure() { run(async {
    let (_running,mut s,mut events)=launch(dynamic(),Arc::new(Refuse)).await;
    let mut b=vec![5,1,0]; b.extend(request(80)); s.write_all(&b).await.unwrap();
    let mut reply=[0;12]; s.read_exact(&mut reply).await.unwrap(); assert_eq!(reply[3],1); assert!(matches!(events.recv().await.unwrap(),Event::Refused{..}));
}); }
#[test]
fn occupied_port_reports_failure() { run(async {
    let listener=TcpListener::bind("127.0.0.1:0").await.unwrap(); let port=listener.local_addr().unwrap().port(); let (tx,mut rx)=mpsc::unbounded_channel();
    let _running=start(Forward::Dynamic{bind:"127.0.0.1".into(),port},Arc::new(Tcp),tx).await; assert!(matches!(rx.recv().await.unwrap(),Event::Failed{..}));
}); }
#[test]
fn stop_closes_listener_but_keeps_open_streams() { run(async {
    let target=echo().await; let (running,mut s,mut events)=launch(local(target),Arc::new(Tcp)).await; let address=s.peer_addr().unwrap();
    assert!(matches!(events.recv().await.unwrap(),Event::Opened{..})); running.stop(); running.stop();
    if let Ok(mut new)=TcpStream::connect(address).await { assert_eq!(new.read(&mut [0;1]).await.unwrap(),0); }
    check_echo(&mut s).await;
}); }
#[test]
fn remote_is_delegated_to_the_session() { run(async {
    let (tx,mut rx)=mpsc::unbounded_channel(); let _running=start(parse("remote","0 host:80").unwrap(),Arc::new(Tcp),tx).await;
    assert!(matches!(rx.recv().await.unwrap(),Event::Failed{error,..} if error=="remote forwards are started by the session"));
}); }
#[test]
fn dropping_running_closes_listener() { run(async {
    let (running,s,_events)=launch(dynamic(),Arc::new(Tcp)).await; let address=s.peer_addr().unwrap(); drop(running);
    if let Ok(mut new)=TcpStream::connect(address).await { assert_eq!(new.read(&mut [0;1]).await.unwrap(),0); }
}); }
