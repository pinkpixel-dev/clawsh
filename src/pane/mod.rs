mod emulator;
mod input;
mod png;
mod copy;
pub(crate) mod pty;

use std::path::Path;
use std::sync::mpsc::Sender;
use std::time::{Instant, SystemTime};

use anyhow::Result;
use crossterm::event::{KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use portable_pty::PtySize;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Style;

pub use emulator::{CursorState, Emulator, ImageData};
use input::InputEncoder;
use copy::{Clicks, CopyMode, CopyOutcome, Spot};
use pty::Pty;

use crate::event::AppEvent;
use crate::host::CellPx;
use crate::host_colors::HostColors;
use crate::persist::SessionRef;
use crate::review::Review;
use crate::status::Status;
use crate::template;
use crate::theme;
use crate::title;

/// One running program: its PTY, its emulator, and where it is drawn.
pub struct Pane {
    id: usize,
    pty: Pty,
    emu: Emulator,
    input: InputEncoder,
    area: Rect,
    cell: CellPx,
    pub status: Status,
    /// The Claude conversation in this pane, once a hook has named it.
    pub session: Option<SessionRef>,
    /// Set while the pane is in copy mode (`ctrl+b [`).
    copy: Option<CopyMode>,
    /// Where a mouse selection started, while the button is held.
    drag: Option<Spot>,
    /// Repeat presses, for double-click word and triple-click line.
    clicks: Clicks,
    /// A name you gave the tab (`ctrl+b ,`). Auto-titling leaves it alone.
    pub name: Option<String>,
    /// The name the transcript suggests, once it has one.
    transcript_title: Option<String>,
    /// When the tab last did something: a hook fired, or it was opened.
    pub last_active: SystemTime,
    /// The worktree tree saved when the last prompt was sent.
    pub turn_start: Option<String>,
    /// The review view's state, kept so comments survive closing it.
    pub review: Option<Review>,
    /// The template tab this pane was opened from, so `ctrl+b T` can save
    /// its flags and prompt again.
    pub from_template: Option<template::Tab>,
    /// The tab's terminal drawer (`ctrl+b e`), once it's been opened.
    pub drawer: Option<Drawer>,
}

/// A shell in the tab's folder, drawn beside or below Claude.
pub struct Drawer {
    pub pane: Box<Pane>,
    /// Hidden drawers keep their shell running.
    pub shown: bool,
    /// Keys go to the drawer instead of Claude. Only while it's shown.
    pub focused: bool,
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
        colors: HostColors,
        tx: Sender<AppEvent>,
    ) -> Result<Self> {
        let (cols, rows) = grid(area);
        let pty = Pty::spawn(id, program, args, env, cwd, pty_size(cols, rows, cell), tx)?;
        let emu = Emulator::new(cols, rows, cell, colors, pty.writer())?;
        Ok(Self {
            id,
            pty,
            emu,
            input: InputEncoder::new()?,
            area,
            cell,
            status: Status::Idle,
            session: None,
            copy: None,
            drag: None,
            clicks: Clicks::default(),
            name: None,
            transcript_title: None,
            last_active: SystemTime::now(),
            turn_start: None,
            review: None,
            from_template: None,
            drawer: None,
        })
    }

    pub fn id(&self) -> usize {
        self.id
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.emu.feed(bytes);
    }

    pub fn set_host_colors(&mut self, colors: HostColors) -> Result<()> {
        self.emu.set_host_colors(colors)
    }

    /// Bytes this pane wants sent to the host terminal (clipboard writes).
    pub fn take_host_output(&mut self) -> Vec<u8> {
        self.emu.take_host_output()
    }

    /// What the tab is called: your name for it, then Claude's terminal
    /// title, then whatever the transcript says, then just `claude`.
    pub fn title(&self) -> String {
        self.name
            .clone()
            .or_else(|| self.emu.title().as_deref().and_then(title::clean))
            .or_else(|| self.transcript_title.clone())
            .unwrap_or_else(|| "claude".into())
    }

    /// Looks for a name in the transcript, until one turns up. Claude's
    /// generated title and your first prompt don't change once they exist.
    pub fn refresh_transcript_title(&mut self) {
        if self.transcript_title.is_some() {
            return;
        }
        if let Some(path) = self.session.as_ref().and_then(|s| s.transcript.as_deref()) {
            self.transcript_title = title::from_transcript(path);
        }
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
        let cursor = self.emu.render(self.area, buf, remap)?;
        let Some(copy) = &self.copy else {
            return Ok(cursor);
        };
        // In copy mode the program's cursor is hidden and ours is drawn as a
        // block in the accent color.
        if let Some(at) = copy.cursor_in_view(&self.emu)? {
            let cell = Position::new(self.area.x + at.x, self.area.y + at.y);
            if self.area.contains(cell) {
                let p = theme::palette();
                buf[cell].set_style(Style::new().bg(p.accent).fg(p.surface));
            }
        }
        Ok(None)
    }

    pub fn enter_copy_mode(&mut self) -> Result<()> {
        self.drag = None;
        self.copy = Some(CopyMode::enter(&mut self.emu)?);
        Ok(())
    }

    /// A key while in copy mode. Returns false once copy mode is over.
    pub fn copy_key(&mut self, k: &KeyEvent) -> Result<bool> {
        let Some(copy) = &mut self.copy else {
            return Ok(false);
        };
        match copy.key(&mut self.emu, k)? {
            CopyOutcome::Stay => Ok(true),
            CopyOutcome::Done => {
                self.exit_copy_mode()?;
                Ok(false)
            }
        }
    }

    pub fn exit_copy_mode(&mut self) -> Result<()> {
        if self.copy.take().is_some() {
            copy::leave(&mut self.emu)?;
        }
        Ok(())
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
        // The view is frozen in copy mode; the keyboard drives it.
        if self.copy.is_some() {
            return Ok(());
        }
        if !self.emu.terminal().is_mouse_tracking()? && self.select_with_mouse(ev)? {
            return Ok(());
        }
        let bytes = self
            .input
            .mouse(self.emu.terminal_mut(), ev, self.area, self.cell)?;
        self.pty.write(bytes);
        Ok(())
    }

    /// Drag-to-select for programs that don't use the mouse themselves.
    /// Releasing the button copies the selection; a double click copies the
    /// word under it and a triple click the line. Returns false for events
    /// it doesn't handle (wheel, other buttons).
    fn select_with_mouse(&mut self, ev: &MouseEvent) -> Result<bool> {
        let x = ev.column.saturating_sub(self.area.x).min(self.area.width.saturating_sub(1));
        let y = ev.row.saturating_sub(self.area.y).min(self.area.height.saturating_sub(1));
        let left = MouseButton::Left;
        match ev.kind {
            MouseEventKind::Down(b) if b == left => {
                let at = self.emu.spot_at_view(x, y)?;
                self.drag = None;
                let picked = match self.clicks.press(at, Instant::now()) {
                    2 => self.emu.select_word_at(at)?,
                    3 => self.emu.select_line_at(at)?,
                    _ => {
                        self.drag = Some(at);
                        self.emu.clear_selection()?;
                        false
                    }
                };
                if picked {
                    self.emu.copy_selection()?;
                }
            }
            MouseEventKind::Drag(b) if b == left => {
                let Some(start) = self.drag else { return Ok(false) };
                let here = self.emu.spot_at_view(x, y)?;
                self.emu.select(start, here)?;
            }
            MouseEventKind::Up(b) if b == left => {
                let Some(start) = self.drag.take() else { return Ok(false) };
                let here = self.emu.spot_at_view(x, y)?;
                if here != start {
                    self.emu.select(start, here)?;
                    self.emu.copy_selection()?;
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
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
