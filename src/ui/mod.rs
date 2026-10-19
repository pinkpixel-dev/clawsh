//! Claw$h's own chrome: sidebar, tab bar, footer, and the folder picker.
//! Pane contents are drawn by the panes themselves.

mod footer;
mod picker;
mod sidebar;
mod tabbar;
mod usage;

use chrono::{DateTime, Utc};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use crate::picker::Picker;
use crate::status::Status;
use crate::theme;
use crate::usage::Report;

const SIDEBAR_WIDTH: u16 = 26;
/// Below this width the sidebar hides itself so the pane stays usable.
const MIN_WIDTH_FOR_SIDEBAR: u16 = 70;

pub struct Areas {
    pub sidebar: Option<Rect>,
    pub tabbar: Rect,
    pub pane: Rect,
    pub footer: Rect,
}

pub fn layout(screen: Rect, show_sidebar: bool) -> Areas {
    let [main, footer] =
        Layout::vertical([Constraint::Min(2), Constraint::Length(1)]).areas(screen);
    let (sidebar, right) = if show_sidebar && screen.width >= MIN_WIDTH_FOR_SIDEBAR {
        let [s, r] = Layout::horizontal([Constraint::Length(SIDEBAR_WIDTH), Constraint::Min(1)])
            .areas(main);
        (Some(s), r)
    } else {
        (None, main)
    };
    let [tabbar, pane] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(right);
    Areas {
        sidebar,
        tabbar,
        pane,
        footer,
    }
}

/// Something clickable in the chrome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Workspace(usize),
    Tab(usize, usize),
    NewTab,
    NewWorkspace,
    PickerItem(usize),
    /// Inside the picker, but not on an item.
    PickerBackground,
}

/// Clickable regions from the last frame (later entries sit on top), plus
/// where the sidebar's scrolling list ended up.
#[derive(Default)]
pub struct Hits {
    regions: Vec<(Rect, Click)>,
    /// The part of the sidebar that scrolls, for the mouse wheel.
    pub sidebar_list: Option<Rect>,
    /// The scroll offset the sidebar actually used, after clamping and
    /// following the active tab.
    pub sidebar_scroll: usize,
}

impl Hits {
    fn push(&mut self, rect: Rect, click: Click) {
        self.regions.push((rect, click));
    }

    pub fn at(&self, x: u16, y: u16) -> Option<Click> {
        self.regions
            .iter()
            .rev()
            .find(|(r, _)| r.contains(Position::new(x, y)))
            .map(|&(_, c)| c)
    }
}

pub struct TabView {
    pub title: String,
    pub status: Status,
}

pub struct WorkspaceView {
    pub name: String,
    pub tabs: Vec<TabView>,
    pub active_tab: usize,
}

impl WorkspaceView {
    /// The most urgent status among the workspace's tabs.
    fn status(&self) -> Status {
        self.tabs
            .iter()
            .map(|t| t.status)
            .max_by_key(|s| s.urgency())
            .unwrap_or_default()
    }
}

pub enum Mode<'a> {
    Normal,
    Prefix,
    Picker(&'a Picker),
    Copy,
    /// The active tab's name, being edited.
    Rename(&'a str),
}

pub struct View<'a> {
    /// Advances while anything is working, to animate the spinner.
    pub spinner_frame: usize,
    /// Claude's limits, shown once at the bottom of the sidebar. `None` when
    /// the program isn't Claude or the first reading hasn't arrived.
    pub usage: Option<&'a Report>,
    pub now: DateTime<Utc>,
    pub workspaces: Vec<WorkspaceView>,
    pub active: usize,
    pub mode: Mode<'a>,
    /// Rows scrolled off the top of the sidebar's workspace list.
    pub sidebar_scroll: usize,
    /// The active tab just changed, so the sidebar should scroll to show it.
    pub follow_active: bool,
    /// Desktop notifications on or off, for the footer's prefix hints.
    pub notifications: bool,
}

/// Draws everything except the pane. Returns the click map and, while the
/// picker or a rename is open, where its text cursor goes.
pub fn draw_chrome(frame: &mut Frame, areas: &Areas, view: &View) -> (Hits, Option<Position>) {
    let mut hits = Hits::default();
    if let Some(area) = areas.sidebar {
        sidebar::draw(frame, area, view, &mut hits);
    }
    let rename_cursor = tabbar::draw(frame, areas.tabbar, view, &mut hits);
    footer::draw(frame, areas.footer, view);
    let cursor = match view.mode {
        Mode::Picker(p) => Some(picker::draw(frame, p, &mut hits)),
        _ => rename_cursor,
    };
    (hits, cursor)
}

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// One cell that says what a tab is doing. Shapes differ as well as colors,
/// so status never depends on color alone.
fn status_icon(status: Status, frame: usize) -> Span<'static> {
    let bold = Modifier::BOLD;
    match status {
        Status::Idle => Span::styled("○", theme::faint()),
        Status::Working => Span::styled(SPINNER[frame % SPINNER.len()], Style::new().fg(theme::palette().accent)),
        Status::NeedsInput => Span::styled("?", Style::new().fg(theme::palette().warn).add_modifier(bold)),
        Status::Done => Span::styled("●", Style::new().fg(theme::palette().ok)),
        Status::Failed => Span::styled("✗", Style::new().fg(theme::palette().danger).add_modifier(bold)),
    }
}

/// Cuts `s` to `max` characters, ending with an ellipsis when it was longer.
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidebar_hides_on_narrow_screens() {
        assert!(layout(Rect::new(0, 0, 120, 40), true).sidebar.is_some());
        assert!(layout(Rect::new(0, 0, 60, 40), true).sidebar.is_none());
        assert!(layout(Rect::new(0, 0, 120, 40), false).sidebar.is_none());
    }

    #[test]
    fn pane_gets_what_is_left() {
        let a = layout(Rect::new(0, 0, 120, 40), true);
        assert_eq!(a.pane, Rect::new(SIDEBAR_WIDTH, 1, 120 - SIDEBAR_WIDTH, 38));
        assert_eq!(a.footer, Rect::new(0, 39, 120, 1));
    }

    #[test]
    fn later_hits_win() {
        let mut h = Hits::default();
        h.push(Rect::new(0, 0, 10, 10), Click::NewTab);
        h.push(Rect::new(2, 2, 2, 2), Click::PickerItem(0));
        assert_eq!(h.at(3, 3), Some(Click::PickerItem(0)));
        assert_eq!(h.at(0, 0), Some(Click::NewTab));
        assert_eq!(h.at(20, 20), None);
    }

    #[test]
    fn workspace_shows_its_most_urgent_tab() {
        let tab = |status| TabView { title: String::new(), status };
        let ws = WorkspaceView {
            name: String::new(),
            tabs: vec![tab(Status::Done), tab(Status::NeedsInput), tab(Status::Working)],
            active_tab: 0,
        };
        assert_eq!(ws.status(), Status::NeedsInput);
    }

    #[test]
    fn truncates_with_ellipsis() {
        assert_eq!(truncate("claude", 10), "claude");
        assert_eq!(truncate("claude code", 6), "claud…");
    }
}
