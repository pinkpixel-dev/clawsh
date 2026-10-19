//! Color tokens for Claw$h's own chrome. Pane contents keep the colors the
//! program asked for.
//!
//! There's a dark and a light palette. Which one is used is decided once at
//! startup from the terminal's own background, or `CLAWSH_THEME`.

use std::sync::atomic::{AtomicBool, Ordering};

use libghostty_vt::terminal::ColorScheme;
use ratatui::style::{Color, Modifier, Style};

pub struct Palette {
    pub surface: Color,
    /// The active workspace or tab: a faint pink tint over the surface.
    pub surface_active: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    pub accent: Color,
    pub ok: Color,
    pub warn: Color,
    pub danger: Color,
}

pub static DARK: Palette = Palette {
    surface: Color::Rgb(0x1c, 0x1d, 0x21),
    surface_active: Color::Rgb(0x34, 0x24, 0x2c),
    border: Color::Rgb(0x33, 0x35, 0x3c),
    text: Color::Rgb(0xec, 0xec, 0xef),
    muted: Color::Rgb(0x8a, 0x8c, 0x96),
    faint: Color::Rgb(0x5c, 0x5e, 0x67),
    accent: Color::Rgb(0xf2, 0x6d, 0x9b),
    ok: Color::Rgb(0x6c, 0xcf, 0x8e),
    warn: Color::Rgb(0xe8, 0xb4, 0x5a),
    danger: Color::Rgb(0xec, 0x6a, 0x6a),
};

/// Warm off-white, with the accent and status colors darkened so anything
/// used as text clears 4.5:1 contrast on both the panel and the active row.
pub static LIGHT: Palette = Palette {
    surface: Color::Rgb(0xf2, 0xf0, 0xec),
    surface_active: Color::Rgb(0xf6, 0xdf, 0xe7),
    border: Color::Rgb(0xd8, 0xd4, 0xce),
    text: Color::Rgb(0x23, 0x24, 0x2a),
    muted: Color::Rgb(0x62, 0x64, 0x6c),
    faint: Color::Rgb(0x8a, 0x8c, 0x94),
    accent: Color::Rgb(0xb0, 0x25, 0x5d),
    ok: Color::Rgb(0x17, 0x76, 0x3f),
    warn: Color::Rgb(0x8c, 0x54, 0x00),
    danger: Color::Rgb(0xb5, 0x2a, 0x25),
};

static LIGHT_ON: AtomicBool = AtomicBool::new(false);
/// Set by `CLAWSH_THEME`; the terminal's own changes are ignored then.
static PINNED: AtomicBool = AtomicBool::new(false);

/// Picks the palette for this run. `CLAWSH_THEME=light|dark` wins; then the
/// terminal's own light or dark background; dark when neither says.
pub fn choose(forced: Option<&str>, scheme: Option<ColorScheme>) -> &'static Palette {
    match (forced, scheme) {
        (Some("light"), _) => &LIGHT,
        (Some("dark"), _) => &DARK,
        (_, Some(ColorScheme::Light)) => &LIGHT,
        _ => &DARK,
    }
}

/// Sets the palette at startup.
pub fn init(forced: Option<&str>, scheme: Option<ColorScheme>) {
    LIGHT_ON.store(std::ptr::eq(choose(forced, scheme), &LIGHT), Ordering::Relaxed);
    PINNED.store(matches!(forced, Some("light" | "dark")), Ordering::Relaxed);
}

/// Follows the terminal switching between light and dark, unless
/// `CLAWSH_THEME` pinned the palette.
pub fn follow(scheme: ColorScheme) {
    if !PINNED.load(Ordering::Relaxed) {
        LIGHT_ON.store(scheme == ColorScheme::Light, Ordering::Relaxed);
    }
}

pub fn palette() -> &'static Palette {
    if LIGHT_ON.load(Ordering::Relaxed) { &LIGHT } else { &DARK }
}

pub fn bar() -> Style {
    Style::new().bg(palette().surface).fg(palette().muted)
}

pub fn panel() -> Style {
    Style::new().bg(palette().surface).fg(palette().text)
}

pub fn border() -> Style {
    Style::new().fg(palette().border)
}

pub fn active_row() -> Style {
    Style::new().bg(palette().surface_active).fg(palette().text)
}

pub fn muted() -> Style {
    Style::new().fg(palette().muted)
}

pub fn faint() -> Style {
    Style::new().fg(palette().faint)
}

pub fn key_hint() -> Style {
    Style::new().fg(palette().text).add_modifier(Modifier::BOLD)
}

pub fn accent() -> Style {
    Style::new().fg(palette().accent).add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_terminal_unless_told_otherwise() {
        let is = |p: &Palette, q: &Palette| std::ptr::eq(p, q);
        assert!(is(choose(None, Some(ColorScheme::Light)), &LIGHT));
        assert!(is(choose(None, Some(ColorScheme::Dark)), &DARK));
        assert!(is(choose(None, None), &DARK), "unknown terminals stay dark");
        assert!(is(choose(Some("dark"), Some(ColorScheme::Light)), &DARK));
        assert!(is(choose(Some("light"), None), &LIGHT));
        assert!(is(choose(Some("purple"), Some(ColorScheme::Light)), &LIGHT), "unknown values are ignored");
    }
}
