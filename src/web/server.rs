//! The phone server: one listener thread, plus a thread per connection, on
//! localhost only. `tailscale serve` is the way in from the phone.
//!
//! Routes: `/pair?c=<code>` trades the desk's pairing code for a device
//! cookie, `/api/me` says whether this browser is paired, `/ws` is the
//! WebSocket (paired devices only), and everything else is the client's files.

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use tungstenite::protocol::Role;
use tungstenite::{Message, WebSocket};

use super::devices::{Device, Devices, Pairing, name_from_agent};
use super::http::{self, Request};
use super::{WebEvent, assets};
use crate::event::AppEvent;

/// The port unless `CLAWSH_WEB_PORT` says otherwise.
pub const DEFAULT_PORT: u16 = 7878;
/// How long a socket read waits before checking for frames to send.
const POLL: Duration = Duration::from_millis(15);
const COOKIE: &str = "clawsh_device";
/// 400 days, the longest browsers keep a cookie.
const COOKIE_AGE: u32 = 400 * 24 * 60 * 60;

pub struct WebServer {
    addr: SocketAddr,
    /// `<name>.ts.net:<port>`, where `tailscale serve` answers.
    tailnet: String,
    stop: Arc<AtomicBool>,
}

impl WebServer {
    /// Listens on `127.0.0.1:<port>`. `tailnet` is this machine's `*.ts.net`
    /// name; pages from there count as our own origin.
    pub fn start(
        port: u16,
        tailnet: &str,
        tx: Sender<AppEvent>,
        devices: Arc<Mutex<Devices>>,
        pairing: Arc<Mutex<Option<Pairing>>>,
    ) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port)).with_context(|| format!("couldn't listen on port {port}"))?;
        let addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let tailnet = format!("{tailnet}:{}", addr.port());
        let shared = Arc::new(Shared {
            stop: stop.clone(),
            tx,
            next_id: AtomicU64::new(1),
            tailnet: tailnet.clone(),
            devices,
            pairing,
        });
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
        Ok(Self { addr, tailnet, stop })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    /// Where a paired phone goes, worth a bookmark.
    pub fn home(&self) -> String {
        format!("https://{}/", self.tailnet)
    }

    /// The link in the QR code.
    pub fn pair_link(&self, code: &str) -> String {
        format!("https://{}/pair?c={code}", self.tailnet)
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
    stop: Arc<AtomicBool>,
    tx: Sender<AppEvent>,
    next_id: AtomicU64,
    tailnet: String,
    devices: Arc<Mutex<Devices>>,
    /// The code on screen at the desk, if any.
    pairing: Arc<Mutex<Option<Pairing>>>,
}

impl Shared {
    fn device(&self, req: &Request) -> Option<Device> {
        let token = http::cookie(req.cookie.as_deref(), COOKIE)?;
        self.devices.lock().ok()?.by_token(token).cloned()
    }
}

fn serve(mut stream: TcpStream, shared: &Shared) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let (head, rest) = http::read_head(&mut stream)?;
    let Some(req) = http::parse(&head) else {
        return http::respond(&mut stream, "400 Bad Request", "text/plain", b"bad request", &[]);
    };
    match req.path.as_str() {
        "/ws" => {
            // Other programs on this computer can reach localhost too.
            if !http::origin_allowed(req.origin.as_deref(), req.host.as_deref(), &shared.tailnet) {
                return http::respond(&mut stream, "403 Forbidden", "text/plain", b"forbidden", &[]);
            }
            match (req.websocket_key.clone(), shared.device(&req)) {
                (None, _) => http::respond(&mut stream, "400 Bad Request", "text/plain", b"websocket only", &[]),
                (_, None) => http::respond(&mut stream, "401 Unauthorized", "text/plain", b"not paired", &[]),
                (Some(key), Some(device)) => run_socket(stream, &key, rest, shared, device),
            }
        }
        "/api/me" => match shared.device(&req) {
            Some(d) => {
                let body = serde_json::json!({ "name": d.name, "watch_only": d.watch_only }).to_string();
                http::respond(&mut stream, "200 OK", "application/json", body.as_bytes(), &[])
            }
            None => http::respond(&mut stream, "401 Unauthorized", "application/json", b"{}", &[]),
        },
        "/pair" => pair(&mut stream, &req, shared),
        path => {
            let name = match path.trim_start_matches('/') {
                "" => "index.html",
                other => other,
            };
            match assets::get(name) {
                Some((body, kind)) => http::respond(&mut stream, "200 OK", kind, &body, &[]),
                None if name == "index.html" => {
                    http::respond(&mut stream, "503 Service Unavailable", "text/plain", assets::NOT_BUILT.as_bytes(), &[])
                }
                None => http::respond(&mut stream, "404 Not Found", "text/plain", b"not found", &[]),
            }
        }
    }
}

/// Trades the code from the QR link or the pairing form for a device cookie.
fn pair(stream: &mut TcpStream, req: &Request, shared: &Shared) -> Result<()> {
    let code = http::query_param(&req.query, "c").unwrap_or("");
    let watch_only = {
        let mut pairing = shared.pairing.lock().map_err(|_| anyhow::anyhow!("pairing lock poisoned"))?;
        match pairing.as_mut().map(|p| (p.check(code), p.watch_only)) {
            Some((true, watch_only)) => {
                // One code, one device.
                *pairing = None;
                Some(watch_only)
            }
            _ => None,
        }
    };
    let Some(watch_only) = watch_only else {
        return http::redirect(stream, "/?pair=failed", &[]);
    };
    let name = name_from_agent(req.user_agent.as_deref().unwrap_or(""));
    let (device, token) = shared
        .devices
        .lock()
        .map_err(|_| anyhow::anyhow!("devices lock poisoned"))?
        .add(name, watch_only)?;
    let _ = shared.tx.send(AppEvent::Web(WebEvent::Paired(device.name.clone())));
    let cookie = format!("{COOKIE}={token}; Path=/; Max-Age={COOKIE_AGE}; HttpOnly; Secure; SameSite=Strict");
    http::redirect(stream, "/", &[("Set-Cookie", &cookie)])
}

fn run_socket(mut stream: TcpStream, key: &str, rest: Vec<u8>, shared: &Shared, device: Device) -> Result<()> {
    use std::io::Write;
    let accept = tungstenite::handshake::derive_accept_key(key.as_bytes());
    write!(
        stream,
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    )?;
    stream.set_read_timeout(Some(POLL))?;
    stream.set_nodelay(true)?;
    let mut ws = WebSocket::from_partially_read(stream, rest, Role::Server, None);

    if let Ok(mut devices) = shared.devices.lock() {
        devices.touch(&device.id);
    }
    let id = shared.next_id.fetch_add(1, Ordering::Relaxed);
    let (out_tx, out_rx) = mpsc::channel::<String>();
    shared.tx.send(AppEvent::Web(WebEvent::Connected {
        id,
        tx: out_tx,
        device: device.id,
        watch_only: device.watch_only,
    }))?;
    let result = (|| -> Result<()> {
        while !shared.stop.load(Ordering::Relaxed) {
            loop {
                match out_rx.try_recv() {
                    Ok(text) => ws.send(Message::text(text))?,
                    Err(TryRecvError::Empty) => break,
                    // Claw$h let go of this phone: a newer connection, or
                    // the device was removed.
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
