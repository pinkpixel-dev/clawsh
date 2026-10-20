//! Phone access from the main loop's side: starting and stopping the server,
//! the phone's input, and drawing a second frame at the phone's size.
//!
//! Panes can only have one size, so whoever typed last owns it, like tmux's
//! `window-size latest`. The other screen sees the panes padded or cropped.

use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use crossterm::event::Event;
use ratatui::layout::Rect;

use super::{App, Mode};
use super::render::{self, Screen};
use crate::host_colors::HostColors;
use crate::pane::Pane;
use crate::ui::{self, Hits, View, WebView};
use crate::web::protocol::FromPhone;
use crate::web::{self, Capture, Serve, WebEvent, WebServer};
use crate::workspace::Workspaces;

/// Phone access stops on its own after this long with no phone connected.
const IDLE_STOP: Duration = Duration::from_secs(15 * 60);

pub(super) struct Web {
    /// Dropping it stops the server.
    _server: WebServer,
    /// `tailscale serve` in front of the server. Dropping it stops that too.
    _serve: Option<Serve>,
    url: String,
    /// Why there's no `serve`, shown in the popup.
    local_only: Option<String>,
    phone: Option<Phone>,
    /// Since when no phone has been connected; `None` while one is.
    idle_since: Option<Instant>,
}

/// The connected phone. Only one at a time; a new connection replaces it.
struct Phone {
    id: u64,
    tx: Sender<String>,
    /// `None` until the phone says how big it is.
    capture: Option<Capture>,
    /// Click targets from the phone's last frame.
    hits: Hits,
}

impl App {
    /// `ctrl+b W`: start the server, or stop it if it's running.
    pub(super) fn toggle_web(&mut self) -> Result<()> {
        if self.web.is_some() {
            return self.stop_web("Phone access off");
        }
        let addr = std::env::var("CLAWSH_WEB_ADDR").unwrap_or_else(|_| web::DEFAULT_ADDR.into());
        let tailnet = if std::env::var("CLAWSH_TAILSCALE").as_deref() == Ok("off") {
            Err("CLAWSH_TAILSCALE is off".to_owned())
        } else {
            web::tailnet_name()
        };
        let server = match WebServer::start(&addr, self.tx.clone(), tailnet.as_deref().ok()) {
            Ok(server) => server,
            Err(err) => {
                self.notice = Some(format!("Phone access failed: {err:#}"));
                return Ok(());
            }
        };
        let (serve, local_only) = match tailnet.and_then(|_| Serve::start(server.port())) {
            Ok(serve) => (Some(serve), None),
            Err(why) => (None, Some(why)),
        };
        let url = server.url(serve.is_some());
        // The QR code is the easy way over, but the clipboard helps when the
        // phone isn't the device that needs the link.
        super::write_host(format!("\x1b]52;c;{}\x1b\\", STANDARD.encode(&url)).as_bytes())?;
        self.web = Some(Web {
            _server: server,
            _serve: serve,
            url,
            local_only,
            phone: None,
            idle_since: Some(Instant::now()),
        });
        self.mode = Mode::WebLink;
        Ok(())
    }

    fn stop_web(&mut self, why: &str) -> Result<()> {
        self.web = None;
        if matches!(self.mode, Mode::WebLink) {
            self.mode = Mode::Normal;
        }
        self.phone_owns_size = false;
        self.layout_panes()?;
        self.notice = Some(why.into());
        Ok(())
    }

    /// Stops phone access once nobody has used it for a while.
    pub(super) fn web_tick(&mut self) -> Result<()> {
        let idle = self.web.as_ref().and_then(|w| w.idle_since).is_some_and(|t| t.elapsed() >= IDLE_STOP);
        if idle {
            self.stop_web("Phone access stopped after 15 minutes with no phone")?;
        }
        Ok(())
    }

    /// What the footer badge and the popup show.
    pub(super) fn web_view(&self) -> Option<WebView> {
        let web = self.web.as_ref()?;
        Some(WebView { url: web.url.clone(), note: web.local_only.clone(), connected: web.phone.is_some() })
    }

    pub(super) fn web_running(&self) -> bool {
        self.web.is_some()
    }

