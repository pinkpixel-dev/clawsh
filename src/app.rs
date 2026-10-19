mod input;
mod session;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant, SystemTime};

use anyhow::Result;
use crossterm::event::Event;
use ratatui::DefaultTerminal;
use ratatui::layout::Rect;

use crate::event::AppEvent;
use crate::graphics::ImageBridge;
use crate::host::{self, CellPx};
use crate::host_colors::HostColors;
use crate::pane::{CursorState, Pane};
use crate::notify;
use crate::persist::{self, Saved};
use crate::picker::Picker;
use crate::status::{self, Status, StatusServer};
use crate::theme;
use crate::ui::{self, Hits, TabView, View, WorkspaceView};
use crate::usage::{Report, UsagePoller};
use crate::workspace::Workspaces;

pub struct Launch {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    /// Skip restoring the last session.
    pub fresh: bool,
}

enum Mode {
    Normal,
    /// `ctrl+b` was pressed; the next key is a command.
    Prefix,
    Picker(Picker),
    /// The active pane is in copy mode; keys move its cursor.
    Copy,
    /// Typing a new name for the active tab (`ctrl+b ,`).
    Rename(String),
}

/// How often to redraw while a tab is working, so its spinner moves.
const SPINNER_TICK: Duration = Duration::from_millis(120);
/// How often to redraw otherwise, so idle tabs fade as they age.
const IDLE_TICK: Duration = Duration::from_secs(60);

struct App {
    spaces: Workspaces<Pane>,
    images: ImageBridge,
    tx: Sender<AppEvent>,
    program: String,
    /// `--settings` JSON with our status hooks; only for Claude, and only
    /// when the status socket is up.
    hook_settings: Option<String>,
    status_server: Option<StatusServer>,
    /// Only runs when the program is Claude.
    usage_poller: Option<UsagePoller>,
    usage: Option<Report>,
    /// Where the session is saved; only set when the program is Claude, so
    /// test runs with another program never touch it.
    session_file: Option<PathBuf>,
    /// Desktop notifications for background tabs (`ctrl+b m`).
    notifications: bool,
    /// Sidebar busiest-first instead of open order (`ctrl+b a`).
    by_activity: bool,
    last_saved: Option<Saved>,
    started: Instant,
    launch_dir: PathBuf,
    next_id: usize,
    cell: CellPx,
    /// The host terminal's colors, handed to every pane.
    colors: HostColors,
    /// Where panes are drawn; every pane is kept at this size.
    pane_area: Rect,
    show_sidebar: bool,
    host_focused: bool,
    mode: Mode,
    hits: Hits,
    /// Rows scrolled off the top of the sidebar's workspace list.
    sidebar_scroll: usize,
    /// The (workspace, tab) the sidebar last scrolled to show.
    sidebar_followed: Option<(usize, usize)>,
    quit: bool,
    cursor_style: Option<CursorState>,
}

