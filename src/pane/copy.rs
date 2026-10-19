//! Copy mode (`ctrl+b [`) and mouse selection, on top of libghostty's own
//! selection support.
//!
//! Copy mode freezes the view: a cursor moves through the screen and the
//! scrollback, `v` starts a selection, and `y` copies it to the host
//! clipboard with OSC 52. Positions are kept in full-scrollback coordinates
//! ([`Spot`]) so a selection stays put while the view scrolls.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use libghostty_vt::selection::{FormatOptions, Selection};
use libghostty_vt::terminal::{Point, PointCoordinate, ScrollViewport};
use ratatui::layout::Position;

use super::emulator::{Emulator, osc52};

/// A cell anywhere in the screen plus scrollback (row 0 is the oldest line).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spot {
    pub x: u16,
    pub y: u32,
}

impl Spot {
    fn point(self) -> Point {
        Point::Screen(PointCoordinate { x: self.x, y: self.y })
    }
}

/// What a key did in copy mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyOutcome {
    Stay,
    /// Copied or cancelled; copy mode is over.
    Done,
}

struct Metrics {
    cols: u16,
    /// Rows in the screen plus scrollback.
    total: u32,
    /// The first row the view shows.
    offset: u32,
    /// Rows the view shows.
    rows: u32,
}

impl Emulator {
    fn metrics(&self) -> Result<Metrics> {
        let bar = self.term.scrollbar()?;
        Ok(Metrics {
            cols: self.term.cols()?.max(1),
            total: (bar.total as u32).max(1),
            offset: bar.offset as u32,
            rows: (bar.len as u32).max(1),
        })
    }

    /// The scrollback position of a cell in the visible pane.
    pub(super) fn spot_at_view(&self, x: u16, y: u16) -> Result<Spot> {
        let m = self.metrics()?;
        Ok(Spot {
            x: x.min(m.cols - 1),
            y: (m.offset + y as u32).min(m.total - 1),
        })
    }

    pub(super) fn select(&mut self, from: Spot, to: Spot) -> Result<()> {
        let start = self.term.grid_ref(from.point())?;
        let end = self.term.grid_ref(to.point())?;
        self.term.set_selection(Some(&Selection::new(start, end, false)))?;
        Ok(())
    }

    pub(super) fn clear_selection(&mut self) -> Result<()> {
        self.term.set_selection(None)?;
        Ok(())
    }

    /// Sends the selected text to the host clipboard. False when nothing
    /// (or only whitespace) is selected.
    pub(super) fn copy_selection(&mut self) -> Result<bool> {
        let options = FormatOptions::new().with_unwrap(true).with_trim(true);
        let Some(bytes) = self.term.format_selection_alloc(None, options)? else {
            return Ok(false);
        };
        let text = String::from_utf8_lossy(&bytes).into_owned();
        if text.trim().is_empty() {
            return Ok(false);
        }
        self.host_out.borrow_mut().extend(osc52('c', &text));
        Ok(true)
    }

    /// Scrolls just enough for row `y` to be in view.
    fn reveal(&mut self, y: u32) -> Result<()> {
        let m = self.metrics()?;
        if y < m.offset {
            self.term.scroll_viewport(ScrollViewport::Row(y as usize));
        } else if y >= m.offset + m.rows {
            self.term.scroll_viewport(ScrollViewport::Row((y + 1 - m.rows) as usize));
        }
        Ok(())
    }
}

pub struct CopyMode {
    cursor: Spot,
    /// Where the selection started, once `v` was pressed.
    anchor: Option<Spot>,
}

impl CopyMode {
    /// Starts at the program's own cursor.
    pub fn enter(emu: &mut Emulator) -> Result<Self> {
        let m = emu.metrics()?;
        let active_rows = emu.term.rows()? as u32;
        let cursor = Spot {
            x: emu.term.cursor_x()?.min(m.cols - 1),
            y: (m.total.saturating_sub(active_rows) + emu.term.cursor_y()? as u32).min(m.total - 1),
        };
        emu.clear_selection()?;
        emu.reveal(cursor.y)?;
        Ok(Self { cursor, anchor: None })
    }

    pub fn key(&mut self, emu: &mut Emulator, k: &KeyEvent) -> Result<CopyOutcome> {
        if k.kind == KeyEventKind::Release {
            return Ok(CopyOutcome::Stay);
        }
        let m = emu.metrics()?;
        let (last_x, last_y) = (m.cols - 1, m.total - 1);
        let page = m.rows.saturating_sub(1).max(1);
        let half = (m.rows / 2).max(1);
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let c = &mut self.cursor;
        match k.code {
            KeyCode::Char('q') | KeyCode::Esc => return Ok(CopyOutcome::Done),
            KeyCode::Char('y') | KeyCode::Enter => {
                // Nothing selected: copy the whole line under the cursor.
                if self.anchor.is_none() {
                    emu.select(Spot { x: 0, y: c.y }, Spot { x: last_x, y: c.y })?;
                }
                emu.copy_selection()?;
                return Ok(CopyOutcome::Done);
            }
            KeyCode::Char('v') | KeyCode::Char(' ') => {
                self.anchor = if self.anchor.is_some() { None } else { Some(*c) };
            }
            KeyCode::Char('u') if ctrl => c.y = c.y.saturating_sub(half),
            KeyCode::Char('d') if ctrl => c.y = (c.y + half).min(last_y),
            KeyCode::Left | KeyCode::Char('h') => c.x = c.x.saturating_sub(1),
            KeyCode::Right | KeyCode::Char('l') => c.x = (c.x + 1).min(last_x),
            KeyCode::Up | KeyCode::Char('k') => c.y = c.y.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => c.y = (c.y + 1).min(last_y),
            KeyCode::Home | KeyCode::Char('0') => c.x = 0,
            KeyCode::End | KeyCode::Char('$') => c.x = last_x,
            KeyCode::PageUp => c.y = c.y.saturating_sub(page),
            KeyCode::PageDown => c.y = (c.y + page).min(last_y),
            KeyCode::Char('g') => *c = Spot { x: 0, y: 0 },
            KeyCode::Char('G') => *c = Spot { x: 0, y: last_y },
            _ => return Ok(CopyOutcome::Stay),
        }
        emu.reveal(self.cursor.y)?;
        match self.anchor {
            Some(anchor) => emu.select(anchor, self.cursor)?,
            None => emu.clear_selection()?,
        }
        Ok(CopyOutcome::Stay)
    }

    /// Where to draw the copy cursor inside the pane, if it's in view.
    pub fn cursor_in_view(&self, emu: &Emulator) -> Result<Option<Position>> {
        let m = emu.metrics()?;
        let y = self.cursor.y;
        Ok((y >= m.offset && y < m.offset + m.rows)
            .then(|| Position::new(self.cursor.x, (y - m.offset) as u16)))
    }
}

/// Leaving copy mode: drop the selection and go back to the live screen.
pub fn leave(emu: &mut Emulator) -> Result<()> {
    emu.clear_selection()?;
    emu.term.scroll_viewport(ScrollViewport::Bottom);
    Ok(())
}

#[cfg(test)]
#[path = "copy_tests.rs"]
mod tests;
