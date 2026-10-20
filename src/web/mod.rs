//! Phone access: a WebSocket server inside the running Claw$h, plus the
//! pieces that turn the phone's input into ordinary events and Claw$h's
//! frames into cell diffs. See `DOCS/WEB_ACCESS.md` for the plan.

mod assets;
// Each side uses half of the color helpers.
#[allow(dead_code)]
pub mod protocol;
mod server;

use std::sync::mpsc::Sender;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::backend::TestBackend;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Position, Rect};
use ratatui::style::Color;
use ratatui::Terminal;

use crate::host_colors::HostColors;
use protocol::{CellDiff, FromPhone, Key, MouseKind, ToPhone, color_code, modifier_code};
pub use server::{DEFAULT_ADDR, WebServer};

/// What the web threads tell the main loop.
#[derive(Debug)]
pub enum WebEvent {
    /// A phone opened the socket; frames for it go down this sender.
    Connected(u64, Sender<String>),
    Message(u64, FromPhone),
    Gone(u64),
}

/// The phone's own off-screen terminal, and what it was last sent.
pub struct Capture {
    term: Terminal<TestBackend>,
    last: Option<Buffer>,
    cursor: Option<(u16, u16)>,
}

impl Capture {
    pub fn new(cols: u16, rows: u16) -> Self {
        let term = Terminal::new(TestBackend::new(cols.max(1), rows.max(1))).expect("test backend never fails");
        Self { term, last: None, cursor: None }
    }

    pub fn size(&self) -> Rect {
        self.term.backend().buffer().area
    }

    pub fn terminal(&mut self) -> &mut Terminal<TestBackend> {
        &mut self.term
    }

    /// The changes since the last frame sent, or `None` when nothing changed.
    /// Default colors become the desktop terminal's colors when it told us
    /// what they are, so the phone matches it.
    pub fn frame(&mut self, buf: &Buffer, cursor: Option<Position>, colors: HostColors) -> Option<ToPhone> {
        let cursor = cursor.map(|p| (p.x, p.y));
        let area = buf.area;
        let full = self.last.as_ref().is_none_or(|last| last.area != area);
        let cells: Vec<CellDiff> = if full {
            let width = area.width.max(1) as usize;
            let at = |i: usize| ((i % width) as u16, (i / width) as u16);
            buf.content.iter().enumerate().map(|(i, c)| { let (x, y) = at(i); diff(x, y, c, colors) }).collect()
        } else {
            let last = self.last.as_ref().expect("checked above");
            last.diff(buf).into_iter().map(|(x, y, c)| diff(x, y, c, colors)).collect()
        };
        if !full && cells.is_empty() && cursor == self.cursor {
            return None;
        }
        self.last = Some(buf.clone());
        self.cursor = cursor;
        Some(ToPhone::Frame { cols: area.width, rows: area.height, full, cells, cursor })
    }
}

fn diff(x: u16, y: u16, cell: &Cell, colors: HostColors) -> CellDiff {
    let host = |c: Option<libghostty_vt::style::RgbColor>| c.map_or(Color::Reset, |c| Color::Rgb(c.r, c.g, c.b));
    let fg = if cell.fg == Color::Reset { host(colors.fg) } else { cell.fg };
    let bg = if cell.bg == Color::Reset { host(colors.bg) } else { cell.bg };
    CellDiff { x, y, s: cell.symbol().to_owned(), fg: color_code(fg), bg: color_code(bg), m: modifier_code(cell.modifier) }
}

/// The phone's input as the same events the desktop terminal produces.
/// `Size` isn't input, so it gives `None`.
pub fn to_event(msg: FromPhone) -> Option<Event> {
    Some(match msg {
        FromPhone::Size { .. } => return None,
        FromPhone::Paste { text } => Event::Paste(text),
        FromPhone::Key { key, ctrl, alt } => {
            let mut mods = KeyModifiers::NONE;
            mods.set(KeyModifiers::CONTROL, ctrl);
            mods.set(KeyModifiers::ALT, alt);
            let code = match key {
                Key::Char(c) => {
                    if c.is_uppercase() {
                        mods |= KeyModifiers::SHIFT;
                    }
                    KeyCode::Char(c)
                }
                Key::Enter => KeyCode::Enter,
                Key::Backspace => KeyCode::Backspace,
                Key::Delete => KeyCode::Delete,
                Key::Tab => KeyCode::Tab,
                Key::BackTab => {
                    mods |= KeyModifiers::SHIFT;
                    KeyCode::BackTab
                }
                Key::Esc => KeyCode::Esc,
                Key::Up => KeyCode::Up,
                Key::Down => KeyCode::Down,
                Key::Left => KeyCode::Left,
                Key::Right => KeyCode::Right,
                Key::Home => KeyCode::Home,
                Key::End => KeyCode::End,
                Key::PageUp => KeyCode::PageUp,
                Key::PageDown => KeyCode::PageDown,
            };
            Event::Key(KeyEvent::new(code, mods))
        }
        FromPhone::Mouse { kind, x, y } => Event::Mouse(MouseEvent {
            kind: match kind {
                MouseKind::Down => MouseEventKind::Down(MouseButton::Left),
                MouseKind::Up => MouseEventKind::Up(MouseButton::Left),
                MouseKind::Drag => MouseEventKind::Drag(MouseButton::Left),
                MouseKind::ScrollUp => MouseEventKind::ScrollUp,
                MouseKind::ScrollDown => MouseEventKind::ScrollDown,
            },
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    #[test]
    fn first_frame_is_full_then_only_changes() {
        let mut cap = Capture::new(4, 2);
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        let Some(ToPhone::Frame { full, cells, .. }) = cap.frame(&buf, None, HostColors::default()) else { panic!() };
        assert!(full);
        assert_eq!(cells.len(), 8);
        assert!(cap.frame(&buf, None, HostColors::default()).is_none());

        buf.set_string(1, 1, "x", Style::new().fg(Color::Rgb(1, 2, 3)));
        let Some(ToPhone::Frame { full, cells, cursor, .. }) = cap.frame(&buf, Some(Position::new(2, 1)), HostColors::default()) else { panic!() };
        assert!(!full);
        assert_eq!(cursor, Some((2, 1)));
        assert_eq!(cells, vec![CellDiff { x: 1, y: 1, s: "x".into(), fg: 0x0101_0203, bg: -1, m: 0 }]);
    }

    #[test]
    fn a_new_size_sends_everything_again() {
        let mut cap = Capture::new(2, 1);
        cap.frame(&Buffer::empty(Rect::new(0, 0, 2, 1)), None, HostColors::default());
        let Some(ToPhone::Frame { full, cells, .. }) = cap.frame(&Buffer::empty(Rect::new(0, 0, 3, 1)), None, HostColors::default()) else { panic!() };
        assert!(full);
        assert_eq!(cells.len(), 3);
    }

    #[test]
    fn phone_keys_become_crossterm_keys() {
        let ev = to_event(FromPhone::Key { key: Key::Char('b'), ctrl: true, alt: false });
        assert_eq!(ev, Some(Event::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL))));
        let ev = to_event(FromPhone::Key { key: Key::Char('W'), ctrl: false, alt: false });
        assert_eq!(ev, Some(Event::Key(KeyEvent::new(KeyCode::Char('W'), KeyModifiers::SHIFT))));
        assert_eq!(to_event(FromPhone::Size { cols: 1, rows: 1 }), None);
    }
}
