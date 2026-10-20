//! Phone access from the main loop's side: starting and stopping the server,
//! the phone's input, and drawing a second frame at the phone's size.
//!
//! Panes can only have one size, so whoever typed last owns it, like tmux's
//! `window-size latest`. The other screen sees the panes padded or cropped.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
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
use crate::web::{self, Capture, Pairing, Serve, WebEvent, WebServer};
use crate::workspace::Workspaces;

/// Phone access stops on its own after this long with no phone connected.
const IDLE_STOP: Duration = Duration::from_secs(15 * 60);

pub(super) struct Web {
    server: WebServer,
    /// `tailscale serve` in front of the server. Dropping it stops that.
    _serve: Serve,
    /// The code on screen, shared with the server, which clears it once a
    /// phone uses it.
    pairing: Arc<Mutex<Option<Pairing>>>,
    phone: Option<Phone>,
    /// Since when no phone has been connected; `None` while one is.
    idle_since: Option<Instant>,
}

/// The connected phone. Only one at a time; a new connection replaces it.
struct Phone {
    id: u64,
    tx: Sender<String>,
    /// Which paired device this is, for removing it.
    device: String,
    /// Sees the screen, but its input is dropped.
    watch_only: bool,
    /// `None` until the phone says how big it is.
    capture: Option<Capture>,
    /// Click targets from the phone's last frame.
    hits: Hits,
}

impl App {
    /// `ctrl+b W`: start the server, or stop it if it's running. Phone access
    /// only works through Tailscale, so it won't start without it.
    pub(super) fn toggle_web(&mut self) -> Result<()> {
        if self.web.is_some() {
            return self.stop_web("Phone access off");
        }
        let tailnet = match web::tailnet_name() {
            Ok(name) => name,
            Err(why) => {
                self.notice = Some(format!("Phone access needs Tailscale: {why}"));
                return Ok(());
            }
        };
        let port = std::env::var("CLAWSH_WEB_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(web::DEFAULT_PORT);
        let pairing = Arc::new(Mutex::new(None));
        let server = match WebServer::start(port, &tailnet, self.tx.clone(), self.devices.clone(), pairing.clone()) {
            Ok(server) => server,
            Err(err) => {
                self.notice = Some(format!("Phone access failed: {err:#}"));
                return Ok(());
            }
        };
        let serve = match Serve::start(server.port()) {
            Ok(serve) => serve,
            Err(why) => {
                self.notice = Some(format!("Phone access needs Tailscale: {why}"));
                return Ok(());
            }
        };
        // Paired phones only need this, so it's handy for a bookmark.
        super::write_host(format!("\x1b]52;c;{}\x1b\\", STANDARD.encode(server.home())).as_bytes())?;
        self.web = Some(Web { server, _serve: serve, pairing, phone: None, idle_since: Some(Instant::now()) });
        self.show_web_link(false)
    }

    /// Opens the popup with a fresh pairing code.
    pub(super) fn show_web_link(&mut self, watch_only: bool) -> Result<()> {
        let Some(web) = &self.web else {
            return Ok(());
        };
        let fresh = Pairing::new(watch_only)?;
        if let Ok(mut pairing) = web.pairing.lock() {
            *pairing = Some(fresh);
        }
        self.mode = Mode::WebLink;
        Ok(())
    }

    /// Keys in the popup: `r` switches watch-only, `n` makes a new code, and
    /// anything else closes it.
    pub(super) fn web_link_key(&mut self, c: Option<char>) -> Result<()> {
        let watch_only = self.web_view().is_some_and(|v| v.watch_only);
        match c {
            Some('r') => self.show_web_link(!watch_only),
            Some('n') => self.show_web_link(watch_only),
            _ => {
                self.mode = Mode::Normal;
                Ok(())
            }
        }
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
        let pairing = web.pairing.lock().ok();
        let live = pairing.as_ref().and_then(|p| p.as_ref()).filter(|p| !p.expired());
        Some(WebView {
            link: live.map(|p| web.server.pair_link(p.code())),
            code: live.map(|p| p.display()),
            minutes_left: live.map_or(0, |p| p.remaining().as_secs().div_ceil(60)),
            watch_only: pairing.as_ref().and_then(|p| p.as_ref()).is_some_and(|p| p.watch_only),
            home: web.server.home(),
            connected: web.phone.is_some(),
        })
    }

    pub(super) fn web_running(&self) -> bool {
        self.web.is_some()
    }

    /// Drops the connected phone if it's this device, after it was removed.
    pub(super) fn disconnect_device(&mut self, device: &str) -> Result<()> {
        let Some(web) = &mut self.web else {
            return Ok(());
        };
        if web.phone.as_ref().is_some_and(|p| p.device == device) {
            web.phone = None;
            web.idle_since = Some(Instant::now());
            self.own_size(false)?;
        }
        Ok(())
    }

    pub(super) fn web_event(&mut self, ev: WebEvent) -> Result<()> {
        let Some(web) = &mut self.web else {
            return Ok(());
        };
        match ev {
            WebEvent::Connected { id, tx, device, watch_only } => {
                // Dropping the old sender tells its socket thread to close.
                web.phone = Some(Phone { id, tx, device, watch_only, capture: None, hits: Hits::default() });
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
            WebEvent::Paired(name) => {
                if matches!(self.mode, Mode::WebLink) {
                    self.mode = Mode::Normal;
                }
                self.notice = Some(format!("Paired {name}"));
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
                // Input before the phone's first frame has nothing to aim
                // at, and a watch-only phone doesn't get to send any.
                if phone.capture.is_none() || phone.watch_only {
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
