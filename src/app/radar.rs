//! The radar's keys and clicks: move through every tab, jump to one, or jump
//! and review its last turn.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::{App, Mode};
use crate::review::{Review, Scope};
use crate::ui::Click;

impl App {
    /// `ctrl+b v`. Starts on the first row, whatever needs you most.
    pub(super) fn open_radar(&mut self) {
        self.mode = Mode::Radar(None);
    }

    pub(super) fn radar_key(&mut self, k: &KeyEvent) -> Result<()> {
        if k.kind == KeyEventKind::Release {
            return Ok(());
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        match k.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('v') => self.mode = Mode::Normal,
            KeyCode::Enter => return self.radar_jump(false),
            KeyCode::Char('r') => return self.radar_jump(true),
            KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => self.radar_move(-1),
            KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => self.radar_move(1),
            KeyCode::Char('p') if ctrl => self.radar_move(-1),
            KeyCode::Char('n') if ctrl => self.radar_move(1),
            KeyCode::Home => self.radar_move(isize::MIN),
            KeyCode::End => self.radar_move(isize::MAX),
            _ => {}
        }
        Ok(())
    }

    pub(super) fn radar_mouse(&mut self, m: &MouseEvent, hit: Option<Click>) -> Result<()> {
        match m.kind {
            MouseEventKind::ScrollUp => self.radar_move(-1),
            MouseEventKind::ScrollDown => self.radar_move(1),
            MouseEventKind::Down(MouseButton::Left) => match hit {
                Some(Click::RadarItem(id)) => {
                    self.mode = Mode::Radar(Some(id));
                    return self.radar_jump(false);
                }
                Some(Click::RadarBackground) => {}
                _ => self.mode = Mode::Normal,
            },
            _ => {}
        }
        Ok(())
    }

    /// Moves the selection through the order the radar last drew, clamped at
    /// both ends.
    fn radar_move(&mut self, by: isize) {
        let Mode::Radar(selected) = &mut self.mode else {
            return;
        };
        let ids = &self.hits.radar;
        let Some(last) = ids.len().checked_sub(1) else {
            return;
        };
        let at = selected.and_then(|id| ids.iter().position(|&i| i == id)).unwrap_or(0);
        let next = at.saturating_add_signed(by).min(last);
        *selected = Some(ids[next]);
    }

    /// Closes the radar and switches to the selected tab. With `review`, also
    /// opens its review on the last turn.
    fn radar_jump(&mut self, review: bool) -> Result<()> {
        let Mode::Radar(selected) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return Ok(());
        };
        let Some(id) = selected.or_else(|| self.hits.radar.first().copied()) else {
            return Ok(());
        };
        if !self.select_pane(id)? || !review {
            return Ok(());
        }
        let dir = self.spaces.current().map(|ws| ws.path.clone());
        if let (Some(dir), Some(pane)) = (dir, self.spaces.active_tab_mut()) {
            pane.review.get_or_insert_with(|| Review::new(dir)).scope = Scope::LastTurn;
        }
        self.open_review();
        Ok(())
    }
}
