//! Opening the review view and acting on what it asks for.

use anyhow::Result;

use super::{App, Mode};
use crate::review::{Outcome, Review};
use crate::status::Status;

impl App {
    /// `ctrl+b r`: review the active tab's folder, keeping any comments from
    /// the last time it was open.
    pub(super) fn open_review(&mut self) {
        let Some(dir) = self.spaces.current().map(|ws| ws.path.clone()) else {
            return;
        };
        if let Some(pane) = self.spaces.active_tab_mut() {
            pane.review.get_or_insert_with(|| Review::new(dir));
            self.mode = Mode::Review;
            self.refresh_review();
        }
    }

    /// Takes a fresh snapshot for the active tab's review.
    pub(super) fn refresh_review(&mut self) {
        if let Some(pane) = self.spaces.active_tab_mut() {
            let turn = pane.turn_start.clone();
            if let Some(review) = &mut pane.review {
                review.refresh(turn.as_deref());
            }
        }
    }

    pub(super) fn review_outcome(&mut self, outcome: Outcome) -> Result<()> {
        match outcome {
            Outcome::Stay => {}
            Outcome::Close => self.mode = Mode::Normal,
            Outcome::Refresh => self.refresh_review(),
            Outcome::Send(text) => {
                let Some(pane) = self.spaces.active_tab_mut() else {
                    return Ok(());
                };
                // A paste into a permission prompt would be lost.
                if pane.status == Status::NeedsInput {
                    if let Some(review) = &mut pane.review {
                        review.notice = Some("Claude is waiting on you, answer it first".into());
                    }
                    return Ok(());
                }
                pane.paste(&text)?;
                pane.status = pane.status.seen();
                if let Some(review) = &mut pane.review {
                    review.comments.clear();
                }
                self.mode = Mode::Normal;
            }
        }
        Ok(())
    }
}
