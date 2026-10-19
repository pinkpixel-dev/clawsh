//! The conversation search overlay (`ctrl+b /`): a query line, how the
//! search is going, and two rows per result (title, folder, and age, then
//! the matching line with the match picked out).

use std::time::SystemTime;

use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::Modifier;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Widget};

use super::radar::age;
use super::{Click, Hits, truncate};
use crate::search::{MAX_HITS, Search, Who};
use crate::theme;
use crate::workspace::folder_name;

const WIDTH: u16 = 96;
const HEIGHT: u16 = 30;
const FOLDER_WIDTH: usize = 16;
const AGE_WIDTH: usize = 4;

/// Draws the search over everything else and returns where the text cursor
/// belongs.
pub fn draw(buf: &mut Buffer, screen: Rect, search: &Search, hits: &mut Hits) -> Position {
    let w = WIDTH.min(screen.width.saturating_sub(4));
    let h = HEIGHT.min(screen.height.saturating_sub(4));
    let area = Rect::new(
        screen.x + (screen.width.saturating_sub(w)) / 2,
        screen.y + (screen.height.saturating_sub(h)) / 3,
        w,
        h,
    );
    let block = Block::bordered()
        .title(Span::styled(" Search conversations ", theme::accent()))
        .border_style(theme::border())
        .style(theme::panel());
    let inner = block.inner(area);
    Clear.render(area, buf);
    block.render(area, buf);
    hits.push(area, Click::SearchBackground);

    let prompt = Line::from(vec![Span::styled(" › ", theme::accent()), Span::raw(search.query.clone())]);
    buf.set_line(inner.x, inner.y, &prompt, inner.width);
    let cursor = Position::new(
        (inner.x + 3 + search.query.chars().count() as u16).min(inner.right().saturating_sub(1)),
        inner.y,
    );
    if inner.height < 2 {
        return cursor;
    }
    let status = Line::from(Span::styled(format!("   {}", status(search)), theme::faint()));
    buf.set_line(inner.x, inner.y + 1, &status, inner.width);

    let list_top = inner.y + 3;
    let slots = (inner.bottom().saturating_sub(list_top) / 2) as usize;
    let at = search.selected_index();
    // Keep the selection on screen.
    let first = at.saturating_sub(slots.saturating_sub(1));
    let width = inner.width as usize;
    // Marker on the left; folder and age columns on the right.
    let title_width = width.saturating_sub(3 + 1 + FOLDER_WIDTH + 1 + AGE_WIDTH + 1);
    let now = SystemTime::now();
    for (i, hit) in search.hits.iter().enumerate().skip(first).take(slots) {
        let y = list_top + ((i - first) * 2) as u16;
        let picked = i == at;
        let rect = Rect::new(inner.x, y, inner.width, 2);
        let base = if picked { theme::active_row() } else { theme::panel() };
        buf.set_style(rect, base);

        let top = Line::from(vec![
            Span::styled(if picked { " › " } else { "   " }, theme::accent()),
            Span::styled(format!("{:<title_width$}", truncate(&hit.title, title_width)), base),
            Span::styled(format!(" {:<FOLDER_WIDTH$}", truncate(&folder_name(&hit.cwd), FOLDER_WIDTH)), theme::muted()),
            Span::styled(format!(" {:>AGE_WIDTH$}", age(now.duration_since(hit.modified).ok())), theme::faint()),
        ]);
        buf.set_line(inner.x, y, &top, inner.width);

        let s = &hit.snippet;
        let who = match s.who {
            Who::You => "you",
            Who::Claude => "claude",
        };
        // Enough of what came before to read the match in context, while
        // leaving the match itself on screen.
        let room = width / 4;
        let skip = s.before.chars().count().saturating_sub(room);
        let mut parts = vec![
            Span::styled(format!("   {who:<7}"), theme::faint()),
            Span::styled(s.before.chars().skip(skip).collect::<String>(), theme::muted()),
            Span::styled(s.matched.clone(), theme::accent().add_modifier(Modifier::UNDERLINED)),
            Span::styled(s.after.clone(), theme::muted()),
        ];
        if hit.count > 1 {
            parts.push(Span::styled(format!("  +{}", hit.count - 1), theme::faint()));
        }
        buf.set_line(inner.x, y + 1, &Line::from(parts), inner.width);
        hits.push(rect, Click::SearchItem(i));
    }
    cursor
}

/// One line on how the search is going.
fn status(search: &Search) -> String {
    let n = search.hits.len();
    if search.stale() {
        return "enter to search".to_owned();
    }
    match (search.scanning, n) {
        (true, _) => format!("searching… {n} found"),
        (false, 0) => "no matches".to_owned(),
        (false, 1) => "1 conversation".to_owned(),
        _ if search.capped => format!("newest {MAX_HITS} conversations"),
        _ => format!("{n} conversations"),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::persist::SessionRef;
    use crate::search::{Hit, Snippet};

    fn hit(id: &str, title: &str, mins: u64, count: usize) -> Hit {
        Hit {
            session: SessionRef::new(id, None).unwrap(),
            cwd: PathBuf::from("/p/clawsh"),
            title: title.into(),
            modified: SystemTime::now() - Duration::from_secs(mins * 60),
            count,
            snippet: Snippet {
                who: Who::Claude,
                before: "the radar puts ".into(),
                matched: "needs-input".into(),
                after: " tabs first".into(),
            },
        }
    }

    fn row_text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect()
    }

    #[test]
    fn draws_two_rows_per_conversation() {
        let mut search = Search::new();
        search.query = "needs".into();
        search.start(None, mpsc::channel().0);
        // `start` with no projects folder finds nothing; feed results in by hand.
        let g = search.generation();
        search.add(g, hit("a", "Radar ordering", 5, 3));
        search.add(g, hit("b", "Older chat", 600, 1));
        let screen = Rect::new(0, 0, 100, 24);
        let mut buf = Buffer::empty(screen);
        let mut hits = Hits::default();
        let cursor = draw(&mut buf, screen, &search, &mut hits);

        // 20 rows tall in a 24 row screen, a third of the gap down: top at 1.
        assert!(row_text(&buf, 2).contains("› needs"));
        assert_eq!(cursor, Position::new(3 + 3 + 5, 2));
        assert!(row_text(&buf, 3).contains("2 conversations"));
        let first = row_text(&buf, 5);
        assert!(first.contains("› Radar ordering") && first.contains("clawsh") && first.contains("5m"), "{first}");
        let snippet = row_text(&buf, 6);
        assert!(snippet.contains("claude") && snippet.contains("the radar puts needs-input tabs first  +2"), "{snippet}");
        assert!(row_text(&buf, 7).contains("Older chat") && row_text(&buf, 7).contains("10h"));
        assert_eq!(hits.at(30, 6), Some(Click::SearchItem(0)));
        assert_eq!(hits.at(30, 7), Some(Click::SearchItem(1)));
        assert_eq!(hits.at(30, 3), Some(Click::SearchBackground));
    }

    #[test]
    fn status_says_what_is_happening() {
        let mut search = Search::new();
        assert_eq!(status(&search), "enter to search");
        search.query = "x".into();
        search.start(None, mpsc::channel().0);
        assert_eq!(status(&search), "no matches");
        search.scanning = true;
        assert_eq!(status(&search), "searching… 0 found");
        search.query.push('y');
        assert_eq!(status(&search), "enter to search");
    }
}
