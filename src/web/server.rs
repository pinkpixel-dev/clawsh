//! The phone server: one listener thread, plus a thread per connection.
//! Plain GETs get the client's files, and `/ws?t=<token>` upgrades to the
//! WebSocket that carries frames out and input back in.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender, TryRecvError};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use tungstenite::protocol::Role;
use tungstenite::{Message, WebSocket};

use super::{WebEvent, assets};
use crate::event::AppEvent;

/// Where the server listens unless `CLAWSH_WEB_ADDR` says otherwise.
pub const DEFAULT_ADDR: &str = "127.0.0.1:7878";
/// Longest request head we'll read before giving up.
const MAX_HEAD: usize = 16 * 1024;
/// How long a socket read waits before checking for frames to send.
const POLL: Duration = Duration::from_millis(15);

pub struct WebServer {
    addr: SocketAddr,
    token: String,
    stop: Arc<AtomicBool>,
}

impl WebServer {
    pub fn start(addr: &str, tx: Sender<AppEvent>) -> Result<Self> {
        let listener = TcpListener::bind(addr).with_context(|| format!("couldn't listen on {addr}"))?;
        let addr = listener.local_addr()?;
        let token = random_token()?;
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Shared { token: token.clone(), stop: stop.clone(), tx, next_id: AtomicU64::new(1) });
        thread::Builder::new().name("clawsh-web".into()).spawn(move || {
            for stream in listener.incoming() {
                if shared.stop.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                let shared = shared.clone();
                let _ = thread::Builder::new()
                    .name("clawsh-web-conn".into())
                    .spawn(move || {
                        let _ = serve(stream, &shared);
                    });
            }
        })?;
        Ok(Self { addr, token, stop })
    }

    /// The address to open on the phone, token included.
    pub fn url(&self) -> String {
        format!("http://{}/?t={}", self.addr, self.token)
    }
}

impl Drop for WebServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wake the listener so it sees the flag. Open sockets notice on
        // their next poll.
        let _ = TcpStream::connect(self.addr);
    }
}

struct Shared {
    token: String,
    stop: Arc<AtomicBool>,
    tx: Sender<AppEvent>,
    next_id: AtomicU64,
}

/// What we need from a request head.
struct Request {
    path: String,
    query: String,
    websocket_key: Option<String>,
    origin: Option<String>,
    host: Option<String>,
}

fn serve(mut stream: TcpStream, shared: &Shared) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let (head, rest) = read_head(&mut stream)?;
    let Some(req) = parse(&head) else {
        return respond(&mut stream, "400 Bad Request", "text/plain", b"bad request");
    };
    if req.path == "/ws" {
        let token_ok = query_param(&req.query, "t").is_some_and(|t| same(t, &shared.token));
        // Other programs on this computer can reach localhost too, so a page
        // from somewhere else must not be able to open the socket.
        let origin_ok = match (&req.origin, &req.host) {
            (Some(origin), Some(host)) => origin.split_once("://").is_some_and(|(_, h)| h == host),
            (None, _) => true,
            _ => false,
        };
        return match (req.websocket_key, token_ok && origin_ok) {
            (Some(key), true) => run_socket(stream, &key, rest, shared),
            (None, _) => respond(&mut stream, "400 Bad Request", "text/plain", b"websocket only"),
            (_, false) => respond(&mut stream, "403 Forbidden", "text/plain", b"forbidden"),
        };
    }
    let name = match req.path.trim_start_matches('/') {
        "" => "index.html",
        other => other,
    };
    match assets::get(name) {
        Some((body, kind)) => respond(&mut stream, "200 OK", kind, &body),
        None if name == "index.html" => respond(&mut stream, "503 Service Unavailable", "text/plain", assets::NOT_BUILT.as_bytes()),
        None => respond(&mut stream, "404 Not Found", "text/plain", b"not found"),
    }
}

/// Reads up to the blank line after the headers. Whatever came in after it
/// belongs to the WebSocket.
fn read_head(stream: &mut TcpStream) -> Result<(Vec<u8>, Vec<u8>)> {
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

fn parse(head: &[u8]) -> Option<Request> {
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
    })
}

fn query_param<'q>(query: &'q str, name: &str) -> Option<&'q str> {
    query.split('&').find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
}

/// Compares without stopping at the first difference.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-cache\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)?;
    Ok(())
}

fn run_socket(mut stream: TcpStream, key: &str, rest: Vec<u8>, shared: &Shared) -> Result<()> {
    let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
    write!(
        stream,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    )?;
    stream.set_read_timeout(Some(POLL))?;
    stream.set_nodelay(true)?;
    let mut ws = WebSocket::from_partially_read(stream, rest, Role::Server, None);

    let id = shared.next_id.fetch_add(1, Ordering::Relaxed);
    let (out_tx, out_rx) = mpsc::channel::<String>();
    shared.tx.send(AppEvent::Web(WebEvent::Connected(id, out_tx)))?;
    let result = (|| -> Result<()> {
        while !shared.stop.load(Ordering::Relaxed) {
            loop {
                match out_rx.try_recv() {
                    Ok(text) => ws.send(Message::text(text))?,
                    Err(TryRecvError::Empty) => break,
                    // Claw$h let go of this phone, usually for a newer connection.
                    Err(TryRecvError::Disconnected) => return Ok(()),
                }
            }
            match ws.read() {
                Ok(Message::Text(text)) => match serde_json::from_str(text.as_str()) {
                    Ok(msg) => shared.tx.send(AppEvent::Web(WebEvent::Message(id, msg)))?,
                    Err(_) => continue,
                },
                Ok(Message::Close(_)) => return Ok(()),
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    })();
    let _ = ws.close(None);
    let _ = shared.tx.send(AppEvent::Web(WebEvent::Gone(id)));
    result
}

/// 128 random bits as hex, straight from the kernel.
fn random_token() -> Result<String> {
    let mut bytes = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_upgrade_request() {
        let head = b"GET /ws?t=abc&x=1 HTTP/1.1\r\nHost: 127.0.0.1:7878\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nOrigin: http://127.0.0.1:7878\r\n\r\n";
        let req = parse(head).unwrap();
        assert_eq!(req.path, "/ws");
        assert_eq!(query_param(&req.query, "t"), Some("abc"));
        assert_eq!(query_param(&req.query, "x"), Some("1"));
        assert_eq!(query_param(&req.query, "tt"), None);
        assert_eq!(req.websocket_key.as_deref(), Some("dGhlIHNhbXBsZSBub25jZQ=="));
        assert_eq!(req.host.as_deref(), Some("127.0.0.1:7878"));
    }

    #[test]
    fn plain_get_has_no_websocket_key_and_post_is_refused() {
        let req = parse(b"GET / HTTP/1.1\r\nHost: x\r\nSec-WebSocket-Key: k\r\n\r\n").unwrap();
        assert!(req.websocket_key.is_none());
        assert!(parse(b"POST / HTTP/1.1\r\nHost: x\r\n\r\n").is_none());
    }

    #[test]
    fn token_compare() {
        assert!(same("abcd", "abcd"));
        assert!(!same("abcd", "abce"));
        assert!(!same("abc", "abcd"));
    }

    #[test]
    fn tokens_are_32_hex_chars_and_differ() {
        let (a, b) = (random_token().unwrap(), random_token().unwrap());
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
