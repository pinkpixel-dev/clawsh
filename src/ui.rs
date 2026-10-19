use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::theme;

pub struct Areas {
    pub pane: Rect,
    pub footer: Rect,
}

pub fn layout(screen: Rect) -> Areas {
    let [pane, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(screen);
    Areas { pane, footer }
}

pub struct FooterState<'a> {
    pub title: Option<&'a str>,
    pub prefix: bool,
    pub exited: bool,
}

pub fn draw_footer(frame: &mut Frame, area: Rect, state: &FooterState) {
    let mut left = vec![Span::styled(" Claw$h ", theme::accent())];
    if let Some(title) = state.title {
        left.push(Span::raw(title.to_owned()));
    }
    if state.exited {
        left.push(Span::styled("  exited", theme::warn()));
    }

    let hints: &[(&str, &str)] = if state.prefix {
        &[("q", "quit"), ("b", "send ctrl+b"), ("esc", "cancel")]
    } else {
        &[("ctrl+b", "commands")]
    };
    let mut right = Vec::new();
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
