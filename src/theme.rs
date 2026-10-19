//! Color tokens for clawsh's own chrome. Pane contents keep the
//! colors the program asked for.

use ratatui::style::{Color, Modifier, Style};

pub const SURFACE: Color = Color::Rgb(0x1c, 0x1d, 0x21);
pub const TEXT: Color = Color::Rgb(0xec, 0xec, 0xef);
pub const MUTED: Color = Color::Rgb(0x8a, 0x8c, 0x96);
pub const ACCENT: Color = Color::Rgb(0xf2, 0x6d, 0x9b);
pub const WARN: Color = Color::Rgb(0xe8, 0xb4, 0x5a);

pub fn bar() -> Style {
    Style::new().bg(SURFACE).fg(MUTED)
}

pub fn key_hint() -> Style {
    Style::new().fg(TEXT).add_modifier(Modifier::BOLD)
}

pub fn accent() -> Style {
    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn warn() -> Style {
    Style::new().fg(WARN)
}
