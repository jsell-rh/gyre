use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

#[test]
fn probe_std_tcp_blocking() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut sock, peer)) = listener.accept() {
            eprintln!("STD ACCEPTED from {peer}");
            let mut buf = [0u8; 1024];
            let n = sock.read(&mut buf).unwrap_or(0);
            eprintln!("STD READ {n}");
            let _ = sock.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        } else {
            eprintln!("STD ACCEPT FAILED");
        }
    });
    std::thread::sleep(std::time::Duration::from_millis(200));
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("std connect");
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n")
        .unwrap();
    let mut buf = [0u8; 1024];
    let n = stream.read(&mut buf).unwrap_or(0);
    eprintln!("STD RESPONSE {n} bytes: {:?}", String::from_utf8_lossy(&buf[..n]));
    assert!(n > 0, "std blocking TCP also broken");
}
