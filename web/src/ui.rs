//! Paints the screen Claw$h sent, plus a status line while the connection
//! is down.

use ratzilla::ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Clear, Paragraph},
};

use crate::app::{App, Link};

const BG: Color = Color::Rgb(0x17, 0x18, 0x1a);
const FG: Color = Color::Rgb(0xed, 0xed, 0xed);
const ACCENT: Color = Color::Rgb(0xf9, 0x25, 0x71);

pub fn draw(frame: &mut Frame, app: &App) {
    let area = frame.area();
    let buf = frame.buffer_mut();
    buf.set_style(area, Style::new().bg(BG).fg(FG));
    let shared = area.intersection(app.screen.area);
    for y in shared.top()..shared.bottom() {
        for x in shared.left()..shared.right() {
            buf[(x, y)] = app.screen[(x, y)].clone();
        }
    }
    if let Some((x, y)) = app.cursor
        && shared.contains((x, y).into())
    {
        let cell = &mut buf[(x, y)];
        cell.modifier.toggle(Modifier::REVERSED);
    }

    let status = match app.link {
        Link::Open => return,
        Link::Connecting => " Connecting to Claw$h… ",
        Link::Lost => " Disconnected, retrying… ",
    };
    let row = Rect::new(area.x, area.bottom().saturating_sub(1), area.width, 1.min(area.height));
    frame.render_widget(Clear, row);
    frame.render_widget(
        Paragraph::new(Line::styled(status, Style::new().fg(BG).bg(ACCENT).add_modifier(Modifier::BOLD))),
        row,
    );
}
