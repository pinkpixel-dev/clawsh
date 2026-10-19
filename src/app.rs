mod input;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::Event;
use ratatui::DefaultTerminal;
use ratatui::layout::Rect;

use crate::event::{self, AppEvent};
use crate::graphics::ImageBridge;
use crate::host::{self, CellPx};
use crate::pane::{CursorState, Pane};
use crate::picker::Picker;
use crate::status::{self, Status, StatusServer};
use crate::ui::{self, Hits, TabView, View, WorkspaceView};
use crate::workspace::Workspaces;

pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

enum Mode {
    Normal,
    /// `ctrl+b` was pressed; the next key is a command.
    Prefix,
    Picker(Picker),
}

/// How often to redraw while a tab is working, so its spinner moves.
const SPINNER_TICK: Duration = Duration::from_millis(120);

struct App {
    spaces: Workspaces<Pane>,
    images: ImageBridge,
    tx: Sender<AppEvent>,
    program: String,
    /// `--settings` JSON with our status hooks; only for Claude, and only
    /// when the status socket is up.
    hook_settings: Option<String>,
    status_server: Option<StatusServer>,
    started: Instant,
    launch_dir: PathBuf,
    next_id: usize,
    cell: CellPx,
    /// Where panes are drawn; every pane is kept at this size.
    pane_area: Rect,
    show_sidebar: bool,
    host_focused: bool,
    mode: Mode,
    hits: Hits,
    quit: bool,
    cursor_style: Option<CursorState>,
}

pub fn run(term: &mut DefaultTerminal, launch: Launch) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    event::spawn_input_reader(tx.clone());

    let show_sidebar = true;
    let areas = ui::layout(term.size()?.into(), show_sidebar);
    // Without the socket, tabs still work; they just have no status.
    let status_server = StatusServer::start(tx.clone()).ok();
    let is_claude = Path::new(&launch.program).file_name().is_some_and(|n| n == "claude");
    let hook_settings = match (&status_server, std::env::current_exe()) {
        (Some(_), Ok(exe)) if is_claude => Some(status::hook_settings(&exe)),
        _ => None,
    };
    let mut app = App {
        spaces: Workspaces::new(),
        images: ImageBridge::new(),
        tx,
        program: launch.program,
        hook_settings,
        status_server,
        started: Instant::now(),
        launch_dir: launch.cwd.clone(),
        next_id: 0,
        cell: CellPx::query(),
        pane_area: areas.pane,
        show_sidebar,
        host_focused: true,
        mode: Mode::Normal,
        hits: Hits::default(),
        quit: false,
        cursor_style: None,
    };
    app.open_workspace(launch.cwd, &launch.args)?;

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

fn build_view<'m>(spaces: &Workspaces<Pane>, mode: &'m Mode, frame: usize) -> View<'m> {
    View {
        spinner_frame: frame,
        workspaces: spaces
            .list
            .iter()
            .map(|ws| WorkspaceView {
                name: ws.name.clone(),
                tabs: ws
                    .tabs
                    .iter()
                    .map(|p| TabView {
                        title: p.title().unwrap_or_else(|| "claude".into()),
                        status: p.status,
                    })
                    .collect(),
                active_tab: ws.active,
            })
            .collect(),
        active: spaces.active,
        mode: match mode {
            Mode::Normal => ui::Mode::Normal,
            Mode::Prefix => ui::Mode::Prefix,
            Mode::Picker(p) => ui::Mode::Picker(p),
        },
    }
}

impl App {
    fn spawn_pane(&mut self, cwd: &Path, args: &[String]) -> Result<Pane> {
        let id = self.next_id;
        self.next_id += 1;
        let mut full_args = Vec::new();
        if let Some(settings) = &self.hook_settings {
            full_args.extend(["--settings".to_owned(), settings.clone()]);
        }
        full_args.extend_from_slice(args);
        let mut env = vec![(status::PANE_ENV, id.to_string())];
        if let Some(server) = &self.status_server {
            env.push((status::SOCKET_ENV, server.path().to_string_lossy().into_owned()));
        }
        Pane::spawn(id, &self.program, &full_args, &env, cwd, self.pane_area, self.cell, self.tx.clone())
    }

    /// Switches to the workspace for `path`, starting one if it isn't open.
    fn open_workspace(&mut self, path: PathBuf, args: &[String]) -> Result<()> {
        if let Some(i) = self.spaces.list.iter().position(|ws| ws.path == path) {
            return self.switch(|s| s.select_workspace(i));
        }
        let pane = self.spawn_pane(&path, args)?;
        self.switch(|s| s.add_workspace(path, pane))
    }

    fn new_tab(&mut self) -> Result<()> {
        let Some(path) = self.spaces.current().map(|ws| ws.path.clone()) else {
            return Ok(());
        };
        let pane = self.spawn_pane(&path, &[])?;
        self.switch(|s| s.add_tab(pane))
    }

    fn close_active_tab(&mut self) -> Result<()> {
        let id = self.active_id();
        self.switch(|s| {
            s.remove_active_tab();
        })?;
        if let Some(id) = id {
            self.forget_images(id)?;
        }
        Ok(())
    }

    /// A pane's program exited, so its tab goes away.
    fn pane_exited(&mut self, id: usize) -> Result<()> {
        self.switch(|s| {
            s.remove_tab(|p| p.id() == id);
        })?;
        self.forget_images(id)
    }

