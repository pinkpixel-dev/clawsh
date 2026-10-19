//! Keys and mouse for the review view.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use super::{Body, Comment, Draft, Focus, Review, Scope};

/// Longest comment, in characters.
const MAX_COMMENT: usize = 2000;
/// Lines moved per mouse wheel tick.
const WHEEL: isize = 3;

/// What the app has to do after a key or click.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
    /// Take a new snapshot and reload.
    Refresh,
    /// Paste this into the tab's Claude.
    Send(String),
}

impl Review {
    pub fn key(&mut self, k: &KeyEvent) -> Outcome {
        if k.kind == KeyEventKind::Release {
            return Outcome::Stay;
        }
        if self.draft.is_some() {
            self.draft_key(k);
            return Outcome::Stay;
        }
        self.notice = None;
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') => return Outcome::Close,
            KeyCode::Char('u') if !ctrl => return self.set_scope(Scope::Uncommitted),
            KeyCode::Char('t') => return self.set_scope(Scope::LastTurn),
            KeyCode::Char('r') => return Outcome::Refresh,
            KeyCode::Char('s') => match self.message() {
                Some(text) => return Outcome::Send(text),
                None => self.notice = Some("no comments yet".into()),
            },
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::Files => Focus::Diff,
                    Focus::Diff => Focus::Files,
                };
            }
            KeyCode::Char('m') => {
                if self.is_markdown() {
                    self.rendered = !self.rendered;
                    self.load_body(true, self.rendered);
                } else {
                    self.notice = Some("not a markdown file".into());
                }
            }
            KeyCode::Char('f') => self.select_file(self.selected as isize + 1),
            KeyCode::Char('F') => self.select_file(self.selected as isize - 1),
            KeyCode::Char('n') => self.jump_comment(1),
            KeyCode::Char('N') => self.jump_comment(-1),
            _ => match self.focus {
                Focus::Files => self.files_key(k),
                Focus::Diff => self.diff_key(k, ctrl),
            },
        }
        Outcome::Stay
    }

    fn files_key(&mut self, k: &KeyEvent) {
        match k.code {
            KeyCode::Char('j') | KeyCode::Down => self.select_file(self.selected as isize + 1),
            KeyCode::Char('k') | KeyCode::Up => self.select_file(self.selected as isize - 1),
            KeyCode::Char('g') | KeyCode::Home => self.select_file(0),
            KeyCode::Char('G') | KeyCode::End => self.select_file(self.files.len() as isize - 1),
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => self.focus = Focus::Diff,
            _ => {}
        }
    }

    fn diff_key(&mut self, k: &KeyEvent, ctrl: bool) {
        let page = (self.rows / 2).max(1) as isize;
        match k.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_cursor(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_cursor(-1),
            KeyCode::Char('d') if ctrl => self.move_cursor(page),
            KeyCode::Char('u') if ctrl => self.move_cursor(-page),
            KeyCode::PageDown => self.move_cursor(page * 2),
            KeyCode::PageUp => self.move_cursor(-page * 2),
            KeyCode::Char('g') | KeyCode::Home => self.cursor = 0,
            KeyCode::Char('G') | KeyCode::End => self.cursor = self.body_len().saturating_sub(1),
            KeyCode::Char(']') => self.jump_hunk(1),
            KeyCode::Char('[') => self.jump_hunk(-1),
            KeyCode::Char('h') | KeyCode::Left => self.focus = Focus::Files,
            KeyCode::Char('v') => {
                self.anchor = match self.anchor {
                    Some(_) => None,
                    None => Some(self.cursor),
                };
            }
            KeyCode::Char('c') | KeyCode::Enter => self.start_comment(),
            KeyCode::Char('d') => match self.comment_at(self.cursor) {
                Some(i) => {
                    self.comments.remove(i);
                }
                None => self.notice = Some("no comment on this line".into()),
            },
            _ => {}
        }
    }

    fn draft_key(&mut self, k: &KeyEvent) {
        let Some(draft) = &mut self.draft else { return };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => self.draft = None,
            KeyCode::Enter => self.save_draft(),
            KeyCode::Backspace => {
                draft.text.pop();
            }
            KeyCode::Char('u') if ctrl => draft.text.clear(),
            KeyCode::Char(c) if !ctrl && !k.modifiers.contains(KeyModifiers::ALT) => push_text(&mut draft.text, c),
            _ => {}
        }
    }

    /// Pasted text goes into the comment being typed, as one line.
    pub fn paste(&mut self, text: &str) {
        if let Some(draft) = &mut self.draft {
            for c in text.chars() {
                push_text(&mut draft.text, if c == '\n' { ' ' } else { c });
            }
        }
    }

    fn save_draft(&mut self) {
        let Some(draft) = self.draft.take() else { return };
        let text = draft.text.trim().to_owned();
        match draft.editing {
            // Emptying a comment deletes it.
            Some(i) if text.is_empty() => {
                self.comments.remove(i);
            }
            Some(i) => self.comments[i].text = text,
            None if text.is_empty() => {}
            None => self.comments.push(Comment { text, ..draft.target }),
        }
    }

    /// `c`: edit the comment under the cursor, or start one on the selection.
    fn start_comment(&mut self) {
        let Body::Diff(lines) = &self.body else {
            self.notice = Some("comments go on the diff (m)".into());
            return;
        };
        if self.anchor.is_none()
            && let Some(i) = self.comment_at(self.cursor)
        {
            self.draft = Some(Draft { text: self.comments[i].text.clone(), editing: Some(i), target: self.comments[i].clone() });
            return;
        }
        let (a, b) = self.selection();
        let picked: Vec<_> = lines.get(a..=b).unwrap_or_default().iter().filter(|l| l.is_code()).collect();
        let Some(path) = self.path().filter(|_| !picked.is_empty()) else {
            self.notice = Some("pick a line of code".into());
            return;
        };
        let target = Comment::on(path, &picked);
        self.draft = Some(Draft { text: String::new(), editing: None, target });
        self.anchor = None;
    }

    fn set_scope(&mut self, scope: Scope) -> Outcome {
        if self.scope == scope {
            return Outcome::Stay;
        }
        self.scope = scope;
        Outcome::Refresh
    }

    pub(crate) fn select_file(&mut self, i: isize) {
        let last = self.files.len() as isize - 1;
        let i = i.clamp(0, last.max(0)) as usize;
        if i != self.selected || matches!(self.body, Body::Message(ref m) if m.is_empty()) {
            self.selected = i;
            self.load_body(true, self.rendered);
        }
    }

    fn move_cursor(&mut self, by: isize) {
        let last = self.body_len().saturating_sub(1) as isize;
        self.cursor = (self.cursor as isize + by).clamp(0, last.max(0)) as usize;
    }

    fn jump_hunk(&mut self, dir: isize) {
        let Body::Diff(lines) = &self.body else { return };
        let is_hunk = |i: &usize| lines[*i].kind == super::Kind::Hunk;
        let found = if dir > 0 {
            (self.cursor + 1..lines.len()).find(is_hunk)
        } else {
            (0..self.cursor).rev().find(is_hunk)
        };
        if let Some(i) = found {
            self.cursor = i;
        }
    }

    /// `n`/`N`: the next or previous commented line, moving on to other files
    /// with comments when this one has no more.
    fn jump_comment(&mut self, dir: isize) {
        if self.comments.is_empty() {
            self.notice = Some("no comments yet".into());
            return;
        }
        let here = self.commented_lines();
        let next = if dir > 0 {
            here.iter().find(|&&i| i > self.cursor)
        } else {
            here.iter().rev().find(|&&i| i < self.cursor)
        };
        if let Some(&i) = next {
            self.cursor = i;
            return;
        }
        let n = self.files.len() as isize;
        for step in 1..=n {
            let i = (self.selected as isize + dir * step).rem_euclid(n) as usize;
            if self.comments_on(&self.files[i].path) > 0 {
                self.selected = i;
                // Comments only show on the diff, even for markdown.
                self.load_body(true, false);
                let lines = self.commented_lines();
                if let Some(&c) = if dir > 0 { lines.first() } else { lines.last() } {
                    self.cursor = c;
                    self.focus = Focus::Diff;
                    return;
                }
            }
        }
        self.notice = Some("no comment found in this diff".into());
    }

    fn commented_lines(&self) -> Vec<usize> {
        let len = if let Body::Diff(lines) = &self.body { lines.len() } else { 0 };
        (0..len).filter(|&i| self.comment_at(i).is_some()).collect()
    }

    pub fn mouse(&mut self, m: &MouseEvent) -> Outcome {
        let at = Position::new(m.column, m.row);
        let row_in = |r: Rect| (r.contains(at)).then(|| (m.row - r.y) as usize);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if self.draft.is_some() {
                    return Outcome::Stay;
                }
                self.notice = None;
                if let Some(i) = self.areas.scopes.iter().position(|r| r.contains(at)) {
                    return self.set_scope(if i == 0 { Scope::Uncommitted } else { Scope::LastTurn });
                }
                if let Some(row) = row_in(self.areas.files) {
                    self.focus = Focus::Files;
                    if self.file_scroll + row < self.files.len() {
                        self.select_file((self.file_scroll + row) as isize);
                    }
                } else if let Some(row) = row_in(self.areas.diff) {
                    self.focus = Focus::Diff;
                    self.anchor = None;
                    let line = self.scroll + row;
                    if line < self.body_len() {
                        self.cursor = line;
                        self.drag_from = Some(line);
                    }
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let diff = self.areas.diff;
                if let Some(from) = self.drag_from
                    && matches!(self.body, Body::Diff(_))
                    && diff.height > 0
                {
                    // Dragging past the edges still picks the first or last row.
                    let row = m.row.clamp(diff.y, diff.bottom() - 1) - diff.y;
                    self.cursor = (self.scroll + row as usize).min(self.body_len().saturating_sub(1));
                    self.anchor = (self.cursor != from).then_some(from);
                }
            }
            MouseEventKind::Up(MouseButton::Left) => self.drag_from = None,
            MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                let by = if m.kind == MouseEventKind::ScrollDown { 1 } else { -1 };
                if self.areas.files.contains(at) {
                    self.select_file(self.selected as isize + by);
                } else if self.areas.diff.contains(at) {
                    self.move_cursor(by * WHEEL);
                }
            }
            _ => {}
        }
        Outcome::Stay
    }
}

