//! Draws the fake Claw$h layout. Narrow screens get a one-row tab strip
//! instead of the sidebar.

use ratzilla::ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::App;

const BG: Color = Color::Rgb(0x17, 0x18, 0x1a);
const FG: Color = Color::Rgb(0xed, 0xed, 0xed);
const ACCENT: Color = Color::Rgb(0xf9, 0x25, 0x71);
const DIM: Color = Color::Rgb(0x6b, 0x6e, 0x73);
const LINE: Color = Color::Rgb(0x2a, 0x2c, 0x30);
const GREEN: Color = Color::Rgb(0x81, 0xb3, 0x13);

const NARROW: u16 = 70;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(BG).fg(FG)), area);
    app.hits.clear();

    let main = if area.width < NARROW {
        let [strip, rest] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
        tab_strip(frame, app, strip);
        rest
    } else {
        let [side, rest] = Layout::horizontal([Constraint::Length(22), Constraint::Fill(1)]).areas(area);
        sidebar(frame, app, side);
        rest
    };

    let [pane, events, footer] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(9),
        Constraint::Length(1),
    ])
    .areas(main);
    claude_pane(frame, app, pane);
    event_log(frame, app, events);
    status(frame, app, footer, area);
}

fn tab_strip(frame: &mut Frame, app: &mut App, area: Rect) {
    let mut x = area.x;
    let mut spans = Vec::new();
    for (i, tab) in app.tabs.iter().enumerate() {
        let label = format!(" {} {} ", i + 1, tab.name);
        let width = label.chars().count() as u16;
        app.hits.push((Rect::new(x, area.y, width, 1), i));
        x += width + 1;
        spans.push(Span::styled(label, tab_style(i == app.active)));
        spans.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn sidebar(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = Block::new().borders(Borders::RIGHT).border_style(Style::new().fg(LINE));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = vec![Line::styled(" Claw$h", Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)), Line::raw("")];
    for (i, tab) in app.tabs.iter().enumerate() {
        let row = inner.y + lines.len() as u16;
        app.hits.push((Rect::new(inner.x, row, inner.width, 1), i));
        let badge = if i == 1 { Span::styled(" ●", Style::new().fg(GREEN)) } else { Span::raw("") };
        lines.push(Line::from(vec![Span::styled(format!(" {} {:<12}", i + 1, tab.name), tab_style(i == app.active)), badge]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}

fn tab_style(active: bool) -> Style {
    if active {
        Style::new().fg(BG).bg(ACCENT).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(DIM)
    }
}

fn claude_pane(frame: &mut Frame, app: &App, area: Rect) {
    let tab = &app.tabs[app.active];
    let block = Block::bordered()
        .border_style(Style::new().fg(LINE))
        .title(Span::styled(format!(" claude · {} ", tab.name), Style::new().fg(FG)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let height = inner.height.saturating_sub(1) as usize;
    let end = tab.lines.len().saturating_sub(app.scroll);
    let start = end.saturating_sub(height);
    let mut lines: Vec<Line> = tab.lines[start..end]
        .iter()
        .map(|l| {
            let style = if l.starts_with('●') { Style::new().fg(FG) } else if l.starts_with('>') { Style::new().fg(ACCENT) } else { Style::new().fg(DIM) };
            Line::styled(l.clone(), style)
        })
        .collect();
    while lines.len() < height {
        lines.insert(0, Line::raw(""));
    }
    lines.push(Line::from(vec![
        Span::styled("> ", Style::new().fg(ACCENT)),
        Span::raw(app.input.clone()),
        Span::styled(app.preview.clone(), Style::new().fg(DIM).add_modifier(Modifier::UNDERLINED)),
        Span::styled("▌", Style::new().fg(ACCENT)),
    ]));
    frame.render_widget(Paragraph::new(lines), inner);
}

fn event_log(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().border_style(Style::new().fg(LINE)).title(Span::styled(" raw events ", Style::new().fg(DIM)));
    let lines: Vec<Line> = app.events.iter().map(|e| Line::styled(e.clone(), Style::new().fg(DIM))).collect();
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn status(frame: &mut Frame, app: &App, area: Rect, full: Rect) {
    let flag = |on: bool, label: &'static str| Span::styled(label, if on { Style::new().fg(ACCENT).add_modifier(Modifier::BOLD) } else { Style::new().fg(DIM) });
    let line = Line::from(vec![
        flag(app.ctrl, " ctrl "),
        flag(app.prefix, " prefix "),
        Span::styled(format!(" {}x{} · scroll {} ", full.width, full.height, app.scroll), Style::new().fg(DIM)),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}
