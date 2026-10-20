use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};

use super::{Click, Hits};
use crate::menu::Menu;
use crate::theme;

/// Draws the menu over everything else, next to where it was opened.
pub fn draw(frame: &mut Frame, menu: &Menu, hits: &mut Hits) {
    let label_width = menu.items.iter().map(|i| Line::raw(i.label.as_str()).width()).max().unwrap_or(0);
    let key_width = menu.items.iter().filter_map(|i| i.key).map(str::len).max().unwrap_or(0);
    let row_width = 1 + label_width + if key_width > 0 { 3 + key_width } else { 0 } + 1;
    let title_width = menu.title.as_deref().map_or(0, |t| Line::raw(t).width() + 2);
    let rows = menu.items.len() + menu.items.iter().filter(|i| i.divider).count();
    let area = place(
        menu.anchor,
        (row_width.max(title_width) + 2) as u16,
        (rows + 2) as u16,
        frame.area(),
    );

    let mut block = Block::bordered().border_style(theme::border()).style(theme::panel());
    if let Some(title) = &menu.title {
        block = block.title(Span::styled(title.as_str(), theme::accent()));
    }
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    hits.push(area, Click::MenuBackground);

    let buf = frame.buffer_mut();
    let mut y = inner.y;
    for (i, item) in menu.items.iter().enumerate() {
        if item.divider && y < inner.bottom() {
            // Across the border too, so it joins the sides.
            let rule = format!("├{}┤", "─".repeat(inner.width as usize));
            buf.set_line(area.x, y, &Line::from(Span::styled(rule, theme::border())), area.width);
            y += 1;
        }
        if y >= inner.bottom() {
            break;
        }
        let row = Rect::new(inner.x, y, inner.width, 1);
        let selected = i == menu.selected;
        let base = if selected { theme::active_row() } else { theme::panel() };
        let label = if item.danger { base.fg(theme::palette().danger) } else { base };
        buf.set_style(row, base);
        buf.set_line(row.x, y, &Line::from(Span::styled(format!(" {}", item.label), label)), row.width);
        if let Some(key) = item.key {
            let x = row.right().saturating_sub(key.len() as u16 + 1);
            buf.set_line(x, y, &Line::from(Span::styled(key, base.fg(theme::palette().faint))), key.len() as u16);
        }
        hits.push(row, Click::MenuItem(i));
        y += 1;
    }
}

/// Puts a `w` by `h` box just below `anchor`, or above it when it would run
/// off the bottom, shifted left as needed to stay on screen. The anchor is
/// in the desktop's cells, so on a smaller screen (the phone) it can be off
/// the edge; it's pulled back onto the screen first.
fn place(anchor: (u16, u16), w: u16, h: u16, screen: Rect) -> Rect {
    let w = w.min(screen.width);
    let h = h.min(screen.height);
    let anchor = (anchor.0, anchor.1.min(screen.bottom().saturating_sub(1)));
    let x = anchor.0.min(screen.right().saturating_sub(w)).max(screen.x);
    let below = anchor.1.saturating_add(1);
    let y = if below + h <= screen.bottom() {
        below
    } else {
        anchor.1.saturating_sub(h).max(screen.y)
    };
    Rect::new(x, y, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::new(0, 0, 80, 24);

    #[test]
    fn opens_below_the_click() {
        assert_eq!(place((10, 3), 12, 5, SCREEN), Rect::new(10, 4, 12, 5));
    }

    #[test]
    fn flips_up_near_the_bottom() {
        // From the footer row, the menu sits right on top of it.
        assert_eq!(place((0, 23), 20, 8, SCREEN), Rect::new(0, 15, 20, 8));
    }

    #[test]
    fn shifts_left_at_the_right_edge() {
        assert_eq!(place((75, 0), 12, 5, SCREEN), Rect::new(68, 1, 12, 5));
    }

    #[test]
    fn an_anchor_below_a_small_screen_stays_on_it() {
        // Opened from row 37 at the desk, drawn on a 50 by 20 phone.
        let phone = Rect::new(0, 0, 50, 20);
        let r = place((0, 37), 20, 8, phone);
        assert_eq!(r, Rect::new(0, 11, 20, 8));
        assert!(r.bottom() <= phone.bottom());
    }

    #[test]
    fn never_bigger_than_the_screen() {
        let r = place((5, 5), 200, 100, Rect::new(0, 0, 30, 10));
        assert_eq!(r, Rect::new(0, 0, 30, 10));
    }
}
