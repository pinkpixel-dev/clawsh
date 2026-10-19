use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};

use super::{Click, Hits, View, status_icon, truncate, usage};
use crate::status::Status;
use crate::theme;

/// One line of the scrolling workspace list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Row {
    Workspace(usize),
    Tab(usize, usize),
    Gap,
}

pub fn draw(frame: &mut Frame, area: Rect, view: &View, hits: &mut Hits) {
    let block = Block::new()
        .borders(Borders::RIGHT)
        .border_style(theme::border())
        .style(theme::panel());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let buf = frame.buffer_mut();

    let mut y = inner.y;
    buf.set_line(inner.x, y, &Line::from(Span::styled(" Claw$h", theme::accent())), inner.width);
    y += 1;
    // Up top, next to the title, so it's easy to find.
    let new_row = Rect::new(inner.x, y, inner.width, 1);
    buf.set_line(inner.x, y, &Line::from(Span::styled(" + new workspace", theme::muted())), inner.width);
    hits.push(new_row, Click::NewWorkspace);
    y += 2;

    // Usage owns the bottom rows; the list stops a row above it.
    let usage_rows = if view.usage.is_some() { usage::HEIGHT + 1 } else { 0 };
    let bottom = inner.bottom().saturating_sub(usage_rows).max(y);
    let list = Rect::new(inner.x, y, inner.width, bottom - y);
    draw_list(buf, list, view, hits);

    if view.usage.is_some() && list.height > 0 {
        let area = Rect::new(inner.x, bottom + 1, inner.width, usage::HEIGHT);
        usage::draw(buf, area, view.usage, view.now);
    }
}

/// Workspaces and their tabs, scrolled so the window fits `list`.
fn draw_list(buf: &mut Buffer, list: Rect, view: &View, hits: &mut Hits) {
    let mut rows = Vec::new();
    for (wi, ws) in view.workspaces.iter().enumerate() {
        if wi > 0 {
            rows.push(Row::Gap);
        }
        rows.push(Row::Workspace(wi));
        rows.extend((0..ws.tabs.len()).map(|ti| Row::Tab(wi, ti)));
    }

    let height = list.height as usize;
    let scroll = scroll_for(&rows, height, view);
    hits.sidebar_list = Some(list);
    hits.sidebar_scroll = scroll;

    for (i, row) in rows.iter().skip(scroll).take(height).enumerate() {
        let rect = Rect::new(list.x, list.y + i as u16, list.width, 1);
        match *row {
            Row::Workspace(wi) => workspace_row(buf, rect, view, wi, hits),
            Row::Tab(wi, ti) => tab_row(buf, rect, view, wi, ti, hits),
            Row::Gap => {}
        }
    }

    // Faint arrows at the far edge when there's more above or below.
    let edge = list.right().saturating_sub(1);
    if scroll > 0 && height > 0 {
        buf.set_line(edge, list.y, &Line::from(Span::styled("▲", theme::faint())), 1);
    }
    if scroll + height < rows.len() && height > 0 {
        buf.set_line(edge, list.bottom() - 1, &Line::from(Span::styled("▼", theme::faint())), 1);
    }
}

/// Clamps the requested scroll, and when the active tab just changed, moves
/// just far enough to show it along with its workspace's name.
fn scroll_for(rows: &[Row], height: usize, view: &View) -> usize {
    let max = rows.len().saturating_sub(height);
    let mut scroll = view.sidebar_scroll.min(max);
    if view.follow_active && height > 0 {
        let active_tab = view.workspaces.get(view.active).map_or(0, |ws| ws.active_tab);
        let find = |target: Row| rows.iter().position(|&r| r == target);
        if let (Some(ws_row), Some(tab_row)) = (
            find(Row::Workspace(view.active)),
            find(Row::Tab(view.active, active_tab)),
        ) {
            if tab_row >= scroll + height {
                scroll = tab_row + 1 - height;
            }
            if ws_row < scroll {
                scroll = ws_row.min(tab_row);
            }
        }
    }
    scroll.min(max)
}

