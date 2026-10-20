//! The radar (`ctrl+b v`): every tab in every workspace in one list, with
//! whatever needs you first. Rebuilt each frame, so it stays live while open.

use std::cmp::Reverse;
use std::time::Duration;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Widget};

use super::{Click, Hits, View, activity, status_icon, truncate};
use crate::status::Status;
use crate::theme;

const WIDTH: u16 = 72;
const WORKSPACE_WIDTH: usize = 16;
const AGE_WIDTH: usize = 4;

/// Every tab as (workspace, tab) indices, most urgent first, then the most
/// recently busy. Stable, so ties keep their open order.
pub fn order(view: &View) -> Vec<(usize, usize)> {
    let mut list: Vec<(usize, usize)> = view
        .workspaces
        .iter()
        .enumerate()
        .flat_map(|(wi, ws)| (0..ws.tabs.len()).map(move |ti| (wi, ti)))
        .collect();
    list.sort_by_key(|&(wi, ti)| Reverse(activity::tab_key(&view.workspaces[wi].tabs[ti])));
    list
}

/// Draws the radar over everything else. `selected` is a pane id; when it's
/// missing or gone, the first row is selected. Records the drawn order in
/// `hits.radar` so keys can move through it.
pub fn draw(buf: &mut Buffer, screen: Rect, view: &View, selected: Option<usize>, hits: &mut Hits) {
    let rows = order(view);
    let w = WIDTH.min(screen.width.saturating_sub(4));
    // Borders, the rule, and the summary line, plus at least one row.
    let h = (rows.len().max(1) as u16 + 4).min(screen.height.saturating_sub(4));
    let area = Rect::new(
        screen.x + (screen.width.saturating_sub(w)) / 2,
        screen.y + (screen.height.saturating_sub(h)) / 3,
        w,
        h,
    );

    let block = Block::bordered()
        .title(Span::styled(" Radar ", theme::accent()))
        .border_style(theme::border())
        .style(theme::panel());
    let inner = block.inner(area);
    Clear.render(area, buf);
    block.render(area, buf);
    hits.push(area, Click::RadarBackground);
    hits.radar = rows.iter().map(|&(wi, ti)| view.workspaces[wi].tabs[ti].id).collect();
    if inner.height < 3 {
        return;
    }

    let list_rows = inner.height as usize - 2;
    let at = selected.and_then(|id| hits.radar.iter().position(|&i| i == id)).unwrap_or(0);
    // Keep the selection on screen.
    let first = at.saturating_sub(list_rows.saturating_sub(1));
    // Marker, icon and gap on the left; workspace and age columns on the right.
    let title_width = (inner.width as usize).saturating_sub(3 + 2 + 1 + WORKSPACE_WIDTH + 1 + AGE_WIDTH + 1);
    for (row, &(wi, ti)) in rows.iter().enumerate().skip(first).take(list_rows) {
        let ws = &view.workspaces[wi];
        let tab = &ws.tabs[ti];
        let y = inner.y + (row - first) as u16;
        let picked = row == at;
        let rect = Rect::new(inner.x, y, inner.width, 1);
        let base = if picked { theme::active_row() } else { theme::panel() };
        let current = wi == view.active && ti == ws.active_tab;
        buf.set_style(rect, base);
        let line = Line::from(vec![
            Span::styled(if picked { " › " } else { "   " }, theme::accent()),
            status_icon(tab.status, view.spinner_frame),
            Span::raw(" "),
            Span::styled(
                format!("{:<title_width$}", truncate(&tab.title, title_width)),
                activity::title_style(tab, current),
            ),
            Span::styled(
                format!(" {:<WORKSPACE_WIDTH$}", truncate(&ws.name, WORKSPACE_WIDTH)),
                theme::muted(),
            ),
            Span::styled(format!(" {:>AGE_WIDTH$}", age(tab.idle_for)), theme::faint()),
        ]);
        buf.set_line(inner.x, y, &line, inner.width);
        hits.push(rect, Click::RadarItem(tab.id));
    }
    if rows.is_empty() {
        buf.set_line(inner.x, inner.y, &Line::from(Span::styled("   no tabs", theme::faint())), inner.width);
    }

    // The rule runs across the border too, so it joins the sides.
    let rule_y = inner.bottom() - 2;
    let rule = format!("├{}┤", "─".repeat(inner.width as usize));
    buf.set_line(area.x, rule_y, &Line::from(Span::styled(rule, theme::border())), area.width);
    let summary = Span::styled(format!(" {}", summary(view)), theme::faint());
    buf.set_line(inner.x, rule_y + 1, &Line::from(summary), inner.width);
}

