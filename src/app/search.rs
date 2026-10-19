//! Conversation search's keys and clicks, and resuming the conversation you
//! pick.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::{App, Mode};
use crate::pane::Pane;
use crate::search::{self, Hit, Search};
use crate::ui::Click;

/// Longest query accepted, in characters.
const MAX_QUERY: usize = 200;

impl App {
    /// `ctrl+b /`. Nothing is scanned until you press Enter.
    pub(super) fn open_search(&mut self) {
        self.mode = Mode::Search(Search::new());
    }

    pub(super) fn search_key(&mut self, k: &KeyEvent) -> Result<()> {
        if k.kind == KeyEventKind::Release {
            return Ok(());
        }
        let Mode::Search(search) = &mut self.mode else {
            return Ok(());
        };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            // A changed query searches; otherwise Enter resumes the pick.
            KeyCode::Enter if search.stale() => search.start(search::projects_dir(), self.tx.clone()),
            KeyCode::Enter => return self.resume_picked(),
            KeyCode::Up | KeyCode::BackTab => search.move_by(-1),
            KeyCode::Down | KeyCode::Tab => search.move_by(1),
            KeyCode::Char('p') if ctrl => search.move_by(-1),
            KeyCode::Char('n') if ctrl => search.move_by(1),
            KeyCode::PageUp => search.move_by(-10),
            KeyCode::PageDown => search.move_by(10),
            KeyCode::Char('u') if ctrl => search.query.clear(),
            KeyCode::Backspace => {
                search.query.pop();
            }
            KeyCode::Char(c)
                if !ctrl
                    && !k.modifiers.contains(KeyModifiers::ALT)
                    && !c.is_control()
                    && search.query.chars().count() < MAX_QUERY =>
            {
                search.query.push(c)
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn search_mouse(&mut self, m: &MouseEvent, hit: Option<Click>) -> Result<()> {
        let Mode::Search(search) = &mut self.mode else {
            return Ok(());
        };
        match m.kind {
            MouseEventKind::ScrollUp => search.move_by(-1),
            MouseEventKind::ScrollDown => search.move_by(1),
            MouseEventKind::Down(MouseButton::Left) => match hit {
                Some(Click::SearchItem(i)) => {
                    search.select(i);
                    return self.resume_picked();
                }
                Some(Click::SearchBackground) => {}
                _ => self.mode = Mode::Normal,
            },
            _ => {}
        }
        Ok(())
    }

    /// Closes the search and goes back to the selected conversation. Stays
    /// open when there's nothing to pick.
    fn resume_picked(&mut self) -> Result<()> {
        let Mode::Search(search) = &self.mode else {
            return Ok(());
        };
        let Some(hit) = search.selected_hit().cloned() else {
            return Ok(());
        };
        self.mode = Mode::Normal;
        self.resume(hit)
    }

    /// Switches to the tab already in this conversation, or opens a new tab
    /// that resumes it, in its folder's workspace (opening that if needed).
    fn resume(&mut self, hit: Hit) -> Result<()> {
        let open = self
            .spaces
            .list
            .iter()
            .flat_map(|ws| &ws.tabs)
            .find(|p| p.session.as_ref().is_some_and(|s| s.id == hit.session.id))
            .map(Pane::id);
        if let Some(id) = open {
            return self.select_pane(id).map(drop);
        }
        let mut pane = self.spawn_pane(&hit.cwd, &hit.session.resume_args())?;
        pane.session = Some(hit.session);
        pane.refresh_transcript_title();
        match self.spaces.list.iter().position(|ws| ws.path == hit.cwd) {
            Some(wi) => self.switch(|s| {
                s.select_workspace(wi);
                s.add_tab(pane);
            }),
            None => self.switch(|s| s.add_workspace(hit.cwd, pane)),
        }
    }
}
