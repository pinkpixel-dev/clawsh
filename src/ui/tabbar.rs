use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::{Click, Hits, View, truncate};
use crate::theme;

const MAX_TITLE: usize = 24;

pub fn draw(frame: &mut Frame, area: Rect, view: &View, hits: &mut Hits) {
    let buf = frame.buffer_mut();
    buf.set_style(area, theme::panel());
    let Some(ws) = view.workspaces.get(view.active) else {
        return;
    };

    let right = area.right();
    let mut x = area.x;
    for (ti, title) in ws.tabs.iter().enumerate() {
        let active = ti == ws.active_tab;
        let (num, text) = if active {
            (theme::accent().bg(theme::SURFACE_ACTIVE), theme::active_row())
        } else {
            (theme::faint(), theme::muted())
        };
        let line = Line::from(vec![
            Span::styled(format!(" {} ", ti + 1), num),
            Span::styled(format!("{} ", truncate(title, MAX_TITLE)), text),
        ]);
        let width = line.width() as u16;
        // Leave room for the "+" button.
        if x + width + 3 > right {
            break;
        }
        buf.set_line(x, area.y, &line, width);
        hits.push(Rect::new(x, area.y, width, 1), Click::Tab(view.active, ti));
        x += width + 1;
    }

    if x + 3 <= right {
        buf.set_line(x, area.y, &Line::from(Span::styled(" + ", theme::muted())), 3);
        hits.push(Rect::new(x, area.y, 3, 1), Click::NewTab);
    }
}
