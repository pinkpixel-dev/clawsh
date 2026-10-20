//! Fake Claw$h state: a few tabs of Claude-ish output, an input line, and a
//! log of the raw browser events so we can see what the phone actually sends.

use std::collections::VecDeque;

use ratzilla::ratatui::layout::Rect;

const EVENT_LOG: usize = 7;

pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Tab,
    Esc,
    Up,
    Down,
    Left,
    Right,
}

pub struct Tab {
    pub name: &'static str,
    pub lines: Vec<String>,
}

pub struct App {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub input: String,
    /// Text the IME is still composing, drawn dimmed after the input.
    pub preview: String,
    pub events: VecDeque<String>,
    pub ctrl: bool,
    pub prefix: bool,
    /// Lines scrolled up from the bottom.
    pub scroll: usize,
    /// Click targets from the last draw: (area, tab index).
    pub hits: Vec<(Rect, usize)>,
    /// Cell size in CSS pixels, from the backend.
    pub cell: (f64, f64),
}

impl App {
    pub fn new() -> Self {
        let tabs = ["main", "api", "docs"]
            .into_iter()
            .map(|name| Tab { name, lines: fake_output(name) })
            .collect();
        Self {
            tabs,
            active: 0,
            input: String::new(),
            preview: String::new(),
            events: VecDeque::new(),
            ctrl: false,
            prefix: false,
            scroll: 0,
            hits: Vec::new(),
            cell: (8.0, 17.0),
        }
    }

    pub fn log(&mut self, event: impl Into<String>) {
        self.events.push_front(event.into());
        self.events.truncate(EVENT_LOG);
    }

    pub fn key(&mut self, key: Key) {
        if self.prefix {
            self.prefix = false;
            if let Key::Char(c) = key {
                self.prefix_command(c);
            }
            return;
        }
        if self.ctrl {
            self.ctrl = false;
            if let Key::Char(c) = key {
                self.ctrl_char(c);
                return;
            }
        }
        match key {
            Key::Char(c) => self.input.push(c),
            Key::Backspace => {
                self.input.pop();
            }
            Key::Enter => self.submit(),
            Key::Esc => self.input.clear(),
            Key::Tab => self.input.push_str("  "),
            Key::Up => self.scroll_by(1),
            Key::Down => self.scroll_by(-1),
            Key::Left | Key::Right => {}
        }
    }

    pub fn ctrl_char(&mut self, c: char) {
        match c.to_ascii_lowercase() {
            'b' => self.prefix = true,
            'c' => {
                self.input.clear();
                self.log("^C");
            }
            other => self.log(format!("ctrl+{other}")),
        }
    }

    fn prefix_command(&mut self, c: char) {
        match c {
            'n' => self.select((self.active + 1) % self.tabs.len()),
            'p' => self.select((self.active + self.tabs.len() - 1) % self.tabs.len()),
            '1'..='9' => {
                let index = c as usize - '1' as usize;
                if index < self.tabs.len() {
                    self.select(index);
                }
            }
            other => self.log(format!("prefix: unbound {other:?}")),
        }
    }

    pub fn select(&mut self, index: usize) {
        self.active = index;
        self.scroll = 0;
    }

    pub fn scroll_by(&mut self, lines: isize) {
        let max = self.tabs[self.active].lines.len();
        self.scroll = self.scroll.saturating_add_signed(lines).min(max);
    }

    fn submit(&mut self) {
        let text = std::mem::take(&mut self.input);
        let lines = &mut self.tabs[self.active].lines;
        lines.push(format!("> {text}"));
        lines.push(format!("● Got it, {} chars. (spike echo)", text.chars().count()));
        lines.push(String::new());
        self.scroll = 0;
    }
}

fn fake_output(name: &str) -> Vec<String> {
    let steps = [
        "● Reading src/app.rs",
        "● Reading src/workspace.rs",
        "● Searching for \"Workspaces::replace_tab\"",
        "  Found 4 matches in 2 files",
        "● Editing src/app/input.rs",
        "  +12 −3",
        "● Running cargo test --lib",
        "  test result: ok. 148 passed; 0 failed",
    ];
    let mut lines = vec![format!("> work on the {name} tab"), String::new()];
    for round in 1..=4 {
        lines.push(format!("── round {round} ──"));
        lines.extend(steps.iter().map(|s| s.to_string()));
        lines.push(String::new());
    }
    lines.push("● Done. Wide chars: 漢字 かな, emoji: ✓ ✗ ●".into());
    lines.push(String::new());
    lines
}
