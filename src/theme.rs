//! Color tokens for clawsh's own chrome. Pane contents keep the
//! colors the program asked for.

use ratatui::style::{Color, Modifier, Style};

pub const SURFACE: Color = Color::Rgb(0x1c, 0x1d, 0x21);
/// The active workspace or tab: a faint pink tint over the surface.
pub const SURFACE_ACTIVE: Color = Color::Rgb(0x34, 0x24, 0x2c);
pub const BORDER: Color = Color::Rgb(0x33, 0x35, 0x3c);
pub const TEXT: Color = Color::Rgb(0xec, 0xec, 0xef);
pub const MUTED: Color = Color::Rgb(0x8a, 0x8c, 0x96);
pub const FAINT: Color = Color::Rgb(0x5c, 0x5e, 0x67);
pub const ACCENT: Color = Color::Rgb(0xf2, 0x6d, 0x9b);

pub fn bar() -> Style {
    Style::new().bg(SURFACE).fg(MUTED)
}

pub fn panel() -> Style {
    Style::new().bg(SURFACE).fg(TEXT)
}

pub fn border() -> Style {
    Style::new().fg(BORDER)
}

pub fn active_row() -> Style {
    Style::new().bg(SURFACE_ACTIVE).fg(TEXT)
}

pub fn muted() -> Style {
    Style::new().fg(MUTED)
}

pub fn faint() -> Style {
    Style::new().fg(FAINT)
}

pub fn key_hint() -> Style {
    Style::new().fg(TEXT).add_modifier(Modifier::BOLD)
}

pub fn accent() -> Style {
    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
}

