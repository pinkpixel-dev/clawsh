use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};

use super::{Click, Hits, Mode, View, status_icon, truncate};
use crate::theme;

const MAX_TITLE: usize = 24;

/// Draws the tabs. While a rename is open, the active tab shows the text
/// being typed and this returns where the cursor goes.
pub fn draw(frame: &mut Frame, area: Rect, view: &View, hits: &mut Hits) -> Option<Position> {
    let buf = frame.buffer_mut();
    buf.set_style(area, theme::panel());
    let ws = view.workspaces.get(view.active)?;
    let mut cursor = None;

    let right = area.right();
    let mut x = area.x;
    for (ti, tab) in ws.tabs.iter().enumerate() {
        let active = ti == ws.active_tab;
        let (num, text) = if active {
            (theme::accent().bg(theme::palette().surface_active), theme::active_row())
        } else {
            (theme::faint(), theme::muted())
        };
        let editing = match view.mode {
            Mode::Rename(name) if active => Some(tail(name, MAX_TITLE)),
            _ => None,
        };
        let label = match &editing {
            Some(name) => format!(" {name} "),
            None => format!(" {} ", truncate(&tab.title, MAX_TITLE)),
        };
        let lead = Line::from(vec![
            Span::styled(format!(" {} ", ti + 1), num),
            status_icon(tab.status, view.spinner_frame),
        ]);
        let lead_width = lead.width() as u16;
        let line = Line::from([lead.spans, vec![Span::styled(label, text)]].concat());
        let width = line.width() as u16;
        // Leave room for the "+" button.
        if x + width + 3 > right {
            break;
        }
        buf.set_line(x, area.y, &line, width);
        if let Some(name) = &editing {
            // Right after the typed text, past the leading space.
            let at = x + lead_width + 1 + Line::raw(name.as_str()).width() as u16;
            cursor = Some(Position::new(at.min(right.saturating_sub(1)), area.y));
        }
        hits.push(Rect::new(x, area.y, width, 1), Click::Tab(view.active, ti));
        x += width + 1;
    }

    if x + 3 <= right {
        buf.set_line(x, area.y, &Line::from(Span::styled(" + ", theme::muted())), 3);
        hits.push(Rect::new(x, area.y, 3, 1), Click::NewTab);
    }
    cursor
}

/// The last `max` characters of `s`, so the end being typed stays visible.
fn tail(s: &str, max: usize) -> String {
    let count = s.chars().count();
    if count <= max {
        return s.to_owned();
    }
    let mut out = String::from('…');
    out.extend(s.chars().skip(count - max + 1));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_names_show_their_end_while_typing() {
        assert_eq!(tail("short", 10), "short");
        assert_eq!(tail("abcdefghij", 5), "…ghij");
    }
}
