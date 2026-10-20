use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::{Click, Hits, Mode, View};
use crate::picker::Kind;
use crate::theme;

/// Draws the footer. While a template or workspace name is being typed, it
/// goes on the left and this returns where the cursor belongs.
pub fn draw(frame: &mut Frame, area: Rect, view: &View, hits: &mut Hits) -> Option<Position> {
    // The name doubles as the menu button.
    let button = Span::styled(" ≡ Claw$h ", theme::accent());
    hits.push(Rect::new(area.x, area.y, (button.width() as u16).min(area.width), 1), Click::OpenMenu);
    let mut left = vec![button];
    // Phone access is on: a dot that fills in while a phone is connected.
    if let Some(web) = &view.web {
        let badge = if web.connected {
            Span::styled("● phone ", theme::accent())
        } else {
            Span::styled("○ phone ", theme::muted())
        };
        let x = area.x + left[0].width() as u16;
        hits.push(Rect::new(x, area.y, (badge.width() as u16).min(area.right().saturating_sub(x)), 1), Click::WebLink);
        left.push(badge);
    }
    let mut cursor = None;
    let field = match view.mode {
        Mode::SaveTemplate(name) => Some(("save template as ", name)),
        Mode::RenameWorkspace(name) => Some(("rename workspace ", name)),
        Mode::NewWorktree(name) => Some(("new worktree branch ", name)),
        _ => None,
    };
    if let Some((prompt, name)) = field {
        left.push(Span::styled(prompt, theme::muted()));
        let before: u16 = left.iter().map(|s| s.width() as u16).sum();
        left.push(Span::raw(name.to_owned()));
        let at = area.x + before + Span::raw(name).width() as u16;
        cursor = Some(Position::new(at.min(area.right().saturating_sub(1)), area.y));
    } else if let Some(notice) = view.notice {
        left.push(Span::raw(notice.to_owned()));
    } else if let Some(ws) = view.workspaces.get(view.active) {
        left.push(Span::raw(ws.name.clone()));
        if let Some(tab) = ws.tabs.get(ws.active_tab) {
            left.push(Span::styled(" › ", theme::faint()));
            left.push(Span::raw(tab.title.clone()));
        }
    }

    let notify = if view.notifications { "mute" } else { "unmute" };
    let order = if view.by_activity { "open order" } else { "by activity" };
    let hints: &[(&str, &str)] = match view.mode {
        Mode::Normal => &[("ctrl+b", "commands")],
        Mode::Prefix => &[
            ("space", "palette"),
            ("c", "tab"),
            ("w", "workspace"),
            ("x", "close"),
            ("n/p", "tabs"),
            ("j/k", "spaces"),
            ("[", "copy"),
            ("r", "review"),
            ("v", "radar"),
            ("/", "search"),
            (",", "rename"),
            ("t/T", "templates"),
            ("i", "snippet"),
            ("f", "file"),
            ("g", "worktrees"),
            ("e", "terminal"),
            ("W", "phone"),
            (";", "focus"),
            ("o", "menu"),
            ("s", "sidebar"),
            ("a", order),
            ("m", notify),
            ("q", "quit"),
            ("esc", "cancel"),
        ],
        Mode::Picker(p) if p.kind == Kind::Worktrees => {
            &[("enter", "open"), ("ctrl+x", "remove"), ("↑↓", "move"), ("esc", "cancel")]
        }
        Mode::Picker(p) if p.kind == Kind::Files => &[("enter", "insert"), ("↑↓", "move"), ("esc", "cancel")],
        Mode::Picker(_) => &[("enter", "open"), ("↑↓", "move"), ("esc", "cancel")],
        Mode::Rename(_) => &[("enter", "save"), ("empty", "auto"), ("esc", "cancel")],
        Mode::SaveTemplate(_) => &[("enter", "save"), ("esc", "cancel")],
        Mode::NewWorktree(_) => &[("enter", "create"), ("esc", "cancel")],
        Mode::RenameWorkspace(_) => &[("enter", "save"), ("empty", "folder name"), ("esc", "cancel")],
        Mode::Menu(_) => &[("enter", "choose"), ("↑↓", "move"), ("esc", "close")],
        Mode::Radar(_) => &[("enter", "jump"), ("r", "review turn"), ("↑↓", "move"), ("esc", "close")],
        Mode::WebLink => &[("r", "watch only"), ("n", "new code"), ("any key", "close")],
        Mode::Sidebar => &[("tap", "switch"), ("esc", "close")],
        Mode::Search(s) if s.stale() => &[("enter", "search"), ("↑↓", "move"), ("esc", "close")],
        Mode::Search(_) => &[("enter", "resume"), ("↑↓", "move"), ("esc", "close")],
        Mode::Review(true) => &[("enter", "save"), ("empty", "delete"), ("esc", "cancel")],
        Mode::Review(false) => &[
            ("u/t", "scope"),
            ("tab", "files/diff"),
            ("v", "select"),
            ("c", "comment"),
            ("n", "next"),
            ("s", "send"),
            ("m", "markdown"),
            ("r", "refresh"),
            ("esc", "close"),
        ],
        Mode::Copy => &[
            ("COPY", ""),
            ("hjkl", "move"),
            ("g/G", "top/end"),
            ("v", "select"),
            ("y", "copy"),
            ("q", "exit"),
        ],
    };
    // Leading gap so a long title never runs into the hints.
    let mut right = vec![Span::raw("  ")];
    for (key, label) in hints {
        right.push(Span::styled(*key, theme::key_hint()));
        right.push(Span::raw(format!(" {label}  ")));
    }

    let right_width = right.iter().map(Span::width).sum::<usize>() as u16;
    let [l, r] =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(right_width)]).areas(area);
    frame.render_widget(Paragraph::new(Line::from(left)).style(theme::bar()), l);
    frame.render_widget(Paragraph::new(Line::from(right)).style(theme::bar()), r);
    cursor
}
