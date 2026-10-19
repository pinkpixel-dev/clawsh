use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use anyhow::Result;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;

use crate::event::{self, AppEvent};
use crate::graphics::ImageBridge;
use crate::host::{self, CellPx};
use crate::pane::{CursorState, Pane};
use crate::ui;

pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

/// The single pane's ID until tabs land.
const PANE_ID: usize = 0;

struct App {
    pane: Pane,
    images: ImageBridge,
    cell: CellPx,
    prefix: bool,
    quit: bool,
    cursor_style: Option<CursorState>,
}

pub fn run(term: &mut DefaultTerminal, launch: Launch) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    event::spawn_input_reader(tx.clone());

    let cell = CellPx::query();
    let areas = ui::layout(term.size()?.into());
    let pane = Pane::spawn(
        PANE_ID,
        &launch.program,
        &launch.args,
        &launch.cwd,
        areas.pane,
        cell,
        tx,
    )?;
    let mut app = App {
        pane,
        images: ImageBridge::new(),
        cell,
        prefix: false,
        quit: false,
        cursor_style: None,
    };

    let result = (|| {
        while !app.quit {
            app.draw(term)?;
            app.wait(&rx)?;
        }
        Ok(())
    })();

    let mut out = Vec::new();
    app.images.clear(&mut out);
    write_host(&out)?;
    result
}

/// Sends kitty graphics commands straight to the host, outside ratatui.
fn write_host(bytes: &[u8]) -> Result<()> {
    if !bytes.is_empty() {
        let mut stdout = io::stdout().lock();
        stdout.write_all(bytes)?;
        stdout.flush()?;
    }
    Ok(())
}

impl App {
    fn draw(&mut self, term: &mut DefaultTerminal) -> Result<()> {
        let areas = ui::layout(term.size()?.into());
        self.pane.set_area(areas.pane, self.cell)?;

        // Images have to exist on the host before the cells that show them.
        let mut out = Vec::new();
        self.images.sync(PANE_ID, self.pane.emulator_mut(), &mut out)?;
        write_host(&out)?;

        let images = &self.images;
        let mut result = Ok(None);
        term.draw(|frame| {
            result = self
                .pane
                .render(frame.buffer_mut(), |id| images.remap(PANE_ID, id));
            let title = self.pane.title();
            ui::draw_footer(
                frame,
                areas.footer,
                &ui::FooterState {
                    title: title.as_deref(),
                    prefix: self.prefix,
                    exited: self.pane.exited,
                },
            );
            if let Ok(Some(c)) = &result
                && !self.prefix
            {
                let area = self.pane.area();
                frame.set_cursor_position((area.x + c.position.x, area.y + c.position.y));
            }
        })?;

        let cursor = result?;
        if let Some(c) = cursor
            && self.cursor_style.is_none_or(|old| (old.style, old.blinking) != (c.style, c.blinking))
        {
            host::apply_cursor_style(&c)?;
            self.cursor_style = Some(c);
        }
        Ok(())
    }

    /// Blocks for the next event, then drains whatever else is queued so a
    /// burst of output becomes one redraw.
    fn wait(&mut self, rx: &Receiver<AppEvent>) -> Result<()> {
        let ev = rx.recv()?;
        self.handle(ev)?;
        while let Ok(ev) = rx.try_recv() {
            self.handle(ev)?;
        }
        Ok(())
    }

    fn handle(&mut self, ev: AppEvent) -> Result<()> {
        match ev {
            AppEvent::PtyOutput(_, bytes) => self.pane.feed(&bytes),
            AppEvent::PtyExited(_) => {
                self.pane.exited = true;
                self.quit = true;
            }
            AppEvent::Input(input) => match input {
                Event::Key(k) => self.key(k)?,
                Event::Mouse(m) => self.pane.mouse(&m)?,
                Event::Paste(text) => self.pane.paste(&text)?,
                Event::FocusGained => self.pane.focus(true)?,
                Event::FocusLost => self.pane.focus(false)?,
                Event::Resize(..) => self.cell = CellPx::query(),
            },
        }
        Ok(())
    }

    fn key(&mut self, k: KeyEvent) -> Result<()> {
        if is_prefix(&k) {
            // Releases of the prefix itself never reach the pane.
            if k.kind == KeyEventKind::Press {
                if self.prefix {
                    self.prefix = false;
                    self.pane.key(&k)?;
                } else {
                    self.prefix = true;
                }
            }
            return Ok(());
        }
        if !self.prefix {
            return self.pane.key(&k);
        }
        if k.kind != KeyEventKind::Press {
            return Ok(());
        }
        self.prefix = false;
        if let KeyCode::Char('q') = k.code {
            self.quit = true;
        }
        Ok(())
    }
}

fn is_prefix(k: &KeyEvent) -> bool {
    k.code == KeyCode::Char('b') && k.modifiers == KeyModifiers::CONTROL
}
