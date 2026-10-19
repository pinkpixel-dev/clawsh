use std::io::{self, Write};

use anyhow::Result;
use crossterm::cursor::SetCursorStyle;
use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{self, supports_keyboard_enhancement};
use libghostty_vt::render::CursorVisualStyle;

use crate::pane::CursorState;

/// Pixel size of one cell on the host terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellPx {
    pub w: u32,
    pub h: u32,
}

impl CellPx {
    /// Used when the host does not report its pixel size.
    const FALLBACK: Self = Self { w: 8, h: 16 };

    pub fn query() -> Self {
        match terminal::window_size() {
            Ok(ws) if ws.columns > 0 && ws.rows > 0 && ws.width > 0 && ws.height > 0 => Self {
                w: (ws.width / ws.columns).max(1) as u32,
                h: (ws.height / ws.rows).max(1) as u32,
            },
            _ => Self::FALLBACK,
        }
    }
}

/// Puts the host terminal into the modes we need and undoes them on drop,
/// including when we panic.
pub struct HostGuard {
    kitty_keys: bool,
}

impl HostGuard {
    pub fn enter() -> Result<Self> {
        let kitty_keys = supports_keyboard_enhancement().unwrap_or(false);
        let mut out = io::stdout();
        // Every key arrives as an unambiguous kitty event, with press/release,
        // so the pane can get whatever its own kitty flags ask for.
        if kitty_keys {
            execute!(
                out,
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                        | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
                        | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                )
            )?;
        }
        execute!(
            out,
            EnableMouseCapture,
            EnableBracketedPaste,
            EnableFocusChange
        )?;

        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            restore(kitty_keys);
            hook(info);
        }));
        Ok(Self { kitty_keys })
    }
}

impl Drop for HostGuard {
    fn drop(&mut self) {
        restore(self.kitty_keys);
    }
}

fn restore(kitty_keys: bool) {
    let mut out = io::stdout();
    let _ = execute!(
        out,
        DisableFocusChange,
        DisableBracketedPaste,
        DisableMouseCapture,
        SetCursorStyle::DefaultUserShape
    );
    if kitty_keys {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
    let _ = out.flush();
}

/// Mirrors the focused pane's cursor shape onto the host cursor.
pub fn apply_cursor_style(cursor: &CursorState) -> Result<()> {
    let style = match (cursor.style, cursor.blinking) {
        (CursorVisualStyle::Bar, true) => SetCursorStyle::BlinkingBar,
        (CursorVisualStyle::Bar, false) => SetCursorStyle::SteadyBar,
        (CursorVisualStyle::Underline, true) => SetCursorStyle::BlinkingUnderScore,
        (CursorVisualStyle::Underline, false) => SetCursorStyle::SteadyUnderScore,
        (_, true) => SetCursorStyle::BlinkingBlock,
        (_, false) => SetCursorStyle::SteadyBlock,
    };
    execute!(io::stdout(), style)?;
    Ok(())
}
