use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear};

use super::{Click, Hits, truncate};
use crate::picker::{Kind, Picker};
use crate::theme;

const WIDTH: u16 = 72;
const HEIGHT: u16 = 18;

/// Draws the picker over everything else and returns where the text cursor
/// belongs.
pub fn draw(frame: &mut Frame, picker: &Picker, hits: &mut Hits) -> Position {
    let screen = frame.area();
    let w = WIDTH.min(screen.width.saturating_sub(4));
    let h = HEIGHT.min(screen.height.saturating_sub(4));
    let area = Rect::new(
        screen.x + (screen.width - w) / 2,
        screen.y + (screen.height - h) / 3,
        w,
        h,
    );

    let (title, empty) = match picker.kind {
        Kind::Folders => (" Open workspace ", "no matching folders, type a path to open any folder"),
        Kind::Templates => (" Open template ", "no templates yet, save one with ctrl+b T"),
        Kind::Snippets => (" Insert snippet ", "no snippets yet, add .md files to ~/.config/clawsh/snippets"),
    };
    let block = Block::bordered()
        .title(Span::styled(title, theme::accent()))
        .border_style(theme::border())
        .style(theme::panel());
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    hits.push(area, Click::PickerBackground);

    let buf = frame.buffer_mut();
    let prompt = Line::from(vec![
        Span::styled(" › ", theme::accent()),
        Span::raw(picker.query.clone()),
    ]);
    buf.set_line(inner.x, inner.y, &prompt, inner.width);
    let cursor = Position::new(
        (inner.x + 3 + picker.query.chars().count() as u16).min(inner.right().saturating_sub(1)),
        inner.y,
    );

    let list_top = inner.y + 2;
    let rows = inner.bottom().saturating_sub(list_top) as usize;
    if let Some(dir) = picker.typed_dir() {
        let line = Line::from(vec![
            Span::styled(" open ", theme::faint()),
            Span::styled(dir.display().to_string(), theme::active_row()),
        ]);
        buf.set_style(Rect::new(inner.x, list_top, inner.width, 1), theme::active_row());
        buf.set_line(inner.x, list_top, &line, inner.width);
        return cursor;
    }

    // Keep the selection on screen.
    let first = picker.selected.saturating_sub(rows.saturating_sub(1));
    let name_width = 24;
    for (row, (i, entry)) in picker.items().enumerate().skip(first).take(rows).enumerate() {
        let y = list_top + row as u16;
        let selected = i == picker.selected;
        let row_rect = Rect::new(inner.x, y, inner.width, 1);
        let base = if selected { theme::active_row() } else { theme::panel() };
        buf.set_style(row_rect, base);
        let line = Line::from(vec![
            Span::styled(if selected { " › " } else { "   " }, theme::accent()),
            Span::styled(format!("{:<name_width$}", truncate(&entry.name, name_width)), base),
            Span::styled(format!(" {}", entry.detail), theme::faint()),
        ]);
        buf.set_line(inner.x, y, &line, inner.width);
        hits.push(row_rect, Click::PickerItem(i));
    }
    if picker.items().next().is_none() {
        let line = Line::from(Span::styled(format!("   {empty}"), theme::faint()));
        buf.set_line(inner.x, list_top, &line, inner.width);
    }
    cursor
}
