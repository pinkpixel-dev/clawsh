//! Where the terminal drawer goes, and the divider between it and Claude.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::{Click, Hits};
use crate::theme;

/// How much of the pane area a new drawer gets.
pub const DEFAULT_PERCENT: u16 = 35;
const MIN_PERCENT: u16 = 15;
const MAX_PERCENT: u16 = 85;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
    /// Claude keeps its full width, which is what its mods are laid out for.
    #[default]
    Bottom,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Split {
    pub main: Rect,
    pub divider: Rect,
    pub drawer: Rect,
}

/// What the divider needs to draw itself.
pub struct DividerView {
    pub rect: Rect,
    pub side: Side,
    /// The drawer has the keyboard, so the divider is in the accent color.
    pub focused: bool,
}

/// Splits `area` between Claude and a drawer taking about `percent` of it.
/// `None` when the area is too small for both to be usable.
pub fn split(area: Rect, side: Side, percent: u16) -> Option<Split> {
    let percent = percent.clamp(MIN_PERCENT, MAX_PERCENT);
    match side {
        Side::Bottom => {
            // At least 3 rows of Claude, the divider, and 2 rows of shell.
            let total = area.height;
            if total < 6 {
                return None;
            }
            let drawer = (total * percent / 100).clamp(2, total - 4);
            let main = total - drawer - 1;
            Some(Split {
                main: Rect { height: main, ..area },
                divider: Rect::new(area.x, area.y + main, area.width, 1),
                drawer: Rect::new(area.x, area.y + main + 1, area.width, drawer),
            })
        }
        Side::Right => {
            // At least 20 columns of Claude, the divider, and 10 of shell.
            let total = area.width;
            if total < 31 {
                return None;
            }
            let drawer = (total * percent / 100).clamp(10, total - 21);
            let main = total - drawer - 1;
            Some(Split {
                main: Rect { width: main, ..area },
                divider: Rect::new(area.x + main, area.y, 1, area.height),
                drawer: Rect::new(area.x + main + 1, area.y, drawer, area.height),
            })
        }
    }
}

/// The drawer size for a divider dragged to (`col`, `row`).
pub fn percent_at(area: Rect, side: Side, col: u16, row: u16) -> u16 {
    let (cells, total) = match side {
        Side::Bottom => (area.bottom().saturating_sub(row.saturating_add(1)), area.height),
        Side::Right => (area.right().saturating_sub(col.saturating_add(1)), area.width),
    };
    if total == 0 {
        return DEFAULT_PERCENT;
    }
    (cells as u32 * 100 / total as u32).clamp(MIN_PERCENT as u32, MAX_PERCENT as u32) as u16
}

pub fn draw_divider(buf: &mut Buffer, divider: &DividerView, hits: &mut Hits) {
    let style = if divider.focused { theme::accent() } else { theme::border() };
    let r = divider.rect;
    match divider.side {
        Side::Bottom => {
            let label = " terminal ";
            let rule = format!("─{label}{}", "─".repeat((r.width as usize).saturating_sub(label.len() + 1)));
            buf.set_line(r.x, r.y, &Line::from(Span::styled(rule, style)), r.width);
        }
        Side::Right => {
            for y in r.top()..r.bottom() {
                buf.set_line(r.x, y, &Line::from(Span::styled("│", style)), 1);
            }
        }
    }
    hits.push(r, Click::Divider);
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(26, 1, 100, 40);

    #[test]
    fn bottom_keeps_claude_full_width() {
        let s = split(AREA, Side::Bottom, 35).unwrap();
        assert_eq!(s.main, Rect::new(26, 1, 100, 25));
        assert_eq!(s.divider, Rect::new(26, 26, 100, 1));
        assert_eq!(s.drawer, Rect::new(26, 27, 100, 14));
        assert_eq!(s.main.height + 1 + s.drawer.height, AREA.height);
    }

    #[test]
    fn right_splits_columns() {
        let s = split(AREA, Side::Right, 40).unwrap();
        assert_eq!(s.main.width + 1 + s.drawer.width, AREA.width);
        assert_eq!(s.drawer.width, 40);
        assert_eq!(s.divider.x, s.main.right());
        assert_eq!(s.drawer.x, s.divider.x + 1);
    }

    #[test]
    fn too_small_means_no_drawer() {
        assert!(split(Rect::new(0, 0, 80, 5), Side::Bottom, 35).is_none());
        assert!(split(Rect::new(0, 0, 30, 40), Side::Right, 35).is_none());
        // Extreme sizes are clamped so both sides stay usable.
        let s = split(Rect::new(0, 0, 80, 6), Side::Bottom, 99).unwrap();
        assert_eq!((s.main.height, s.drawer.height), (3, 2));
    }

    #[test]
    fn dragging_sets_the_percent() {
        // Divider dragged to row 20 leaves rows 21..41 (20 of 40) for the drawer.
        assert_eq!(percent_at(AREA, Side::Bottom, 0, 20), 50);
        assert_eq!(percent_at(AREA, Side::Bottom, 0, 1), MAX_PERCENT);
        assert_eq!(percent_at(AREA, Side::Right, 125, 0), MIN_PERCENT);
    }
}
