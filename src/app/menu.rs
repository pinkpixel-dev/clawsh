//! Opening the pop-up menus and doing what their items say.

use std::path::Path;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::input::MAX_NAME;
use super::{App, Mode};
use crate::menu::{Choice, Menu, Settings};
use crate::pane::Pane;
use crate::ui::Click;
use crate::ui::drawer::Side;
use crate::workspace::Workspace;

impl App {
    /// The footer button's menu, also on `ctrl+b o`.
    pub(super) fn open_main_menu(&mut self) {
        self.mode = Mode::Menu(self.main_menu());
    }

    /// The main menu's items, which the palette lists as commands too.
    pub(super) fn main_menu(&self) -> Menu {
        let settings = Settings {
            by_activity: self.by_activity,
            terminal_right: self.drawer_side == Side::Right,
            notifications: self.notifications,
            sidebar: self.show_sidebar,
        };
        let workspace = self.spaces.current().map(|ws| ws.path.clone());
        let tab = self.spaces.active_tab().map(|t| (t.id(), t.drawer.as_ref().map(|d| d.shown)));
        Menu::main(self.menu_anchor(), workspace, tab, &settings)
    }

    /// The footer sits right under the pane, so menus opened from here sit
    /// just above the menu button.
    pub(super) fn menu_anchor(&self) -> (u16, u16) {
        (0, self.pane_area.bottom())
    }

    /// A right-click on the chrome. Only tabs and workspaces have menus.
    pub(super) fn open_context_menu(&mut self, click: Click, at: (u16, u16)) {
        let menu = match click {
            Click::Tab(wi, ti) => {
                let Some(ws) = self.spaces.list.get(wi) else { return };
                let Some(pane) = ws.tabs.get(ti) else { return };
                Menu::for_tab(at, pane.id(), ws.path.clone(), pane.drawer.as_ref().map(|d| d.shown))
            }
            Click::Workspace(wi) => match self.spaces.list.get(wi) {
                Some(ws) => Menu::for_workspace(at, ws.path.clone()),
                None => return,
            },
            Click::OpenMenu => return self.open_main_menu(),
            _ => return,
        };
        self.mode = Mode::Menu(menu);
    }

    pub(super) fn menu_key(&mut self, k: &KeyEvent) -> Result<()> {
        if k.kind == KeyEventKind::Release {
            return Ok(());
        }
        let Mode::Menu(menu) = &mut self.mode else {
            return Ok(());
        };
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        // The `ctrl+b` key shown next to an item picks it here too. Without
        // kitty's alternate keys, shift+t comes as `t` plus SHIFT.
        if let KeyCode::Char(c) = k.code
            && !ctrl
        {
            let c = if k.modifiers.contains(KeyModifiers::SHIFT) { c.to_ascii_uppercase() } else { c };
            if let Some(i) = menu.items.iter().position(|item| item.key.is_some_and(|key| key.chars().eq([c]))) {
                menu.select(i);
                return self.run_menu();
            }
        }
        match k.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter | KeyCode::Char(' ') => return self.run_menu(),
            KeyCode::Up | KeyCode::BackTab | KeyCode::Char('k') => menu.move_by(-1),
            KeyCode::Down | KeyCode::Tab | KeyCode::Char('j') => menu.move_by(1),
            KeyCode::Char('p') if ctrl => menu.move_by(-1),
            KeyCode::Char('n') if ctrl => menu.move_by(1),
            KeyCode::Home => menu.select(0),
            KeyCode::End => menu.select(menu.items.len().saturating_sub(1)),
            _ => {}
        }
        Ok(())
    }

    /// Closes the menu and does what its selected item says.
    pub(super) fn run_menu(&mut self) -> Result<()> {
        let Mode::Menu(menu) = std::mem::replace(&mut self.mode, Mode::Normal) else {
            return Ok(());
        };
        match menu.choice().cloned() {
            Some(choice) => self.run_choice(choice, menu.anchor),
            None => Ok(()),
        }
    }

    /// Does what a menu item says. A confirmation opens at `at`.
    pub(super) fn run_choice(&mut self, choice: Choice, at: (u16, u16)) -> Result<()> {
        match choice {
            Choice::NewTab(path) => {
                if let Some(wi) = self.spaces.list.iter().position(|ws| ws.path == path) {
                    self.switch(|s| s.select_workspace(wi))?;
                    self.new_tab()?;
                }
            }
            Choice::RenameTab(id) => {
                if self.select_pane(id)? {
                    self.start_rename_tab();
                }
            }
            Choice::CloseTab(id) => {
                if let Some(pane) = self.spaces.list.iter().flat_map(|ws| &ws.tabs).find(|p| p.id() == id) {
                    self.mode = Mode::Menu(Menu::confirm_tab(at, id, &pane.title()));
                }
            }
            Choice::ConfirmCloseTab(id) => self.close_pane(id)?,
            Choice::RenameWorkspace(path) => {
                if let Some(ws) = self.workspace_at(&path) {
                    let name = ws.name.chars().take(MAX_NAME).collect();
                    self.mode = Mode::RenameWorkspace(path, name);
                }
            }
            Choice::CloseWorkspace(path) => {
                if let Some(ws) = self.workspace_at(&path) {
                    let (name, tabs) = (ws.name.clone(), ws.tabs.len());
                    self.mode = Mode::Menu(Menu::confirm_workspace(at, path, &name, tabs));
                }
            }
            Choice::ConfirmCloseWorkspace(path) => {
                let ids: Vec<usize> = self.workspace_at(&path).map(|ws| ws.tabs.iter().map(Pane::id).collect()).unwrap_or_default();
                for id in ids {
                    self.close_pane(id)?;
                }
            }
            Choice::ToggleTerminal(id) => self.toggle_terminal(id)?,
            Choice::CloseTerminal(id) => self.mode = Mode::Menu(Menu::confirm_terminal(at, id)),
            Choice::ConfirmCloseTerminal(id) => self.close_terminal(id)?,
            Choice::ToggleTerminalSide => self.toggle_terminal_side(),
            Choice::Cancel => {}
            Choice::Palette => self.open_palette(),
            Choice::CopyMode => self.start_copy_mode()?,
            Choice::NewWorkspace => self.open_picker(),
            Choice::OpenTemplate => self.open_template_picker(),
            Choice::SaveTemplate => self.start_save_template(),
            Choice::Snippet => self.open_snippet_picker(),
            Choice::Review => self.open_review(),
            Choice::Radar => self.open_radar(),
            Choice::ToggleOrder => self.by_activity = !self.by_activity,
            Choice::ToggleNotifications => self.notifications = !self.notifications,
            Choice::ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            Choice::Quit => self.quit = true,
        }
        Ok(())
    }

    fn workspace_at(&self, path: &Path) -> Option<&Workspace<Pane>> {
        self.spaces.list.iter().find(|ws| ws.path == path)
    }

    /// Switches to the tab with this pane id. `false` if it's gone.
    pub(super) fn select_pane(&mut self, id: usize) -> Result<bool> {
        let found = self.spaces.list.iter().enumerate().find_map(|(wi, ws)| {
            ws.tabs.iter().position(|p| p.id() == id).map(|ti| (wi, ti))
        });
        let Some((wi, ti)) = found else {
            return Ok(false);
        };
        self.switch(|s| s.select_tab(wi, ti))?;
        Ok(true)
    }

    /// Closes one tab wherever it is, without switching to it first.
    fn close_pane(&mut self, id: usize) -> Result<()> {
        self.switch(|s| {
            s.remove_tab(|p| p.id() == id);
        })?;
        self.forget_images(id)
    }
}
