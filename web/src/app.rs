//! The client's state: the screen Claw$h last sent, the sticky ctrl key, and
//! the messages waiting to go out.

use ratzilla::ratatui::{buffer::Buffer, layout::Rect};

use crate::protocol::{FromPhone, Key, MouseKind, ToPhone, code_color, code_modifier};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Connecting,
    Open,
    /// Closed; a retry is scheduled.
    Lost,
}

pub struct App {
    pub screen: Buffer,
    pub cursor: Option<(u16, u16)>,
    pub link: Link,
    /// Sticky ctrl from the key bar: applies to the next key.
    pub ctrl: bool,
    /// Cell size in CSS pixels, from the backend.
    pub cell: (f64, f64),
    /// The grid size Claw$h was last told about.
    pub sent_size: Option<(u16, u16)>,
    outbox: Vec<FromPhone>,
}

impl App {
    pub fn new() -> Self {
        Self {
            screen: Buffer::empty(Rect::ZERO),
            cursor: None,
            link: Link::Connecting,
            ctrl: false,
            cell: (8.0, 17.0),
            sent_size: None,
            outbox: Vec::new(),
        }
    }

    pub fn key(&mut self, key: Key) {
        let ctrl = std::mem::take(&mut self.ctrl);
        self.send(FromPhone::Key { key, ctrl, alt: false });
    }

    pub fn ctrl_char(&mut self, c: char) {
        self.ctrl = false;
        self.send(FromPhone::Key { key: Key::Char(c.to_ascii_lowercase()), ctrl: true, alt: false });
    }

    pub fn paste(&mut self, text: String) {
        self.send(FromPhone::Paste { text });
    }

    pub fn mouse(&mut self, kind: MouseKind, x: u16, y: u16) {
        self.send(FromPhone::Mouse { kind, x, y });
    }

    /// Tells Claw$h the grid size when it changed or after a reconnect.
    pub fn size(&mut self, cols: u16, rows: u16) {
        if self.link == Link::Open && self.sent_size != Some((cols, rows)) {
            self.sent_size = Some((cols, rows));
            self.outbox.push(FromPhone::Size { cols, rows });
        }
    }

    /// Input only goes out while connected; there's no point queueing keys
    /// for a screen nobody can see.
    fn send(&mut self, msg: FromPhone) {
        if self.link == Link::Open {
            self.outbox.push(msg);
        }
    }

    pub fn take_outbox(&mut self) -> Vec<FromPhone> {
        std::mem::take(&mut self.outbox)
    }

    pub fn apply(&mut self, msg: ToPhone) {
        let ToPhone::Frame { cols, rows, full, cells, cursor } = msg;
        let area = Rect::new(0, 0, cols, rows);
        if full || self.screen.area != area {
            self.screen = Buffer::empty(area);
        }
        for c in cells {
            if c.x < cols && c.y < rows {
                let cell = &mut self.screen[(c.x, c.y)];
                cell.set_symbol(&c.s);
                cell.fg = code_color(c.fg);
                cell.bg = code_color(c.bg);
                cell.modifier = code_modifier(c.m);
            }
        }
        self.cursor = cursor;
    }
}
