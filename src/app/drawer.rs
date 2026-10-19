//! Each tab's terminal drawer: a shell in the workspace folder, shown below
//! or beside Claude (`ctrl+b e`), with focus moving between the two.

use anyhow::Result;
use crossterm::event::{MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use super::{App, Mode};
use crate::pane::{Drawer, Pane};
use crate::ui::drawer::{self, Side, Split};

impl App {
    /// Where Claude and a shown drawer go right now, or `None` when the pane
    /// area is too small for both.
    pub(super) fn drawer_split(&self) -> Option<Split> {
        drawer::split(self.pane_area, self.drawer_side, self.drawer_percent)
    }

    /// The active tab's drawer pane, when it's shown and has the keyboard.
    pub(super) fn focused_drawer_mut(&mut self) -> Option<&mut Pane> {
        let d = self.spaces.active_tab_mut()?.drawer.as_mut()?;
        (d.shown && d.focused).then_some(&mut *d.pane)
    }

    /// Whichever pane keys, paste, and copy mode go to.
    pub(super) fn focused_pane_mut(&mut self) -> Option<&mut Pane> {
        if self.focused_drawer_mut().is_some() {
            return self.focused_drawer_mut();
        }
        self.spaces.active_tab_mut()
    }

    pub(super) fn focused_id(&self) -> Option<usize> {
        let tab = self.spaces.active_tab()?;
        match &tab.drawer {
            Some(d) if d.shown && d.focused => Some(d.pane.id()),
            _ => Some(tab.id()),
        }
    }

    /// Any pane by id, drawers included.
    pub(super) fn pane_by_id_mut(&mut self, id: usize) -> Option<&mut Pane> {
        self.spaces.tabs_mut().find_map(|tab| {
            if tab.id() == id {
                return Some(tab);
            }
            tab.drawer.as_mut().map(|d| &mut *d.pane).filter(|p| p.id() == id)
        })
    }

    /// Sends focus-lost to the pane that had the keyboard before and
    /// focus-gained to the one that has it now, if they differ.
    pub(super) fn refocus(&mut self, before: Option<usize>) -> Result<()> {
        let after = self.focused_id();
        if before == after || !self.host_focused {
            return Ok(());
        }
        if let Some(old) = before.and_then(|id| self.pane_by_id_mut(id)) {
            old.focus(false)?;
        }
        if let Some(new) = self.focused_pane_mut() {
            new.focus(true)?;
        }
        Ok(())
    }

    /// Shows or hides the drawer of the tab with this pane id, starting its
    /// shell the first time. The tab becomes the active one.
    pub(super) fn toggle_terminal(&mut self, id: usize) -> Result<()> {
        if !self.select_pane(id)? {
            return Ok(());
        }
        let Some(split) = self.drawer_split() else {
            self.notice = Some("not enough room for a terminal".into());
            return Ok(());
        };
        self.leave_copy_mode()?;
        let before = self.focused_id();
        let needs_shell = self.spaces.active_tab().is_some_and(|t| t.drawer.is_none());
        if needs_shell {
            let Some(cwd) = self.spaces.current().map(|ws| ws.path.clone()) else {
                return Ok(());
            };
            let shell = std::env::var("SHELL").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/bin/sh".into());
            let id = self.next_id;
            self.next_id += 1;
            let pane = Pane::spawn(id, &shell, &[], &[], &cwd, split.drawer, self.cell, self.colors, self.tx.clone())?;
            if let Some(tab) = self.spaces.active_tab_mut() {
                tab.drawer = Some(Drawer {
                    pane: Box::new(pane),
                    shown: true,
                    focused: true,
                });
            }
        } else if let Some(d) = self.spaces.active_tab_mut().and_then(|t| t.drawer.as_mut()) {
            d.shown = !d.shown;
            d.focused = d.shown;
        }
        self.refocus(before)
    }

    /// Moves the keyboard to the active tab's drawer (`true`) or to Claude.
    /// Does nothing when the drawer isn't shown.
    pub(super) fn focus_drawer(&mut self, on: bool) -> Result<()> {
        let shown = self.spaces.active_tab().and_then(|t| t.drawer.as_ref()).is_some_and(|d| d.shown);
        if !shown {
            return Ok(());
        }
        self.leave_copy_mode()?;
        let before = self.focused_id();
        if let Some(d) = self.spaces.active_tab_mut().and_then(|t| t.drawer.as_mut()) {
            d.focused = on;
        }
        self.refocus(before)
    }

    /// `ctrl+b ;`: flips the keyboard between Claude and the drawer.
    pub(super) fn swap_focus(&mut self) -> Result<()> {
        let on_drawer = self.spaces.active_tab().and_then(|t| t.drawer.as_ref()).is_some_and(|d| d.focused);
        self.focus_drawer(!on_drawer)
    }

    /// Ends the shell in the drawer of the tab with this pane id.
    pub(super) fn close_terminal(&mut self, id: usize) -> Result<()> {
        if matches!(self.mode, Mode::Copy) && self.focused_drawer_mut().is_some() {
            self.leave_copy_mode()?;
        }
        let before = self.focused_id();
        let drawer = self.pane_by_id_mut(id).and_then(|tab| tab.drawer.take());
        if let Some(d) = drawer {
            self.forget_pane_images(d.pane.id())?;
        }
        self.refocus(before)
    }

    /// A shell exited on its own. Returns false when `id` isn't a drawer.
    pub(super) fn drawer_exited(&mut self, id: usize) -> Result<bool> {
        let tab = self
            .spaces
            .list
            .iter()
            .flat_map(|ws| &ws.tabs)
            .find(|t| t.drawer.as_ref().is_some_and(|d| d.pane.id() == id))
            .map(Pane::id);
        match tab {
            Some(tab) => self.close_terminal(tab).map(|()| true),
            None => Ok(false),
        }
    }

    /// Mouse events over the pane area: they go to whichever of Claude and
    /// the drawer is under the pointer, and a press there takes the keyboard.
    pub(super) fn pane_mouse(&mut self, m: MouseEvent) -> Result<()> {
        let at = Position::new(m.column, m.row);
        let shown = self.spaces.active_tab().and_then(|t| t.drawer.as_ref()).is_some_and(|d| d.shown);
        let in_drawer = shown && self.drawer_split().is_some_and(|s| s.drawer.contains(at));
        if matches!(m.kind, MouseEventKind::Down(_)) {
            self.focus_drawer(in_drawer)?;
        }
        if in_drawer {
            if let Some(d) = self.spaces.active_tab_mut().and_then(|t| t.drawer.as_mut()) {
                d.pane.mouse(&m)?;
            }
            return Ok(());
        }
        if matches!(self.mode, Mode::Review) {
            if let Some(review) = self.spaces.active_tab_mut().and_then(|p| p.review.as_mut()) {
                let outcome = review.mouse(&m);
                return self.review_outcome(outcome);
            }
            return Ok(());
        }
        match self.spaces.active_tab_mut() {
            Some(pane) => pane.mouse(&m),
            None => Ok(()),
        }
    }

    /// Dragging the divider resizes every drawer.
    pub(super) fn drag_divider(&mut self, m: MouseEvent) {
        self.drawer_percent = drawer::percent_at(self.pane_area, self.drawer_side, m.column, m.row);
    }

    pub(super) fn toggle_terminal_side(&mut self) {
        self.drawer_side = match self.drawer_side {
            Side::Bottom => Side::Right,
            Side::Right => Side::Bottom,
        };
    }
}
