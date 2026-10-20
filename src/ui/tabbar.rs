use ratatui::Frame;
use ratatui::layout::{Position, Rect};
use ratatui::text::{Line, Span};

use super::{Click, Hits, Mode, View, status_icon, truncate};
use crate::theme;

const MAX_TITLE: usize = 24;
/// The terminal button at the right end.
const TERMINAL: &str = " >_ ";

/// Draws the tabs. While a rename is open, the active tab shows the text
/// being typed and this returns where the cursor goes.
pub fn draw(frame: &mut Frame, area: Rect, view: &View, hits: &mut Hits) -> Option<Position> {
    let buf = frame.buffer_mut();
    buf.set_style(area, theme::panel());
    let ws = view.workspaces.get(view.active)?;
    let mut cursor = None;

    // The terminal button sits at the far right; tabs stop before it.
    let button = TERMINAL.len() as u16;
    let right = area.right().saturating_sub(button + 1);
    if area.width > button {
        let style = match view.terminal {
            Some(true) => theme::accent(),
            Some(false) => theme::muted(),
            None => theme::faint(),
        };
        let at = area.right() - button;
        buf.set_line(at, area.y, &Line::from(Span::styled(TERMINAL, style)), button);
        hits.push(Rect::new(at, area.y, button, 1), Click::Terminal);
    }
    // Room for the tabs, leaving space for the "+" button.
    let avail = right.saturating_sub(area.x).saturating_sub(3);
    let editing = match view.mode {
        Mode::Rename(name) => Some(tail(name, MAX_TITLE)),
        _ => None,
    };
    let tabs: Vec<Tab> = ws
        .tabs
        .iter()
        .enumerate()
        .map(|(ti, tab)| {
            let active = ti == ws.active_tab;
            let (num, text) = if active {
                (theme::accent().bg(theme::palette().surface_active), theme::active_row())
            } else {
                (theme::faint(), theme::muted())
            };
            let label = match &editing {
                Some(name) if active => format!(" {name} "),
                _ => format!(" {} ", truncate(&tab.title, MAX_TITLE)),
            };
            let icon = status_icon(tab.status, view.spinner_frame);
            let lead = Line::from(vec![Span::styled(format!(" {} ", ti + 1), num), icon.clone()]);
            let lead_width = lead.width() as u16;
            let full = Line::from([lead.spans, vec![Span::styled(label, text)]].concat());
            let compact = Line::from(vec![Span::styled(format!(" {}", ti + 1), num), icon]);
            Tab { full, compact, lead_width }
        })
        .collect();
    let widths: Vec<(u16, u16)> =
        tabs.iter().map(|t| (t.full.width() as u16, t.compact.width() as u16)).collect();
    let plan = plan(&widths, ws.active_tab, avail);

    let mut x = area.x;
    if plan.hidden_left > 0 {
        let marker = format!("‹{}", plan.hidden_left);
        let w = marker.chars().count() as u16;
        buf.set_line(x, area.y, &Line::from(Span::styled(marker, theme::muted())), w);
        hits.push(Rect::new(x, area.y, w, 1), Click::Tab(view.active, plan.first - 1));
        x += w + 1;
    }
    for (ti, tab) in tabs.iter().enumerate().skip(plan.first).take(plan.shown) {
        let active = ti == ws.active_tab;
        let line = if active || !plan.compact { &tab.full } else { &tab.compact };
        let width = line.width() as u16;
        buf.set_line(x, area.y, line, width);
        if let (true, Some(name)) = (active, &editing) {
            // Right after the typed text, past the leading space.
            let at = x + tab.lead_width + 1 + Line::raw(name.as_str()).width() as u16;
            cursor = Some(Position::new(at.min(right.saturating_sub(1)), area.y));
        }
        hits.push(Rect::new(x, area.y, width, 1), Click::Tab(view.active, ti));
        x += width + 1;
    }
    let hidden_right = tabs.len() - plan.first - plan.shown;
    if hidden_right > 0 {
        let marker = format!("{hidden_right}›");
        let w = marker.chars().count() as u16;
        buf.set_line(x, area.y, &Line::from(Span::styled(marker, theme::muted())), w);
        hits.push(Rect::new(x, area.y, w, 1), Click::Tab(view.active, plan.first + plan.shown));
        x += w + 1;
    }

    if x + 3 <= right {
        buf.set_line(x, area.y, &Line::from(Span::styled(" + ", theme::muted())), 3);
        hits.push(Rect::new(x, area.y, 3, 1), Click::NewTab);
    }
    cursor
}

