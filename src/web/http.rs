//! Just enough HTTP for the phone server: reading a request head, a few
//! headers, cookies, and writing small responses.

use std::io::{Read, Write};
use std::net::TcpStream;

use anyhow::Result;

/// Longest request head we'll read before giving up.
const MAX_HEAD: usize = 16 * 1024;

/// What we need from a request head.
pub struct Request {
    pub path: String,
    pub query: String,
    pub websocket_key: Option<String>,
    pub origin: Option<String>,
    pub host: Option<String>,
    pub cookie: Option<String>,
    pub user_agent: Option<String>,
}

/// Reads up to the blank line after the headers. Whatever came in after it
/// belongs to the WebSocket.
pub fn read_head(stream: &mut TcpStream) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 2048];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buf.split_off(end + 4);
            return Ok((buf, rest));
        }
        anyhow::ensure!(buf.len() < MAX_HEAD, "request head too long");
        let n = stream.read(&mut chunk)?;
        anyhow::ensure!(n > 0, "connection closed");
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// GET requests only; anything else is `None`.
pub fn parse(head: &[u8]) -> Option<Request> {
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut req = httparse::Request::new(&mut headers);
    req.parse(head).ok()?.is_complete().then_some(())?;
    if req.method? != "GET" {
        return None;
    }
    let (path, query) = req.path?.split_once('?').unwrap_or((req.path?, ""));
    let header = |name: &str| {
        req.headers
            .iter()
            .find(|h| h.name.eq_ignore_ascii_case(name))
            .and_then(|h| std::str::from_utf8(h.value).ok())
            .map(str::to_owned)
    };
    let upgrade = header("upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket"));
    Some(Request {
        path: path.to_owned(),
        query: query.to_owned(),
        websocket_key: upgrade.then(|| header("sec-websocket-key")).flatten(),
        origin: header("origin"),
        host: header("host"),
        cookie: header("cookie"),
        user_agent: header("user-agent"),
    })
}

pub fn query_param<'q>(query: &'q str, name: &str) -> Option<&'q str> {
    query.split('&').find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
}

pub fn cookie<'c>(header: Option<&'c str>, name: &str) -> Option<&'c str> {
    header?.split(';').find_map(|pair| pair.trim().strip_prefix(name)?.strip_prefix('='))
}

/// A page from somewhere else must not be able to open the socket. The
/// origin has to match the `Host` header, or our tailnet name, since a proxy
/// may rewrite `Host` to the local address.
pub fn origin_allowed(origin: Option<&str>, host: Option<&str>, tailnet: &str) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let Some((_, from)) = origin.split_once("://") else {
        return false;
    };
    host == Some(from) || tailnet == from
}

pub fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8], extra: &[(&str, &str)]) -> Result<()> {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n",
        body.len()
    );
    for (name, value) in extra {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    Ok(())
}

pub fn redirect(stream: &mut TcpStream, to: &str, extra: &[(&str, &str)]) -> Result<()> {
    let mut headers = vec![("Location", to)];
    headers.extend_from_slice(extra);
    respond(stream, "303 See Other", "text/plain", b"", &headers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_upgrade_request() {
        let head = b"GET /ws?x=1 HTTP/1.1\r\nHost: 127.0.0.1:7878\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nOrigin: http://127.0.0.1:7878\r\nCookie: a=1; clawsh_device=abc\r\nUser-Agent: test\r\n\r\n";
        let req = parse(head).unwrap();
        assert_eq!(req.path, "/ws");
        assert_eq!(query_param(&req.query, "x"), Some("1"));
        assert_eq!(query_param(&req.query, "xx"), None);
        assert_eq!(req.websocket_key.as_deref(), Some("dGhlIHNhbXBsZSBub25jZQ=="));
        assert_eq!(req.host.as_deref(), Some("127.0.0.1:7878"));
        assert_eq!(cookie(req.cookie.as_deref(), "clawsh_device"), Some("abc"));
        assert_eq!(cookie(req.cookie.as_deref(), "clawsh"), None);
        assert_eq!(req.user_agent.as_deref(), Some("test"));
    }

    #[test]
    fn plain_get_has_no_websocket_key_and_post_is_refused() {
        let req = parse(b"GET / HTTP/1.1\r\nHost: x\r\nSec-WebSocket-Key: k\r\n\r\n").unwrap();
        assert!(req.websocket_key.is_none());
        assert!(req.cookie.is_none());
        assert!(parse(b"POST / HTTP/1.1\r\nHost: x\r\n\r\n").is_none());
    }

    #[test]
    fn origins_must_be_ours() {
        let host = Some("127.0.0.1:7878");
        let ts = "box.tail1.ts.net:7878";
        assert!(origin_allowed(None, host, ts));
        assert!(origin_allowed(Some("http://127.0.0.1:7878"), host, ts));
        assert!(origin_allowed(Some("https://box.tail1.ts.net:7878"), host, ts));
        assert!(!origin_allowed(Some("http://evil.example"), host, ts));
        assert!(!origin_allowed(Some("https://other.tail1.ts.net:7878"), host, ts));
        assert!(!origin_allowed(Some("garbage"), host, ts));
    }
}
