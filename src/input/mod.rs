//! Reads the host terminal's input on its own thread.
//!
//! Claw$h doesn't use crossterm's event reader: it would turn terminal
//! replies (theme change notices, OSC color replies) into keystrokes or hold
//! them in its buffer, and those would end up in Claude's prompt. Instead this
//! reads stdin itself and runs it through a copy of crossterm's parser
//! (`parse.rs`) that knows about those replies. Keys, mouse, paste and focus
//! still come out as ordinary crossterm events.

mod parse;

use std::sync::mpsc::Sender;
use std::thread;

use crossterm::event::{Event, KeyboardEnhancementFlags};
use libghostty_vt::style::RgbColor;
use libghostty_vt::terminal::ColorScheme;

use crate::event::AppEvent;

/// Same read size crossterm uses; a full read means more bytes are waiting.
const READ_SIZE: usize = 1024;
/// How long the reader waits for input before checking whether the terminal
/// was resized (we don't get crossterm's SIGWINCH handling).
const RESIZE_CHECK_MS: i32 = 200;

/// What the parser can produce: crossterm's internal events plus the
/// replies Claw$h cares about.
#[derive(Debug, PartialEq, Eq, Clone)]
pub(crate) enum InternalEvent {
    Event(Event),
    CursorPosition(u16, u16),
    KeyboardEnhancementFlags(KeyboardEnhancementFlags),
    PrimaryDeviceAttributes,
    /// The terminal's theme changed (mode 2031).
    ColorScheme(ColorScheme),
    /// A reply to OSC 10 (foreground) or OSC 11 (background).
    HostColor(u8, RgbColor),
    /// A complete reply nobody needs.
    Ignored,
}

/// Feeds bytes through the parser one at a time, like crossterm's own
/// `Parser::advance`.
#[derive(Default)]
struct Parser {
    buffer: Vec<u8>,
}

impl Parser {
    fn advance(&mut self, bytes: &[u8], more: bool, out: &mut Vec<InternalEvent>) {
        for (i, &byte) in bytes.iter().enumerate() {
            let more = i + 1 < bytes.len() || more;
            self.buffer.push(byte);
            match parse::parse_event(&self.buffer, more) {
                Ok(Some(event)) => {
                    out.push(event);
                    self.buffer.clear();
                }
                // Not enough bytes yet for the current sequence.
                Ok(None) => {}
                // Couldn't parse it; drop it and carry on.
                Err(_) => self.buffer.clear(),
            }
        }
    }
}

/// Starts the reader thread. It runs until stdin closes or the app is gone.
pub fn spawn_reader(tx: Sender<AppEvent>) {
    thread::spawn(move || {
        let mut parser = Parser::default();
        let mut events = Vec::new();
        let mut buf = [0u8; READ_SIZE];
        let mut size = crossterm::terminal::size().ok();
        loop {
            if !stdin_ready(RESIZE_CHECK_MS) {
                let now = crossterm::terminal::size().ok();
                if now != size {
                    size = now;
                    if let Some((cols, rows)) = now
                        && tx.send(AppEvent::Input(Event::Resize(cols, rows))).is_err()
                    {
                        return;
                    }
                }
                continue;
            }
            // SAFETY: reading into a local buffer of the length we pass.
            let n = unsafe { libc::read(libc::STDIN_FILENO, buf.as_mut_ptr().cast(), buf.len()) };
            if n <= 0 {
                if n < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return;
            }
            let n = n as usize;
            parser.advance(&buf[..n], n == READ_SIZE, &mut events);
            for event in events.drain(..) {
                let app_event = match event {
                    InternalEvent::Event(e) => AppEvent::Input(e),
                    InternalEvent::ColorScheme(scheme) => AppEvent::HostScheme(scheme),
                    InternalEvent::HostColor(code, rgb) => AppEvent::HostColor(code, rgb),
                    _ => continue,
                };
                if tx.send(app_event).is_err() {
                    return;
                }
            }
        }
    });
}

fn stdin_ready(timeout_ms: i32) -> bool {
    let mut fd = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one valid pollfd for the duration of the call.
    unsafe { libc::poll(&mut fd, 1, timeout_ms) > 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn feed(chunks: &[&[u8]]) -> Vec<InternalEvent> {
        let mut parser = Parser::default();
        let mut out = Vec::new();
        for chunk in chunks {
            parser.advance(chunk, false, &mut out);
        }
        out
    }

    fn key(c: char) -> InternalEvent {
        InternalEvent::Event(Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)))
    }

    #[test]
    fn theme_notices_never_become_keys() {
        // A theme change arriving between two keystrokes.
        let out = feed(&[b"a", b"\x1b[?997;2n", b"b"]);
        assert_eq!(out, [key('a'), InternalEvent::ColorScheme(ColorScheme::Light), key('b')]);
    }

    #[test]
    fn color_replies_are_caught() {
        let out = feed(&[b"\x1b]11;rgb:fafa/f8f8/f5f5\x1b\\x"]);
        assert_eq!(
            out,
            [
                InternalEvent::HostColor(11, RgbColor { r: 0xfa, g: 0xf8, b: 0xf5 }),
                key('x'),
            ]
        );
        // BEL-terminated, split across reads.
        let out = feed(&[b"\x1b]10;rgb:00/00", b"/00\x07"]);
        assert_eq!(out, [InternalEvent::HostColor(10, RgbColor::default())]);
    }

    #[test]
    fn other_replies_are_dropped_without_eating_keys() {
        // A kitty graphics reply, an unknown OSC, and a mode report.
        let out = feed(&[b"\x1b_Gi=1;OK\x1b\\", b"\x1b]2;title\x07", b"\x1b[?2031;2$y", b"q"]);
        assert_eq!(out, [InternalEvent::Ignored, InternalEvent::Ignored, InternalEvent::Ignored, key('q')]);
    }

    #[test]
    fn a_lone_alt_bracket_is_still_a_key() {
        let out = feed(&[b"\x1b]"]);
        assert_eq!(
            out,
            [InternalEvent::Event(Event::Key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::ALT)))]
        );
    }
}