    fn forget_images(&mut self, id: usize) -> Result<()> {
        let mut out = Vec::new();
        self.images.forget(id, &mut out);
        write_host(&out)?;
        if self.spaces.is_empty() {
            self.quit = true;
        }
        Ok(())
    }

    fn active_id(&self) -> Option<usize> {
        self.spaces.active_tab().map(Pane::id)
    }

    /// Runs a change to the workspaces and moves focus reporting along with
    /// the active pane.
    fn switch(&mut self, change: impl FnOnce(&mut Workspaces<Pane>)) -> Result<()> {
        let before = self.active_id();
        change(&mut self.spaces);
        let after = self.active_id();
        if let Some(pane) = self.spaces.active_tab_mut() {
            pane.status = pane.status.seen();
        }
        if before != after && self.host_focused {
            if let Some(old) = before.and_then(|id| self.spaces.find_mut(|p| p.id() == id)) {
                old.focus(false)?;
            }
            if let Some(new) = self.spaces.active_tab_mut() {
                new.focus(true)?;
            }
        }
        Ok(())
    }

    fn draw(&mut self, term: &mut DefaultTerminal) -> Result<()> {
        let areas = ui::layout(term.size()?.into(), self.show_sidebar);
        self.pane_area = areas.pane;
        // Hidden panes keep the same size, so switching tabs never reflows.
        for pane in self.spaces.tabs_mut() {
            pane.set_area(areas.pane, self.cell)?;
        }

        // Images have to exist on the host before the cells that show them.
        let mut out = Vec::new();
        if let Some(pane) = self.spaces.active_tab_mut() {
            let id = pane.id();
            self.images.sync(id, pane.emulator_mut(), &mut out)?;
        }
        write_host(&out)?;

        let frame = (self.started.elapsed().as_millis() / SPINNER_TICK.as_millis()) as usize;
        let view = build_view(&self.spaces, &self.mode, frame);
        let images = &self.images;
        let spaces = &mut self.spaces;
        let normal = matches!(self.mode, Mode::Normal);
        let mut hits = Hits::default();
        let mut pane_cursor = Ok(None);
        term.draw(|frame| {
            if let Some(pane) = spaces.active_tab_mut() {
                let id = pane.id();
                let area = pane.area();
                pane_cursor = pane
                    .render(frame.buffer_mut(), |img| images.remap(id, img))
                    .map(|c| c.map(|c| (c, area)));
            }
            let (chrome_hits, picker_cursor) = ui::draw_chrome(frame, &areas, &view);
            hits = chrome_hits;
            if let Some(pos) = picker_cursor {
                frame.set_cursor_position(pos);
            } else if let Ok(Some((c, area))) = &pane_cursor
                && normal
            {
                frame.set_cursor_position((area.x + c.position.x, area.y + c.position.y));
            }
        })?;
        self.hits = hits;

        // Old animation frames go only once the new cells are on screen.
        let mut out = Vec::new();
        self.images.after_draw(&mut out);
        write_host(&out)?;

        if let Some((c, _)) = pane_cursor?
            && self
                .cursor_style
                .is_none_or(|old| (old.style, old.blinking) != (c.style, c.blinking))
        {
            host::apply_cursor_style(&c)?;
            self.cursor_style = Some(c);
        }
        Ok(())
    }

    /// Blocks for the next event, then drains whatever else is queued so a
    /// burst of output becomes one redraw.
    fn wait(&mut self, rx: &Receiver<AppEvent>) -> Result<()> {
        let working = self.spaces.list.iter().flat_map(|ws| &ws.tabs).any(|p| p.status == Status::Working);
        let ev = if working {
            match rx.recv_timeout(SPINNER_TICK) {
                Ok(ev) => ev,
                Err(RecvTimeoutError::Timeout) => return Ok(()),
                Err(RecvTimeoutError::Disconnected) => return Err(RecvTimeoutError::Disconnected.into()),
            }
        } else {
            rx.recv()?
        };
        self.handle(ev)?;
        while let Ok(ev) = rx.try_recv() {
            self.handle(ev)?;
        }
        Ok(())
    }

    fn handle(&mut self, ev: AppEvent) -> Result<()> {
        match ev {
            AppEvent::PtyOutput(id, bytes) => {
                if let Some(pane) = self.spaces.find_mut(|p| p.id() == id) {
                    pane.feed(&bytes);
                }
            }
            AppEvent::PtyExited(id) => self.pane_exited(id)?,
            AppEvent::Status(id, status) => {
                // No point flagging news on the tab you're looking at.
                let watching = self.host_focused && self.active_id() == Some(id);
                if let Some(pane) = self.spaces.find_mut(|p| p.id() == id) {
                    pane.status = if watching { status.seen() } else { status };
                }
            }
            AppEvent::Input(input) => match input {
                Event::Key(k) => self.key(k)?,
                Event::Mouse(m) => self.mouse(m)?,
                Event::Paste(text) => self.paste(&text)?,
                Event::FocusGained | Event::FocusLost => {
                    self.host_focused = matches!(input, Event::FocusGained);
                    if let Some(pane) = self.spaces.active_tab_mut() {
                        pane.focus(self.host_focused)?;
                    }
                }
                Event::Resize(..) => self.cell = CellPx::query(),
            },
        }
        Ok(())
    }
}