    pub(super) fn web_event(&mut self, ev: WebEvent) -> Result<()> {
        let Some(web) = &mut self.web else {
            return Ok(());
        };
        match ev {
            WebEvent::Connected(id, tx) => {
                // Dropping the old sender tells its socket thread to close.
                web.phone = Some(Phone { id, tx, capture: None, hits: Hits::default() });
                web.idle_since = None;
                if self.phone_owns_size {
                    self.phone_owns_size = false;
                    self.layout_panes()?;
                }
            }
            WebEvent::Gone(id) => {
                if web.phone.as_ref().is_some_and(|p| p.id == id) {
                    web.phone = None;
                    web.idle_since = Some(Instant::now());
                    if self.phone_owns_size {
                        self.phone_owns_size = false;
                        self.layout_panes()?;
                    }
                }
            }
            WebEvent::Message(id, msg) => {
                let Some(phone) = web.phone.as_mut().filter(|p| p.id == id) else {
                    return Ok(());
                };
                if let FromPhone::Size { cols, rows } = msg {
                    phone.capture = Some(Capture::new(cols, rows));
                    if self.phone_owns_size {
                        self.layout_panes()?;
                    }
                    return Ok(());
                }
                // Input before the phone's first frame has nothing to aim at.
                if phone.capture.is_none() {
                    return Ok(());
                }
                let Some(ev) = web::to_event(msg) else {
                    return Ok(());
                };
                self.own_size(true)?;
                self.phone_input(ev)?;
            }
        }
        Ok(())
    }

    /// Handles phone input against the phone's own click targets.
    fn phone_input(&mut self, ev: Event) -> Result<()> {
        let Some(phone) = self.web.as_mut().and_then(|w| w.phone.as_mut()) else {
            return Ok(());
        };
        let phone_hits = std::mem::take(&mut phone.hits);
        let desktop_hits = std::mem::replace(&mut self.hits, phone_hits);
        let result = self.input(ev);
        let phone_hits = std::mem::replace(&mut self.hits, desktop_hits);
        if let Some(phone) = self.web.as_mut().and_then(|w| w.phone.as_mut()) {
            phone.hits = phone_hits;
        }
        result
    }

    /// Gives the pane size to the phone or back to the desktop.
    pub(super) fn own_size(&mut self, phone: bool) -> Result<()> {
        if self.phone_owns_size != phone {
            self.phone_owns_size = phone;
            self.layout_panes()?;
        }
        Ok(())
    }

    /// The phone's screen, once it has said how big it is.
    pub(super) fn phone_screen(&self) -> Option<Rect> {
        self.web.as_ref()?.phone.as_ref()?.capture.as_ref().map(Capture::size)
    }
}

/// What the phone's frame needs from `App::draw`.
pub(super) struct PhoneDraw<'a> {
    pub spaces: &'a mut Workspaces<Pane>,
    pub view: &'a mut View<'a>,
    pub show_sidebar: bool,
    pub side: ui::drawer::Side,
    pub percent: u16,
    pub reviewing: bool,
    pub normal: bool,
    pub colors: HostColors,
}

/// Draws the phone's frame and sends whatever changed. Images are left out;
/// the phone only gets text.
pub(super) fn draw_phone(web: Option<&mut Web>, d: PhoneDraw) -> Result<()> {
    let Some(phone) = web.and_then(|w| w.phone.as_mut()) else {
        return Ok(());
    };
    let Some(capture) = phone.capture.as_mut() else {
        return Ok(());
    };
    let areas = ui::layout(capture.size(), d.show_sidebar);
    let screen = Screen { areas: &areas, side: d.side, percent: d.percent, reviewing: d.reviewing, normal: d.normal };
    let mut drawn = None;
    let completed = capture.terminal().draw(|frame| {
        drawn = Some(render::render(frame, d.spaces, d.view, &screen, &|_, _| None));
    })?;
    let buffer = completed.buffer.clone();
    let drawn = drawn.expect("draw runs the closure")?;
    phone.hits = drawn.hits;
    if let Some(msg) = capture.frame(&buffer, drawn.cursor, d.colors) {
        // A send only fails while the socket thread is closing; `Gone` follows.
        let _ = phone.tx.send(serde_json::to_string(&msg)?);
    }
    Ok(())
}