pub fn run(term: &mut DefaultTerminal, launch: Launch, colors: HostColors) -> Result<()> {
    let (tx, rx) = mpsc::channel();
    crate::input::spawn_reader(tx.clone());

    let show_sidebar = true;
    let areas = ui::layout(term.size()?.into(), show_sidebar);
    // Without the socket, tabs still work; they just have no status.
    let status_server = StatusServer::start(tx.clone()).ok();
    let is_claude = Path::new(&launch.program).file_name().is_some_and(|n| n == "claude");
    let hook_settings = match (&status_server, std::env::current_exe()) {
        (Some(_), Ok(exe)) if is_claude => Some(status::hook_settings(&exe)),
        _ => None,
    };
    let usage_poller = is_claude.then(|| UsagePoller::start(tx.clone()));
    let mut app = App {
        spaces: Workspaces::new(),
        images: ImageBridge::new(),
        tx,
        program: launch.program,
        hook_settings,
        status_server,
        usage_poller,
        usage: None,
        session_file: is_claude.then(persist::default_path).flatten(),
        last_saved: None,
        notifications: true,
        by_activity: false,
        started: Instant::now(),
        launch_dir: launch.cwd.clone(),
        next_id: 0,
        cell: CellPx::query(),
        colors,
        pane_area: areas.pane,
        show_sidebar,
        host_focused: true,
        mode: Mode::Normal,
        hits: Hits::default(),
        sidebar_scroll: 0,
        sidebar_followed: None,
        quit: false,
        cursor_style: None,
    };
    let saved = app.session_file.as_deref().and_then(persist::load);
    if let Some(saved) = &saved {
        app.notifications = saved.notifications;
        app.by_activity = saved.by_activity;
        if !launch.fresh {
            app.restore(saved)?;
        }
    }
    app.open_launch_folder(launch.cwd, &launch.args)?;
    app.save_session();

    let result = (|| {
        while !app.quit {
            app.draw(term)?;
            app.wait(&rx)?;
            app.flush_pane_output()?;
            app.save_session();
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

fn build_view<'m>(
    spaces: &Workspaces<Pane>,
    mode: &'m Mode,
    usage: Option<&'m Report>,
    frame: usize,
    (sidebar_scroll, follow_active): (usize, bool),
    notifications: bool,
    by_activity: bool,
) -> View<'m> {
    View {
        notifications,
        by_activity,
        sidebar_scroll,
        follow_active,
        spinner_frame: frame,
        usage,
        now: chrono::Utc::now(),
        workspaces: spaces
            .list
            .iter()
            .map(|ws| WorkspaceView {
                name: ws.name.clone(),
                tabs: ws
                    .tabs
                    .iter()
                    .map(|p| TabView {
                        title: p.title(),
                        status: p.status,
                        idle_for: SystemTime::now().duration_since(p.last_active).ok(),
                    })
                    .collect(),
                active_tab: ws.active,
            })
            .collect(),
        active: spaces.active,
        mode: match mode {
            Mode::Normal => ui::Mode::Normal,
            Mode::Prefix => ui::Mode::Prefix,
            Mode::Copy => ui::Mode::Copy,
            Mode::Rename(text) => ui::Mode::Rename(text),
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
        Pane::spawn(
            id,
            &self.program,
            &full_args,
            &env,
            cwd,
            self.pane_area,
            self.cell,
            self.colors,
            self.tx.clone(),
        )
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

    /// Sends what panes queued for the host terminal, like clipboard writes.
    fn flush_pane_output(&mut self) -> Result<()> {
        let mut out = Vec::new();
        for pane in self.spaces.tabs_mut() {
            out.extend(pane.take_host_output());
        }
        write_host(&out)
    }

    /// Ends copy mode on the active pane, for when something else (a click,
    /// a tab switch) takes over.
    fn leave_copy_mode(&mut self) -> Result<()> {
        if matches!(self.mode, Mode::Copy) {
            self.mode = Mode::Normal;
            if let Some(pane) = self.spaces.active_tab_mut() {
                pane.exit_copy_mode()?;
            }
        }
        Ok(())
    }

    fn active_id(&self) -> Option<usize> {
        self.spaces.active_tab().map(Pane::id)
    }

    /// Runs a change to the workspaces and moves focus reporting along with
    /// the active pane.
    fn switch(&mut self, change: impl FnOnce(&mut Workspaces<Pane>)) -> Result<()> {
        self.leave_copy_mode()?;
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
        // Scroll the sidebar to the active tab only when it changes, so the
        // mouse wheel can still look around in between.
        let active = self.spaces.current().map(|ws| (self.spaces.active, ws.active));
        let follow = active != self.sidebar_followed;
        self.sidebar_followed = active;
        let view = build_view(
            &self.spaces,
            &self.mode,
            self.usage.as_ref(),
            frame,
            (self.sidebar_scroll, follow),
            self.notifications,
            self.by_activity,
        );
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
        self.sidebar_scroll = hits.sidebar_scroll;
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
        let tick = if working { SPINNER_TICK } else { IDLE_TICK };
        let ev = match rx.recv_timeout(tick) {
            Ok(ev) => ev,
            Err(RecvTimeoutError::Timeout) => return Ok(()),
            Err(RecvTimeoutError::Disconnected) => return Err(RecvTimeoutError::Disconnected.into()),
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
                let mut news = None;
                if let Some((ws, pane)) = self.spaces.find_with_workspace_mut(|p| p.id() == id) {
                    let next = if watching { status.seen() } else { status };
                    // One notification per change; hooks often repeat a status.
                    if !watching && next != pane.status {
                        news = notify::message(next, &ws, &pane.title());
                    }
                    pane.status = next;
                    // Idle only comes from SessionStart, which also fires on
                    // a resume; that isn't the tab doing anything.
                    if status != Status::Idle {
                        pane.last_active = SystemTime::now();
                    }
                    // Turns ending are when a new generated title can appear.
                    if matches!(status, Status::Done | Status::NeedsInput) {
                        pane.refresh_transcript_title();
                    }
                }
                if self.notifications
                    && let Some((title, body)) = news
                {
                    notify::send(title, body);
                }
                // A finished turn just spent some of the limit.
                if matches!(status, Status::Done | Status::Failed)
                    && let Some(poller) = &self.usage_poller
                {
                    poller.refresh();
                }
            }
            AppEvent::Session(id, session) => {
                if let Some(pane) = self.spaces.find_mut(|p| p.id() == id) {
                    pane.session = Some(session);
                    pane.refresh_transcript_title();
                }
            }
            AppEvent::Usage(report) => self.usage = Some(report),
            AppEvent::HostScheme(scheme) => {
                theme::follow(scheme);
                // Ask for the exact new colors; the replies come back as
                // HostColor events and get passed on to every pane.
                write_host(b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\")?;
            }
            AppEvent::HostColor(code, rgb) => {
                match code {
                    10 => self.colors.fg = Some(rgb),
                    _ => self.colors.bg = Some(rgb),
                }
                if let Some(scheme) = self.colors.scheme() {
                    theme::follow(scheme);
                }
                for pane in self.spaces.tabs_mut() {
                    pane.set_host_colors(self.colors)?;
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
