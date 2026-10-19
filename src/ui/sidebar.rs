use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};

use super::{Click, Hits, View, status_icon, truncate, usage};
use crate::status::Status;
use crate::theme;

pub fn draw(frame: &mut Frame, area: Rect, view: &View, hits: &mut Hits) {
    let block = Block::new()
        .borders(Borders::RIGHT)
        .border_style(theme::border())
        .style(theme::panel());
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let width = inner.width as usize;
    let mut y = inner.y;
    // Usage owns the bottom rows; the workspace list stops a row above it.
    let usage_rows = if view.usage.is_some() { usage::HEIGHT + 1 } else { 0 };
    let bottom = inner.bottom().saturating_sub(usage_rows);
    let buf = frame.buffer_mut();

    buf.set_line(inner.x, y, &Line::from(Span::styled(" Claw$h", theme::accent())), inner.width);
    y += 1;
    // Up top, next to the title, so it's easy to find.
    let new_row = Rect::new(inner.x, y, inner.width, 1);
    buf.set_line(inner.x, y, &Line::from(Span::styled(" + new workspace", theme::muted())), inner.width);
    hits.push(new_row, Click::NewWorkspace);
    y += 2;

    for (wi, ws) in view.workspaces.iter().enumerate() {
        if y >= bottom {
            break;
        }
        let active = wi == view.active;
        let row = Rect::new(inner.x, y, inner.width, 1);
        let (marker, style) = if active {
            (Span::styled(" ▸ ", theme::accent()), theme::active_row().add_modifier(Modifier::BOLD))
        } else {
            (Span::raw("   "), theme::panel())
        };
        let name = Span::styled(truncate(&ws.name, width.saturating_sub(5)), style);
        buf.set_style(row, if active { theme::active_row() } else { theme::panel() });
        buf.set_line(inner.x, y, &Line::from(vec![marker, name]), inner.width);
        // The workspace's most urgent status, right-aligned, so a collapsed
        // glance at the sidebar still shows which project wants you.
        let summary = ws.status();
        if summary != Status::Idle && inner.width > 2 {
            let icon = Line::from(status_icon(summary, view.spinner_frame));
            buf.set_line(inner.right() - 2, y, &icon, 1);
        }
        hits.push(row, Click::Workspace(wi));
        y += 1;

        for (ti, tab) in ws.tabs.iter().enumerate() {
            if y >= bottom {
                break;
            }
            let current = active && ti == ws.active_tab;
            let style = if current {
                theme::panel().add_modifier(Modifier::BOLD)
            } else {
                theme::muted()
            };
            let line = Line::from(vec![
                Span::raw("   "),
                status_icon(tab.status, view.spinner_frame),
                Span::raw(" "),
                Span::styled(truncate(&tab.title, width.saturating_sub(5)), style),
            ]);
            buf.set_line(inner.x, y, &line, inner.width);
            hits.push(Rect::new(inner.x, y, inner.width, 1), Click::Tab(wi, ti));
            y += 1;
        }
        y += 1;
    }

    if view.usage.is_some() && bottom > inner.y + 3 {
        let area = Rect::new(inner.x, bottom + 1, inner.width, usage::HEIGHT);
        usage::draw(buf, area, view.usage, view.now);
    }
}
