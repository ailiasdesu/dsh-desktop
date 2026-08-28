//! 极简 HTTP GET（仅用于 /quit 与 /health 探活，避免引入 reqwest 全量依赖）
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

pub fn http_get(path: &str, port: u16, timeout: Duration) -> std::io::Result<String> {
    let addr: SocketAddr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = TcpStream::connect_timeout(&addr, timeout)?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(2)));
    let req = format!(
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nUser-Agent: dsh-desktop-shell/0.1\r\n\r\n"
    );
    s.write_all(req.as_bytes())?;
    let mut buf = String::new();
    let _ = s.read_to_string(&mut buf);
    Ok(buf)
}

/// 200 判定
pub fn is_ok(resp: &str) -> bool {
    resp.starts_with("HTTP/1.1") && resp.split(' ').nth(1) == Some("200")
}
