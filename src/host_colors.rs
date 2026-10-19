//! The host terminal's own foreground and background colors, asked for once
//! at startup so programs in the panes (Claude picking its light or dark
//! theme) see the real ones instead of a made-up default.

use std::io::{self, Write};
use std::time::{Duration, Instant};

use libghostty_vt::style::RgbColor;
use libghostty_vt::terminal::ColorScheme;

/// How long to wait for the terminal to answer. Terminals that don't know
/// OSC 10/11 still answer the DA1 query we send after them, which ends the
/// wait early.
const TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostColors {
    pub fg: Option<RgbColor>,
    pub bg: Option<RgbColor>,
}

impl HostColors {
    /// Light or dark, judged by how bright the background looks. `None`
    /// when the terminal didn't say.
    pub fn scheme(&self) -> Option<ColorScheme> {
        self.bg.map(|bg| {
            if bg.perceived_luminance() > 0.5 {
                ColorScheme::Light
            } else {
                ColorScheme::Dark
            }
        })
    }
}

/// Asks the terminal for its colors. Call it in raw mode, before anything
/// else reads stdin, so the replies can't end up as keypresses.
pub fn query() -> HostColors {
    let mut out = io::stdout();
    // OSC 10 (fg), OSC 11 (bg), then DA1, which every terminal answers.
    if out.write_all(b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b[c").and_then(|_| out.flush()).is_err() {
        return HostColors::default();
    }

    let mut reply = Vec::new();
    let deadline = Instant::now() + TIMEOUT;
    while !has_da1(&reply) {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || !stdin_ready(left) {
            break;
        }
        let mut buf = [0u8; 256];
        // SAFETY: reading into a local buffer of the length we pass.
        let n = unsafe { libc::read(libc::STDIN_FILENO, buf.as_mut_ptr().cast(), buf.len()) };
        if n <= 0 {
            break;
        }
        reply.extend_from_slice(&buf[..n as usize]);
    }

    HostColors {
        fg: parse_osc_color(&reply, 10),
        bg: parse_osc_color(&reply, 11),
    }
}

fn stdin_ready(timeout: Duration) -> bool {
    let mut fd = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = timeout.as_millis().min(i32::MAX as u128) as libc::c_int;
    // SAFETY: one valid pollfd for the duration of the call.
    unsafe { libc::poll(&mut fd, 1, ms) > 0 }
}

/// `ESC [ ? ... c`, the DA1 reply that marks the end of our answers.
fn has_da1(buf: &[u8]) -> bool {
    (0..buf.len()).any(|i| {
        let Some(rest) = buf[i..].strip_prefix(b"\x1b[?") else {
            return false;
        };
        let params = rest.iter().take_while(|b| b.is_ascii_digit() || **b == b';').count();
        rest.get(params) == Some(&b'c')
    })
}

/// Finds `ESC ] <code> ; rgb:RRRR/GGGG/BBBB` (ended by BEL or ST) and scales
/// each channel to 8 bits. Channels can be 1 to 4 hex digits.
fn parse_osc_color(buf: &[u8], code: u8) -> Option<RgbColor> {
    let text = String::from_utf8_lossy(buf);
    let start = text.find(&format!("\x1b]{code};rgb:"))? + format!("\x1b]{code};rgb:").len();
    let rest = &text[start..];
    let end = rest.find(['\x07', '\x1b'])?;
    let mut parts = rest[..end].split('/').map(channel);
    let (r, g, b) = (parts.next()??, parts.next()??, parts.next()??);
    Some(RgbColor { r, g, b })
}

fn channel(hex: &str) -> Option<u8> {
    if hex.is_empty() || hex.len() > 4 {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    let max = (1u32 << (4 * hex.len())) - 1;
    Some((value * 255 / max) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_osc_color_replies() {
        let reply = b"\x1b]10;rgb:ecec/ecec/efef\x1b\\\x1b]11;rgb:1c/1d/21\x07\x1b[?62;22c";
        assert_eq!(parse_osc_color(reply, 10), Some(RgbColor { r: 0xec, g: 0xec, b: 0xef }));
        assert_eq!(parse_osc_color(reply, 11), Some(RgbColor { r: 0x1c, g: 0x1d, b: 0x21 }));
        assert_eq!(parse_osc_color(b"\x1b[?62c", 11), None);
        assert_eq!(parse_osc_color(b"\x1b]11;rgb:zz/00/00\x07", 11), None);
    }

    #[test]
    fn spots_the_end_of_the_replies() {
        assert!(has_da1(b"\x1b]11;rgb:0/0/0\x07\x1b[?62;22c"));
        assert!(has_da1(b"\x1b[?6c"));
        assert!(!has_da1(b"\x1b]11;rgb:0/0/0\x07\x1b[?62;2"));
    }

    #[test]
    fn judges_light_and_dark() {
        let with_bg = |r, g, b| HostColors { fg: None, bg: Some(RgbColor { r, g, b }) };
        assert_eq!(with_bg(0x1c, 0x1d, 0x21).scheme(), Some(ColorScheme::Dark));
        assert_eq!(with_bg(0xfa, 0xf8, 0xf5).scheme(), Some(ColorScheme::Light));
        assert_eq!(HostColors::default().scheme(), None);
    }
}