fn workspace_row(buf: &mut Buffer, row: Rect, view: &View, wi: usize, hits: &mut Hits) {
    let ws = &view.workspaces[wi];
    let active = wi == view.active;
    let (marker, style) = if active {
        (Span::styled(" ▸ ", theme::accent()), theme::active_row().add_modifier(Modifier::BOLD))
    } else {
        (Span::raw("   "), theme::panel())
    };
    let name = Span::styled(truncate(&ws.name, (row.width as usize).saturating_sub(5)), style);
    buf.set_style(row, if active { theme::active_row() } else { theme::panel() });
    buf.set_line(row.x, row.y, &Line::from(vec![marker, name]), row.width);
    // The workspace's most urgent status, right-aligned, so a quick glance
    // at the sidebar still shows which project wants you.
    let summary = ws.status();
    if summary != Status::Idle && row.width > 2 {
        let icon = Line::from(status_icon(summary, view.spinner_frame));
        buf.set_line(row.right() - 2, row.y, &icon, 1);
    }
    hits.push(row, Click::Workspace(wi));
}

fn tab_row(buf: &mut Buffer, row: Rect, view: &View, wi: usize, ti: usize, hits: &mut Hits) {
    let ws = &view.workspaces[wi];
    let tab = &ws.tabs[ti];
    let current = wi == view.active && ti == ws.active_tab;
    let style = if current {
        theme::panel().add_modifier(Modifier::BOLD)
    } else {
        theme::muted()
    };
    let line = Line::from(vec![
        Span::raw("   "),
        status_icon(tab.status, view.spinner_frame),
        Span::raw(" "),
        Span::styled(truncate(&tab.title, (row.width as usize).saturating_sub(5)), style),
    ]);
    buf.set_line(row.x, row.y, &line, row.width);
    hits.push(row, Click::Tab(wi, ti));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{Mode, TabView, WorkspaceView};

    /// Ten workspaces with two tabs each: 10 * 3 rows + 9 gaps = 39 rows.
    fn view(active: usize, scroll: usize, follow: bool) -> View<'static> {
        let tab = |t: &str| TabView { title: t.into(), status: Status::Idle };
        View {
            spinner_frame: 0,
            usage: None,
            now: chrono::Utc::now(),
            workspaces: (0..10)
                .map(|i| WorkspaceView {
                    name: format!("ws{i}"),
                    tabs: vec![tab("a"), tab("b")],
                    active_tab: 1,
                })
                .collect(),
            active,
            mode: Mode::Normal,
            sidebar_scroll: scroll,
            follow_active: follow,
            notifications: true,
        }
    }

    fn render(v: &View) -> (Buffer, Hits) {
        let area = Rect::new(0, 0, 26, 20);
        let mut buf = Buffer::empty(area);
        let mut hits = Hits::default();
        draw_list(&mut buf, area, v, &mut hits);
        (buf, hits)
    }

    fn row_text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_owned()
    }

    #[test]
    fn follows_the_active_tab_down_the_list() {
        let (buf, hits) = render(&view(8, 0, true));
        // ws8's rows are 32 (name), 33, 34 (active tab "b"); 20 rows fit.
        assert_eq!(hits.sidebar_scroll, 15);
        assert!(row_text(&buf, 19).starts_with("   ○ b"));
        assert!(row_text(&buf, 19).ends_with('▼'), "ws9 is still below");
        assert!(row_text(&buf, 17).contains("ws8"));
        assert!(row_text(&buf, 0).ends_with('▲'));
    }

    #[test]
    fn manual_scroll_sticks_until_the_tab_changes() {
        let (_, hits) = render(&view(0, 7, false));
        assert_eq!(hits.sidebar_scroll, 7, "wheel position is kept");
        let (_, hits) = render(&view(0, 99, false));
        assert_eq!(hits.sidebar_scroll, 19, "clamped to the last full page");
        let (buf, hits) = render(&view(0, 7, true));
        assert_eq!(hits.sidebar_scroll, 0, "switching back to ws0 shows it again");
        assert!(row_text(&buf, 19).ends_with('▼'));
    }

    #[test]
    fn clicks_land_on_the_scrolled_rows() {
        let (_, hits) = render(&view(8, 0, true));
        assert_eq!(hits.at(5, 17), Some(Click::Workspace(8)));
        assert_eq!(hits.at(5, 19), Some(Click::Tab(8, 1)));
    }
}
