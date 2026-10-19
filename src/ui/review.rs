//! The review view, drawn over the pane area: a header with the scopes, the
//! changed files on the left, and the selected file's diff on the right.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget, Wrap};

use super::truncate;
use crate::review::{Body, DiffLine, Focus, Kind, Review, Scope, markdown};
use crate::theme;

/// Narrower than this, the files and the diff take turns (tab switches).
const MIN_SPLIT_WIDTH: u16 = 80;

/// Draws the review and returns where the text cursor goes while a comment
/// is being typed.
pub fn draw(frame: &mut Frame, area: Rect, review: &mut Review) -> Option<Position> {
    let buf = frame.buffer_mut();
    buf.set_style(area, theme::panel());
    let [header, body, bottom] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)]).areas(area);
    draw_header(buf, header, review);

    let split = area.width >= MIN_SPLIT_WIDTH;
    let (files, diff) = if split {
        let w = (body.width / 3).clamp(24, 44);
        let [f, sep, d] =
            Layout::horizontal([Constraint::Length(w), Constraint::Length(1), Constraint::Min(1)]).areas(body);
        for y in sep.top()..sep.bottom() {
            buf.set_string(sep.x, y, "│", theme::border());
        }
        (f, d)
    } else if review.focus == Focus::Files {
        (body, Rect::default())
    } else {
        (Rect::default(), body)
    };
    review.areas.files = files;
    review.areas.diff = diff;

    if review.files.is_empty() {
        let why = review.problem.clone().unwrap_or_else(|| "no changes".into());
        let at = if files.width > 0 { files } else { diff };
        buf.set_string(at.x + 1, at.y, truncate(&why, at.width.saturating_sub(2) as usize), theme::muted());
    } else {
        draw_files(buf, files, review);
        draw_body(buf, diff, review);
    }
    draw_bottom(buf, bottom, review)
}

fn draw_header(buf: &mut Buffer, area: Rect, review: &mut Review) {
    buf.set_style(area, theme::bar());
    let mut x = area.x;
    let mut put = |buf: &mut Buffer, text: &str, style: Style| {
        let w = text.chars().count() as u16;
        let rect = Rect::new(x, area.y, w.min(area.right().saturating_sub(x)), 1);
        buf.set_string(x, area.y, text, style);
        x += w;
        rect
    };
    put(buf, " Review ", theme::accent());
    for (i, (scope, label)) in [(Scope::Uncommitted, " uncommitted "), (Scope::LastTurn, " last turn ")]
        .into_iter()
        .enumerate()
    {
        let style = if review.scope == scope { theme::active_row() } else { theme::muted() };
        review.areas.scopes[i] = put(buf, label, style);
    }

    let added: u32 = review.files.iter().filter_map(|f| f.added).sum();
    let deleted: u32 = review.files.iter().filter_map(|f| f.deleted).sum();
    let n = review.files.len();
    let mut right = vec![
        Span::styled(format!("{n} file{}  ", if n == 1 { "" } else { "s" }), theme::muted()),
        Span::styled(format!("+{added} "), Style::new().fg(theme::palette().ok)),
        Span::styled(format!("-{deleted}"), Style::new().fg(theme::palette().danger)),
    ];
    if !review.comments.is_empty() {
        let c = review.comments.len();
        right.push(Span::styled(format!("  {c} comment{} ", if c == 1 { "" } else { "s" }), Style::new().fg(theme::palette().warn)));
    }
    right.push(Span::raw(" "));
    let line = Line::from(right);
    let w = line.width() as u16;
    // Only when it fits next to the scopes.
    if x + w + 2 <= area.right() {
        buf.set_line(area.right() - w, area.y, &line, w);
    }
}

fn draw_files(buf: &mut Buffer, area: Rect, review: &mut Review) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let rows = area.height as usize;
    // Keep the selected file on screen.
    if review.selected < review.file_scroll {
        review.file_scroll = review.selected;
    } else if review.selected >= review.file_scroll + rows {
        review.file_scroll = review.selected + 1 - rows;
    }
    review.file_scroll = review.file_scroll.min(review.files.len().saturating_sub(rows));

    for (row, (i, file)) in review.files.iter().enumerate().skip(review.file_scroll).take(rows).enumerate() {
        let y = area.y + row as u16;
        let selected = i == review.selected;
        let base = if selected { theme::active_row() } else { theme::panel() };
        buf.set_style(Rect::new(area.x, y, area.width, 1), base);

        let stats = match (file.added, file.deleted) {
            (Some(a), Some(d)) => vec![
                Span::styled(format!("+{a}"), Style::new().fg(theme::palette().ok)),
                Span::styled(format!(" -{d} "), Style::new().fg(theme::palette().danger)),
            ],
            _ => vec![Span::styled("bin ", theme::faint())],
        };
        let stats_w: usize = stats.iter().map(Span::width).sum();
        let marker = match (selected && review.focus == Focus::Files, review.comments_on(&file.path) > 0) {
            (_, true) => Span::styled("▌", Style::new().fg(theme::palette().warn)),
            (true, false) => Span::styled("›", theme::accent()),
            _ => Span::raw(" "),
        };
        let room = (area.width as usize).saturating_sub(stats_w + 3);
        let name = format!("{:<room$}", truncate_start(&file.path, room));
        let mut spans = vec![marker, Span::raw(" "), Span::styled(name, base), Span::raw(" ")];
        spans.extend(stats);
        buf.set_line(area.x, y, &Line::from(spans), area.width);
    }
}

