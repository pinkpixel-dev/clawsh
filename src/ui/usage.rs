use chrono::{DateTime, Utc};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use super::truncate;
use crate::theme;
use crate::usage::{Report, Window, until};

/// Rows the usage block takes at the bottom of the sidebar.
pub const HEIGHT: u16 = 2;
const BAR: usize = 8;

/// Draws Claude's limits into `area` (HEIGHT rows). Nothing yet means the
/// first reading is still on its way.
pub fn draw(buf: &mut Buffer, area: Rect, report: Option<&Report>, now: DateTime<Utc>) {
    let mut line = |y: u16, line: Line| {
        buf.set_line(area.x, area.y + y, &line, area.width);
    };
    match report {
        None => line(0, Line::from(Span::styled(" usage …", theme::faint()))),
        Some(Err(reason)) => {
            let text = truncate(&format!(" usage: {reason}"), area.width as usize);
            line(0, Line::from(Span::styled(text, theme::faint())));
        }
        Some(Ok(windows)) => {
            for (y, w) in windows.iter().take(HEIGHT as usize).enumerate() {
                line(y as u16, window_line(w, now));
            }
        }
    }
}

fn window_line(w: &Window, now: DateTime<Utc>) -> Line<'static> {
    let mut spans = vec![Span::styled(format!(" {} ", w.label), theme::muted())];
    match w.left {
        Some(left) => {
            let color = level_color(left);
            let filled = (left as usize * BAR).div_ceil(100).min(BAR);
            spans.push(Span::styled("█".repeat(filled), Style::new().fg(color)));
            spans.push(Span::styled("░".repeat(BAR - filled), theme::faint()));
            spans.push(Span::styled(format!(" {left:>3}%"), Style::new().fg(color)));
        }
        None => spans.push(Span::styled(format!("{:BAR$}  n/a", ""), theme::faint())),
    }
    if let Some(reset) = w.resets_at {
        spans.push(Span::styled(format!(" {}", until(reset, now)), theme::faint()));
    }
    Line::from(spans)
}

/// Green while there's room, yellow under 25% left, red under 10%.
fn level_color(left: u8) -> Color {
    match left {
        0..10 => theme::palette().danger,
        10..25 => theme::palette().warn,
        _ => theme::palette().ok,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn a_window_fits_the_sidebar() {
        let now = Utc.with_ymd_and_hms(2026, 10, 19, 0, 0, 0).unwrap();
        let w = Window {
            label: "5h",
            left: Some(35),
            resets_at: Some(now + chrono::Duration::minutes(200)),
        };
        let line = window_line(&w, now);
        assert_eq!(text(&line), " 5h ███░░░░░  35% 3h 20m");
        assert!(line.width() <= 25, "sidebar is 25 columns inside the border");
    }

    #[test]
    fn color_tracks_how_much_is_left() {
        let p = theme::palette();
        assert_eq!(level_color(80), p.ok);
        assert_eq!(level_color(20), p.warn);
        assert_eq!(level_color(5), p.danger);
    }
}
