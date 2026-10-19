use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::{Mode, View};
use crate::theme;

pub fn draw(frame: &mut Frame, area: Rect, view: &View) {
    let mut left = vec![Span::styled(" Claw$h ", theme::accent())];
    if let Some(ws) = view.workspaces.get(view.active) {
        left.push(Span::raw(ws.name.clone()));
        if let Some(tab) = ws.tabs.get(ws.active_tab) {
            left.push(Span::styled(" › ", theme::faint()));
            left.push(Span::raw(tab.title.clone()));
        }
    }

    let hints: &[(&str, &str)] = match view.mode {
        Mode::Normal => &[("ctrl+b", "commands")],
        Mode::Prefix => &[
            ("c", "tab"),
            ("w", "workspace"),
            ("x", "close"),
            ("n/p", "tabs"),
            ("j/k", "spaces"),
            ("s", "sidebar"),
            ("q", "quit"),
            ("esc", "cancel"),
        ],
        Mode::Picker(_) => &[("enter", "open"), ("↑↓", "move"), ("esc", "cancel")],
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
}