fn draw_body(buf: &mut Buffer, area: Rect, review: &mut Review) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    review.rows = area.height as usize;
    match &review.body {
        Body::Message(m) => {
            buf.set_string(area.x + 1, area.y, truncate(m, area.width.saturating_sub(2) as usize), theme::muted());
        }
        Body::Markdown(src) => {
            let inner = Rect::new(area.x + 1, area.y, area.width.saturating_sub(2), area.height);
            Paragraph::new(markdown::render(src))
                .style(theme::panel())
                .wrap(Wrap { trim: false })
                .scroll((review.cursor.min(u16::MAX as usize) as u16, 0))
                .render(inner, buf);
        }
        Body::Diff(lines) => {
            let rows = area.height as usize;
            if review.cursor < review.scroll {
                review.scroll = review.cursor;
            } else if review.cursor >= review.scroll + rows {
                review.scroll = review.cursor + 1 - rows;
            }
            review.scroll = review.scroll.min(lines.len().saturating_sub(rows));
            let digits = lines
                .iter()
                .filter_map(|l| l.old.max(l.new))
                .max()
                .map_or(3, |n| n.to_string().len().max(3));
            let (sel_a, sel_b) = review.selection();
            let focused = review.focus == Focus::Diff;
            for (row, (i, line)) in lines.iter().enumerate().skip(review.scroll).take(rows).enumerate() {
                let y = area.y + row as u16;
                let is_cursor = focused && i == review.cursor;
                let selected = focused && review.anchor.is_some() && (sel_a..=sel_b).contains(&i);
                if is_cursor || selected {
                    buf.set_style(Rect::new(area.x, y, area.width, 1), theme::active_row());
                }
                let marker = if review.comment_at(i).is_some() {
                    Span::styled("▌", Style::new().fg(theme::palette().warn))
                } else if is_cursor {
                    Span::styled("›", theme::accent())
                } else {
                    Span::raw(" ")
                };
                buf.set_line(area.x, y, &diff_line(line, digits, marker), area.width);
            }
        }
    }
}

fn diff_line(line: &DiffLine, digits: usize, marker: Span<'static>) -> Line<'static> {
    let num = |n: Option<u32>| format!("{:>digits$} ", n.map(|n| n.to_string()).unwrap_or_default());
    let (prefix, style) = match line.kind {
        Kind::Add => ("+", Style::new().fg(theme::palette().ok)),
        Kind::Del => ("-", Style::new().fg(theme::palette().danger)),
        Kind::Hunk => ("", theme::muted()),
        Kind::Note => ("", theme::faint()),
        Kind::Context => (" ", Style::new().fg(theme::palette().text)),
    };
    Line::from(vec![
        marker,
        Span::styled(num(line.old), theme::faint()),
        Span::styled(num(line.new), theme::faint()),
        Span::styled(format!("{prefix}{}", line.text), style),
    ])
}

/// The bottom row: the comment being typed, a notice, or the comment under
/// the cursor.
fn draw_bottom(buf: &mut Buffer, area: Rect, review: &Review) -> Option<Position> {
    buf.set_style(area, theme::bar());
    let width = area.width as usize;
    if let Some(draft) = &review.draft {
        let label = format!(" comment {} › ", draft.target.label());
        let room = width.saturating_sub(label.chars().count() + 1);
        // Show the end of a long comment, where you're typing.
        let skip = draft.text.chars().count().saturating_sub(room);
        let shown: String = draft.text.chars().skip(skip).collect();
        let line = Line::from(vec![Span::styled(label.clone(), theme::accent()), Span::raw(shown.clone())]);
        buf.set_line(area.x, area.y, &line, area.width);
        let x = area.x + (label.chars().count() + shown.chars().count()) as u16;
        return Some(Position::new(x.min(area.right().saturating_sub(1)), area.y));
    }
    let line = if let Some(notice) = &review.notice {
        Line::from(Span::styled(format!(" {notice}"), Style::new().fg(theme::palette().warn)))
    } else if let Some(i) = review.comment_at(review.cursor).filter(|_| review.focus == Focus::Diff) {
        let c = &review.comments[i];
        Line::from(vec![
            Span::styled(" ▌ ", Style::new().fg(theme::palette().warn)),
            Span::styled(format!("{}: ", c.label()), theme::muted()),
            Span::styled(truncate(&c.text, width.saturating_sub(12)), Style::new().fg(theme::palette().text)),
        ])
    } else {
        return None;
    };
    buf.set_line(area.x, area.y, &line, area.width);
    None
}

/// Cuts from the front so the file name survives: `…/review/mod.rs`.
fn truncate_start(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_owned();
    }
    let mut out = String::from("…");
    out.extend(s.chars().skip(n + 1 - max.max(1)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_paths_keep_their_file_name() {
        assert_eq!(truncate_start("src/review/mod.rs", 20), "src/review/mod.rs");
        assert_eq!(truncate_start("src/review/mod.rs", 10), "…ew/mod.rs");
    }
}
