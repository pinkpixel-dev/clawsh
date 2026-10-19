//! How recently each tab did something: idle tabs cool down over time, and
//! the sidebar can list the busiest work first (`ctrl+b a`).

use std::cmp::Reverse;
use std::time::Duration;

use ratatui::style::{Modifier, Style};

use super::{TabView, View};
use crate::status::Status;
use crate::theme;

/// Under this long since the last turn, an idle tab still reads as just stopped.
const FRESH: Duration = Duration::from_secs(15 * 60);
/// Past this long, the whole row dims.
const STALE: Duration = Duration::from_secs(2 * 60 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Freshness {
    Fresh,
    Plain,
    Stale,
}

/// Only idle tabs age; anything else is happening now.
pub fn freshness(tab: &TabView) -> Freshness {
    match (tab.status, tab.idle_for) {
        (Status::Idle, Some(age)) if age >= STALE => Freshness::Stale,
        (Status::Idle, Some(age)) if age >= FRESH => Freshness::Plain,
        (Status::Idle, None) => Freshness::Plain,
        _ => Freshness::Fresh,
    }
}

/// A tab's title color: the status color while something is happening,
/// then cooler the longer it has been idle. Bold for the tab you're on.
pub fn title_style(tab: &TabView, current: bool) -> Style {
    let p = theme::palette();
    let style = match tab.status {
        Status::Working => Style::new().fg(p.accent),
        Status::NeedsInput => Style::new().fg(p.warn),
        Status::Done => Style::new().fg(p.ok),
        Status::Failed => Style::new().fg(p.danger),
        Status::Idle => match freshness(tab) {
            Freshness::Fresh => theme::panel(),
            Freshness::Plain => theme::muted(),
            Freshness::Stale => theme::faint(),
        },
    };
    if current { style.add_modifier(Modifier::BOLD) } else { style }
}

/// Sorts by what's happening (needs input first), then by how recently.
pub(super) fn tab_key(tab: &TabView) -> (u8, Reverse<Duration>) {
    (tab.status.urgency(), Reverse(tab.idle_for.unwrap_or(Duration::MAX)))
}

/// Workspace and tab indices in sidebar order. Open order unless the view
/// asks for activity order, which only changes the sidebar: indices (and so
/// `ctrl+b 1`-`9`) stay as they are.
pub fn order(view: &View) -> Vec<(usize, Vec<usize>)> {
    let mut list: Vec<(usize, Vec<usize>)> = view
        .workspaces
        .iter()
        .enumerate()
        .map(|(wi, ws)| (wi, (0..ws.tabs.len()).collect()))
        .collect();
    if !view.by_activity {
        return list;
    }
    for (wi, tabs) in &mut list {
        let ws = &view.workspaces[*wi];
        tabs.sort_by_key(|&ti| Reverse(tab_key(&ws.tabs[ti])));
    }
    // Stable, so workspaces that tie keep their open order.
    list.sort_by_key(|(wi, _)| {
        Reverse(view.workspaces[*wi].tabs.iter().map(tab_key).max())
    });
    list
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{Mode, WorkspaceView};

    fn tab(status: Status, mins: Option<u64>) -> TabView {
        TabView {
            id: 0,
            title: String::new(),
            status,
            idle_for: mins.map(|m| Duration::from_secs(m * 60)),
        }
    }

    #[test]
    fn idle_tabs_cool_down() {
        assert_eq!(freshness(&tab(Status::Idle, Some(5))), Freshness::Fresh);
        assert_eq!(freshness(&tab(Status::Idle, Some(30))), Freshness::Plain);
        assert_eq!(freshness(&tab(Status::Idle, Some(180))), Freshness::Stale);
        assert_eq!(freshness(&tab(Status::Idle, None)), Freshness::Plain);
        // Something waiting on you never fades, however old.
        assert_eq!(freshness(&tab(Status::NeedsInput, Some(600))), Freshness::Fresh);
    }

    fn view(by_activity: bool) -> View<'static> {
        let ws = |tabs| WorkspaceView { name: String::new(), head: None, tabs, active_tab: 0 };
        View {
            notice: None,
            spinner_frame: 0,
            usage: None,
            now: chrono::Utc::now(),
            workspaces: vec![
                ws(vec![tab(Status::Idle, Some(300))]),
                ws(vec![tab(Status::Idle, Some(40)), tab(Status::Working, Some(0))]),
                ws(vec![tab(Status::Idle, Some(2)), tab(Status::NeedsInput, Some(9))]),
                ws(vec![tab(Status::Idle, Some(300))]),
            ],
            active: 0,
            mode: Mode::Normal,
            sidebar_scroll: 0,
            follow_active: false,
            notifications: true,
            by_activity,
            terminal: None,
            divider: None,
        }
    }

    #[test]
    fn open_order_is_left_alone() {
        let order = order(&view(false));
        assert_eq!(order, vec![(0, vec![0]), (1, vec![0, 1]), (2, vec![0, 1]), (3, vec![0])]);
    }

    #[test]
    fn activity_order_puts_whatever_needs_you_first() {
        let order = order(&view(true));
        // Needs input, then working, then the two stale ones in open order.
        assert_eq!(order, vec![(2, vec![1, 0]), (1, vec![1, 0]), (0, vec![0]), (3, vec![0])]);
    }
}
