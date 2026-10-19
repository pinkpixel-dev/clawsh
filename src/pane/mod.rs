mod emulator;
mod input;
mod png;
mod pty;

use std::path::Path;
use std::sync::mpsc::Sender;

use anyhow::Result;
use crossterm::event::{KeyEvent, MouseEvent};
use portable_pty::PtySize;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

pub use emulator::{CursorState, Emulator, ImageData};
use input::InputEncoder;
use pty::Pty;

use crate::event::AppEvent;
use crate::host::CellPx;
use crate::status::Status;

/// One running program: its PTY, its emulator, and where it is drawn.
pub struct Pane {
    id: usize,
    pty: Pty,
    emu: Emulator,
    input: InputEncoder,
    area: Rect,
    cell: CellPx,
    pub status: Status,
}

impl Pane {
    pub fn spawn(
        id: usize,
        program: &str,
        args: &[String],
        env: &[(&str, String)],
        cwd: &Path,
        area: Rect,
        cell: CellPx,
        tx: Sender<AppEvent>,
    ) -> Result<Self> {
        let (cols, rows) = grid(area);
        let pty = Pty::spawn(id, program, args, env, cwd, pty_size(cols, rows, cell), tx)?;
        let emu = Emulator::new(cols, rows, cell, pty.writer())?;
        Ok(Self {
            id,
            pty,
            emu,
            input: InputEncoder::new()?,
            area,
            cell,
            status: Status::Idle,
        })
    }

    pub fn id(&self) -> usize {
        self.id
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.emu.feed(bytes);
    }

    pub fn title(&self) -> Option<String> {
        self.emu.title()
    }

    /// Moves or resizes the pane; the program only hears about real size changes.
    pub fn set_area(&mut self, area: Rect, cell: CellPx) -> Result<()> {
        let size_changed = (area.width, area.height) != (self.area.width, self.area.height)
            || cell != self.cell;
        self.area = area;
        self.cell = cell;
        if size_changed {
            let (cols, rows) = grid(area);
            self.emu.resize(cols, rows, cell)?;
            self.pty.resize(pty_size(cols, rows, cell));
        }
        Ok(())
    }

    pub fn render(
        &mut self,
        buf: &mut Buffer,
        remap: impl Fn(u32) -> Option<u32>,
    ) -> Result<Option<CursorState>> {
        self.emu.render(self.area, buf, remap)
    }

    pub fn emulator_mut(&mut self) -> &mut Emulator {
        &mut self.emu
    }

    pub fn area(&self) -> Rect {
        self.area
    }

    pub fn key(&mut self, ev: &KeyEvent) -> Result<()> {
        let bytes = self.input.key(self.emu.terminal(), ev)?;
        self.pty.write(bytes);
        Ok(())
    }

    pub fn mouse(&mut self, ev: &MouseEvent) -> Result<()> {
        let bytes = self
            .input
            .mouse(self.emu.terminal_mut(), ev, self.area, self.cell)?;
        self.pty.write(bytes);
        Ok(())
    }

    pub fn paste(&mut self, text: &str) -> Result<()> {
        let bytes = input::paste_bytes(self.emu.terminal(), text)?;
        self.pty.write(&bytes);
        Ok(())
    }

    pub fn focus(&mut self, gained: bool) -> Result<()> {
        let bytes = input::focus_bytes(self.emu.terminal(), gained)?;
        self.pty.write(&bytes);
        Ok(())
    }
}

fn grid(area: Rect) -> (u16, u16) {
    (area.width.max(1), area.height.max(1))
}

fn pty_size(cols: u16, rows: u16, cell: CellPx) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: (cols as u32 * cell.w).min(u16::MAX as u32) as u16,
        pixel_height: (rows as u32 * cell.h).min(u16::MAX as u32) as u16,
    }
}
