//! Messages between Claw$h and the phone client, sent as JSON over the
//! WebSocket. The `web/` client compiles this same file through a `#[path]`
//! module, so it can only use serde and ratatui.

use ratatui::style::{Color, Modifier};
use serde::{Deserialize, Serialize};

/// Claw$h to the phone.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ToPhone {
    /// Cells that changed since the last frame. `full` means the phone should
    /// start from a blank screen of this size first.
    Frame {
        cols: u16,
        rows: u16,
        full: bool,
        cells: Vec<CellDiff>,
        cursor: Option<(u16, u16)>,
    },
}

/// One cell: its position, text, colors (see [`color_code`]), and modifier bits.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CellDiff {
    pub x: u16,
    pub y: u16,
    pub s: String,
    pub fg: i32,
    pub bg: i32,
    pub m: u16,
}

/// The phone to Claw$h.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum FromPhone {
    /// The phone's grid, sent on connect and whenever it changes.
    Size { cols: u16, rows: u16 },
    Key { key: Key, ctrl: bool, alt: bool },
    Paste { text: String },
    Mouse { kind: MouseKind, x: u16, y: u16 },
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(tag = "k", content = "c", rename_all = "snake_case")]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Delete,
    Tab,
    BackTab,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
}

/// Left button only for now; scrolling is the wheel.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MouseKind {
    Down,
    Up,
    Drag,
    ScrollUp,
    ScrollDown,
}

/// Packs a color into one number: -1 is the default color, 0 to 255 are the
/// indexed colors, and anything with bit 24 set is `0x01RRGGBB`.
pub fn color_code(color: Color) -> i32 {
    match color {
        Color::Reset => -1,
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        Color::Indexed(i) => i as i32,
        Color::Rgb(r, g, b) => 0x0100_0000 | (r as i32) << 16 | (g as i32) << 8 | b as i32,
    }
}

/// The other half of [`color_code`]. Indexed colors stay indexed.
pub fn code_color(code: i32) -> Color {
    match code {
        0..=255 => Color::Indexed(code as u8),
        c if c > 0 && c & 0x0100_0000 != 0 => Color::Rgb((c >> 16) as u8, (c >> 8) as u8, c as u8),
        _ => Color::Reset,
    }
}

pub fn modifier_code(m: Modifier) -> u16 {
    m.bits()
}

pub fn code_modifier(bits: u16) -> Modifier {
    Modifier::from_bits_truncate(bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colors_round_trip() {
        for c in [Color::Reset, Color::Indexed(200), Color::Rgb(0xf9, 0x25, 0x71), Color::Rgb(0, 0, 0)] {
            assert_eq!(code_color(color_code(c)), c);
        }
        assert_eq!(code_color(color_code(Color::Red)), Color::Indexed(1));
    }

    #[test]
    fn messages_are_small_tagged_json() {
        let key = FromPhone::Key { key: Key::Char('x'), ctrl: true, alt: false };
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, r#"{"t":"key","key":{"k":"char","c":"x"},"ctrl":true,"alt":false}"#);
        assert_eq!(serde_json::from_str::<FromPhone>(&json).unwrap(), key);
        let enter: FromPhone = serde_json::from_str(r#"{"t":"key","key":{"k":"enter"},"ctrl":false,"alt":false}"#).unwrap();
        assert_eq!(enter, FromPhone::Key { key: Key::Enter, ctrl: false, alt: false });
    }
}