/// How long ago, in the largest whole unit: `now`, `5m`, `3h`, `2d`.
pub(super) fn age(idle_for: Option<Duration>) -> String {
    let Some(secs) = idle_for.map(|d| d.as_secs()) else {
        return String::new();
    };
    match secs {
        0..60 => "now".to_owned(),
        60..3600 => format!("{}m", secs / 60),
        3600..86400 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86400),
    }
}

/// Counts per status, most urgent first, skipping the ones at zero.
fn summary(view: &View) -> String {
    let tabs: Vec<Status> = view.workspaces.iter().flat_map(|ws| &ws.tabs).map(|t| t.status).collect();
    [
        (Status::NeedsInput, "needs input"),
        (Status::Failed, "failed"),
        (Status::Working, "working"),
        (Status::Done, "done"),
        (Status::Idle, "idle"),
    ]
    .into_iter()
    .filter_map(|(status, label)| {
        let n = tabs.iter().filter(|&&s| s == status).count();
        (n > 0).then(|| format!("{n} {label}"))
    })
    .collect::<Vec<_>>()
    .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{Mode, TabView, WorkspaceView};

    fn tab(id: usize, title: &str, status: Status, mins: u64) -> TabView {
        TabView {
            id,
            title: title.into(),
            status,
            idle_for: Some(Duration::from_secs(mins * 60)),
        }
    }

    fn view() -> View<'static> {
        let ws = |name: &str, tabs| WorkspaceView { name: name.into(), head: None, tabs, active_tab: 0 };
        View {
            notice: None,
            spinner_frame: 0,
            usage: None,
            now: chrono::Utc::now(),
            workspaces: vec![
                ws("clawsh", vec![tab(0, "docs", Status::Idle, 180), tab(1, "graphics", Status::Done, 2)]),
                ws("slate", vec![tab(2, "watcher", Status::Working, 0), tab(3, "save logic", Status::NeedsInput, 0)]),
            ],
            active: 0,
            mode: Mode::Normal,
            sidebar_scroll: 0,
            follow_active: false,
            notifications: true,
            by_activity: false,
            terminal: None,
            divider: None,
            web: None,
        }
    }

    fn render(selected: Option<usize>) -> (Buffer, Hits) {
        let screen = Rect::new(0, 0, 80, 20);
        let mut buf = Buffer::empty(screen);
        let mut hits = Hits::default();
        draw(&mut buf, screen, &view(), selected, &mut hits);
        (buf, hits)
    }

    fn row_text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
    }

    #[test]
    fn whatever_needs_you_comes_first() {
        assert_eq!(order(&view()), vec![(1, 1), (1, 0), (0, 1), (0, 0)]);
    }

    #[test]
    fn records_order_and_clickable_rows() {
        let (buf, hits) = render(None);
        assert_eq!(hits.radar, vec![3, 2, 1, 0]);
        // 8 rows tall, a third of the way down a 16 row gap: top border at 4.
        let first = row_text(&buf, 5);
        assert!(first.contains("› ? save logic"), "{first}");
        assert!(first.contains("slate") && first.contains("now"));
        assert!(row_text(&buf, 8).contains("clawsh") && row_text(&buf, 8).contains("3h"));
        assert!(row_text(&buf, 10).contains("1 needs input · 1 working · 1 done · 1 idle"));
        assert_eq!(hits.at(20, 7), Some(Click::RadarItem(1)));
        assert_eq!(hits.at(20, 10), Some(Click::RadarBackground));
    }

    #[test]
    fn selection_follows_the_pane_id() {
        let (buf, _) = render(Some(1));
        assert!(row_text(&buf, 7).contains("› ● graphics"));
        // A pane that's gone falls back to the first row.
        let (buf, _) = render(Some(99));
        assert!(row_text(&buf, 5).contains("›"));
    }

    #[test]
    fn ages_round_down_to_one_unit() {
        assert_eq!(age(None), "");
        assert_eq!(age(Some(Duration::from_secs(59))), "now");
        assert_eq!(age(Some(Duration::from_secs(5 * 60 + 30))), "5m");
        assert_eq!(age(Some(Duration::from_secs(3 * 3600))), "3h");
        assert_eq!(age(Some(Duration::from_secs(2 * 86400 + 5))), "2d");
    }
}