/// One tab drawn two ways: with its name, or as just its number and status.
struct Tab {
    full: Line<'static>,
    compact: Line<'static>,
    /// Width of the number and icon in `full`, for the rename cursor.
    lead_width: u16,
}

/// Which tabs fit, and how.
#[derive(Debug, PartialEq, Eq)]
struct Plan {
    /// Every tab but the active one is drawn compact.
    compact: bool,
    first: usize,
    shown: usize,
    /// Tabs scrolled off the left, counted by the `‹n` marker.
    hidden_left: usize,
}

/// Full tabs when they all fit. Otherwise every tab but the active one
/// shrinks to its number and status, so all of them stay visible. When even
/// that's too wide, a window around the active tab is shown, with `‹n` and
/// `n›` counting what's hidden. `widths` is (full, compact) per tab, and
/// tabs have one cell between them.
fn plan(widths: &[(u16, u16)], active: usize, avail: u16) -> Plan {
    let n = widths.len();
    let row = |w: &dyn Fn(usize) -> u16| -> u32 { (0..n).map(|i| w(i) as u32 + 1).sum() };
    if row(&|i| widths[i].0) <= avail as u32 + 1 {
        return Plan { compact: false, first: 0, shown: n, hidden_left: 0 };
    }
    let size = |i: usize| if i == active { widths[i].0 } else { widths[i].1 };
    if row(&size) <= avail as u32 + 1 {
        return Plan { compact: true, first: 0, shown: n, hidden_left: 0 };
    }
    if n == 0 {
        return Plan { compact: true, first: 0, shown: 0, hidden_left: 0 };
    }
    // Grow a window out from the active tab, one side at a time, keeping
    // room for both markers ("‹99" or "99›" plus a gap).
    let marker = |hidden: usize| if hidden == 0 { 0 } else { hidden.to_string().len() as u32 + 2 };
    let fits = |lo: usize, hi: usize| {
        let tabs: u32 = (lo..=hi).map(|i| size(i) as u32 + 1).sum();
        tabs + marker(lo) + marker(n - 1 - hi) <= avail as u32 + 1
    };
    let (mut lo, mut hi) = (active.min(n - 1), active.min(n - 1));
    loop {
        let right = hi + 1 < n && fits(lo, hi + 1);
        if right {
            hi += 1;
        }
        let left = lo > 0 && fits(lo - 1, hi);
        if left {
            lo -= 1;
        }
        if !right && !left {
            break;
        }
    }
    Plan { compact: true, first: lo, shown: hi - lo + 1, hidden_left: lo }
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
    fn full_tabs_when_they_fit() {
        let p = plan(&[(10, 3), (10, 3)], 0, 21);
        assert_eq!(p, Plan { compact: false, first: 0, shown: 2, hidden_left: 0 });
    }

    #[test]
    fn others_collapse_so_every_tab_shows() {
        // Full: 20 + 1 + 20 + 1 + 20 = 62. Collapsed: 3 + 1 + 20 + 1 + 3 = 28.
        let p = plan(&[(20, 3), (20, 3), (20, 3)], 1, 40);
        assert_eq!(p, Plan { compact: true, first: 0, shown: 3, hidden_left: 0 });
    }

    #[test]
    fn too_many_scrolls_around_the_active_tab() {
        let widths = vec![(20, 3); 12];
        let p = plan(&widths, 10, 30);
        assert!(p.compact);
        assert!(p.first <= 10 && 10 < p.first + p.shown, "active is shown: {p:?}");
        assert_eq!(p.hidden_left, p.first);
        assert!(p.first > 0, "some hidden on the left");
        // What's drawn really fits: tabs, gaps, and both markers.
        let hidden_right = 12 - p.first - p.shown;
        let used: u32 = (p.first..p.first + p.shown).map(|i| if i == 10 { 21 } else { 4 }).sum::<u32>()
            + if p.hidden_left > 0 { p.hidden_left.to_string().len() as u32 + 2 } else { 0 }
            + if hidden_right > 0 { hidden_right.to_string().len() as u32 + 2 } else { 0 };
        assert!(used <= 31, "{used} cells for {p:?}");
    }

    #[test]
    fn no_tabs_is_fine() {
        assert_eq!(plan(&[], 0, 10).shown, 0);
    }

    #[test]
    fn long_names_show_their_end_while_typing() {
        assert_eq!(tail("short", 10), "short");
        assert_eq!(tail("abcdefghij", 5), "…ghij");
    }
}