fn push_text(text: &mut String, c: char) {
    if !c.is_control() && text.chars().count() < MAX_COMMENT {
        text.push(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::{DiffLine, FileStat, Kind};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn line(kind: Kind, new: Option<u32>) -> DiffLine {
        DiffLine { kind, old: None, new, text: "x".into() }
    }

    /// A review with one file and a diff already loaded, no git needed.
    fn review() -> Review {
        let mut r = Review::new(".".into());
        r.files = vec![FileStat { path: "a.rs".into(), added: Some(2), deleted: Some(0) }];
        r.body = Body::Diff(vec![line(Kind::Hunk, None), line(Kind::Add, Some(1)), line(Kind::Add, Some(2))]);
        r.focus = Focus::Diff;
        r
    }

    fn type_text(r: &mut Review, s: &str) {
        for c in s.chars() {
            r.key(&key(KeyCode::Char(c)));
        }
    }

    #[test]
    fn select_a_range_comment_edit_and_send() {
        let mut r = review();
        r.key(&key(KeyCode::Char('j')));
        r.key(&key(KeyCode::Char('v')));
        r.key(&key(KeyCode::Char('j')));
        r.key(&key(KeyCode::Char('c')));
        type_text(&mut r, "nice");
        r.key(&key(KeyCode::Enter));
        assert_eq!(r.comments.len(), 1);
        assert_eq!(r.comments[0].label(), "1-2");
        assert_eq!(r.comment_at(1), Some(0));
        assert_eq!(r.comment_at(0), None, "hunk headers never carry comments");

        // `c` on a commented line edits it; emptying it deletes it.
        r.key(&key(KeyCode::Char('c')));
        assert_eq!(r.draft.as_ref().map(|d| d.text.as_str()), Some("nice"));
        r.key(&KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        r.key(&key(KeyCode::Enter));
        assert!(r.comments.is_empty());

        assert_eq!(r.key(&key(KeyCode::Char('s'))), Outcome::Stay);
        assert_eq!(r.notice.as_deref(), Some("no comments yet"));
        r.key(&key(KeyCode::Char('c')));
        type_text(&mut r, "ok");
        r.key(&key(KeyCode::Enter));
        assert!(matches!(r.key(&key(KeyCode::Char('s'))), Outcome::Send(t) if t.contains("a.rs:2\n")));
    }

    #[test]
    fn comments_need_a_code_line_and_esc_cancels() {
        let mut r = review();
        r.key(&key(KeyCode::Char('c')));
        assert!(r.draft.is_none());
        r.key(&key(KeyCode::Char('j')));
        r.key(&key(KeyCode::Char('c')));
        type_text(&mut r, "q");
        assert_eq!(r.key(&key(KeyCode::Esc)), Outcome::Stay, "esc only cancels the draft");
        assert!(r.comments.is_empty());
        assert_eq!(r.key(&key(KeyCode::Esc)), Outcome::Close);
    }

    #[test]
    fn scope_keys_ask_for_a_refresh_once() {
        let mut r = review();
        assert_eq!(r.key(&key(KeyCode::Char('u'))), Outcome::Stay);
        assert_eq!(r.key(&key(KeyCode::Char('t'))), Outcome::Refresh);
        assert_eq!(r.scope, Scope::LastTurn);
    }

    #[test]
    fn click_and_drag_select_diff_lines() {
        let mut r = review();
        r.focus = Focus::Files;
        r.areas.diff = Rect::new(40, 2, 60, 10);
        let ev = |kind, row| MouseEvent { kind, column: 50, row, modifiers: KeyModifiers::NONE };
        r.mouse(&ev(MouseEventKind::Down(MouseButton::Left), 3));
        assert_eq!((r.focus, r.cursor, r.anchor), (Focus::Diff, 1, None));
        // Dragging below the diff clamps to its last row, then to the last line.
        r.mouse(&ev(MouseEventKind::Drag(MouseButton::Left), 30));
        assert_eq!((r.cursor, r.anchor), (2, Some(1)));
        r.mouse(&ev(MouseEventKind::Up(MouseButton::Left), 30));
        r.mouse(&ev(MouseEventKind::ScrollUp, 3));
        assert_eq!(r.cursor, 0);
    }

    #[test]
    fn hunks_and_cursor_stay_in_bounds() {
        let mut r = review();
        r.key(&key(KeyCode::Char('G')));
        assert_eq!(r.cursor, 2);
        r.key(&key(KeyCode::Char('j')));
        assert_eq!(r.cursor, 2);
        r.key(&key(KeyCode::Char('[')));
        assert_eq!(r.cursor, 0);
    }
}
