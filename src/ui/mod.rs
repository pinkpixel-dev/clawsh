//! Claw$h's own chrome: sidebar, tab bar, footer, and the folder picker.
//! Pane contents are drawn by the panes themselves.

mod footer;
mod picker;
mod sidebar;
mod tabbar;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};

use crate::picker::Picker;

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

/// Clickable regions from the last frame. Later entries sit on top.
#[derive(Default)]
pub struct Hits(Vec<(Rect, Click)>);

impl Hits {
    fn push(&mut self, rect: Rect, click: Click) {
        self.0.push((rect, click));
    }

    pub fn at(&self, x: u16, y: u16) -> Option<Click> {
        self.0
            .iter()
            .rev()
            .find(|(r, _)| r.contains(Position::new(x, y)))
            .map(|&(_, c)| c)
    }
}

pub struct WorkspaceView {
    pub name: String,
    pub tabs: Vec<String>,
    pub active_tab: usize,
}

pub enum Mode<'a> {
    Normal,
    Prefix,
    Picker(&'a Picker),
}

pub struct View<'a> {
    pub workspaces: Vec<WorkspaceView>,
    pub active: usize,
    pub mode: Mode<'a>,
}

/// Draws everything except the pane. Returns the click map and, while the
/// picker is open, where its text cursor goes.
pub fn draw_chrome(frame: &mut Frame, areas: &Areas, view: &View) -> (Hits, Option<Position>) {
    let mut hits = Hits::default();
    if let Some(area) = areas.sidebar {
        sidebar::draw(frame, area, view, &mut hits);
    }
    tabbar::draw(frame, areas.tabbar, view, &mut hits);
    footer::draw(frame, areas.footer, view);
    let cursor = match view.mode {
        Mode::Picker(p) => Some(picker::draw(frame, p, &mut hits)),
        _ => None,
    };
    (hits, cursor)
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
    fn truncates_with_ellipsis() {
        assert_eq!(truncate("claude", 10), "claude");
        assert_eq!(truncate("claude code", 6), "claud…");
    }
}
