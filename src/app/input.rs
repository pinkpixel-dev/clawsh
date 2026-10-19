//! Keys, mouse, and paste: what goes to the active pane and what Claw$h
//! handles itself.

use anyhow::Result;
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Position;

use super::{App, Mode};
use crate::picker::Picker;
use crate::ui::Click;

/// What a key after `ctrl+b` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    NewTab,
    NewWorkspace,
    CloseTab,
    CycleTab(isize),
    SelectTab(usize),
    CycleWorkspace(isize),
    ToggleSidebar,
    Quit,
    /// `ctrl+b ctrl+b` sends a literal `ctrl+b` to the pane.
    SendPrefix,
}

pub(super) fn prefix_action(k: &KeyEvent) -> Option<Action> {
    if is_prefix(k) {
        return Some(Action::SendPrefix);
    }
    Some(match k.code {
        KeyCode::Char('c') => Action::NewTab,
        KeyCode::Char('w') => Action::NewWorkspace,
        KeyCode::Char('x') => Action::CloseTab,
        KeyCode::Char('n') | KeyCode::Right => Action::CycleTab(1),
        KeyCode::Char('p') | KeyCode::Left => Action::CycleTab(-1),
        KeyCode::Char(d @ '1'..='9') => Action::SelectTab(d as usize - '1' as usize),
        KeyCode::Char('j') | KeyCode::Down => Action::CycleWorkspace(1),
        KeyCode::Char('k') | KeyCode::Up => Action::CycleWorkspace(-1),
        KeyCode::Char('s') => Action::ToggleSidebar,
        KeyCode::Char('q') => Action::Quit,
        _ => return None,
    })
}

fn is_prefix(k: &KeyEvent) -> bool {
    k.code == KeyCode::Char('b') && k.modifiers == KeyModifiers::CONTROL
}

impl App {
    pub(super) fn key(&mut self, k: KeyEvent) -> Result<()> {
        match &mut self.mode {
            Mode::Normal => {
                if is_prefix(&k) {
                    // The prefix never reaches the pane, press or release.
                    if k.kind == KeyEventKind::Press {
                        self.mode = Mode::Prefix;
                    }
                    return Ok(());
                }
                match self.spaces.active_tab_mut() {
                    Some(pane) => pane.key(&k),
                    None => Ok(()),
                }
            }
            Mode::Prefix => {
                if k.kind != KeyEventKind::Press {
                    return Ok(());
                }
                self.mode = Mode::Normal;
                match prefix_action(&k) {
                    Some(action) => self.run_action(action, &k),
                    None => Ok(()),
                }
            }
            Mode::Picker(picker) => {
                if k.kind == KeyEventKind::Release {
                    return Ok(());
                }
                let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                match k.code {
                    KeyCode::Esc => self.mode = Mode::Normal,
                    KeyCode::Enter => return self.open_picked(),
                    KeyCode::Up => picker.move_by(-1),
                    KeyCode::Down | KeyCode::Tab => picker.move_by(1),
                    KeyCode::Char('p') if ctrl => picker.move_by(-1),
                    KeyCode::Char('n') if ctrl => picker.move_by(1),
                    KeyCode::Backspace => picker.backspace(),
                    KeyCode::Char(c) if !ctrl && !k.modifiers.contains(KeyModifiers::ALT) => {
                        picker.push(c)
                    }
                    _ => {}
                }
                Ok(())
            }
        }
    }

    pub(super) fn run_action(&mut self, action: Action, key: &KeyEvent) -> Result<()> {
        match action {
            Action::NewTab => self.new_tab()?,
            Action::NewWorkspace => self.open_picker(),
            Action::CloseTab => self.close_active_tab()?,
            Action::CycleTab(d) => self.switch(|s| s.cycle_tab(d))?,
            Action::SelectTab(i) => {
                let ws = self.spaces.active;
                self.switch(|s| s.select_tab(ws, i))?
            }
            Action::CycleWorkspace(d) => self.switch(|s| s.cycle_workspace(d))?,
            Action::ToggleSidebar => self.show_sidebar = !self.show_sidebar,
            Action::Quit => self.quit = true,
            Action::SendPrefix => {
                if let Some(pane) = self.spaces.active_tab_mut() {
                    pane.key(key)?;
                }
            }
        }
        Ok(())
    }

    fn open_picker(&mut self) {
        let extra = std::env::var("CLAWSH_PROJECTS").ok();
        let candidates = Picker::candidates(&self.launch_dir, extra.as_deref());
        self.mode = Mode::Picker(Picker::new(candidates));
    }

    fn open_picked(&mut self) -> Result<()> {
        let Mode::Picker(picker) = &self.mode else {
            return Ok(());
        };
        let Some(path) = picker.choice() else {
            return Ok(());
        };
        self.mode = Mode::Normal;
        self.open_workspace(path, &[])
    }

    pub(super) fn mouse(&mut self, m: MouseEvent) -> Result<()> {
        let hit = self.hits.at(m.column, m.row);
        let left_click = m.kind == MouseEventKind::Down(MouseButton::Left);

        if let Mode::Picker(picker) = &mut self.mode {
            match m.kind {
                MouseEventKind::ScrollUp => picker.move_by(-1),
                MouseEventKind::ScrollDown => picker.move_by(1),
                _ if left_click => match hit {
                    Some(Click::PickerItem(i)) => {
                        picker.select(i);
                        return self.open_picked();
                    }
                    Some(Click::PickerBackground) => {}
                    _ => self.mode = Mode::Normal,
                },
                _ => {}
            }
            return Ok(());
        }

        if let Some(click) = hit {
            if left_click {
                self.mode = Mode::Normal;
                self.click(click)?;
            }
            return Ok(());
        }
        if self.pane_area.contains(Position::new(m.column, m.row))
            && let Some(pane) = self.spaces.active_tab_mut()
        {
            pane.mouse(&m)?;
        }
        Ok(())
    }

    fn click(&mut self, click: Click) -> Result<()> {
        match click {
            Click::Workspace(i) => self.switch(|s| s.select_workspace(i)),
            Click::Tab(ws, tab) => self.switch(|s| s.select_tab(ws, tab)),
            Click::NewTab => self.new_tab(),
            Click::NewWorkspace => {
                self.open_picker();
                Ok(())
            }
            Click::PickerItem(_) | Click::PickerBackground => Ok(()),
        }
    }

    pub(super) fn paste(&mut self, text: &str) -> Result<()> {
        match &mut self.mode {
            Mode::Picker(picker) => {
                for c in text.lines().next().unwrap_or("").chars() {
                    picker.push(c);
                }
                Ok(())
            }
            _ => match self.spaces.active_tab_mut() {
                Some(pane) => pane.paste(text),
                None => Ok(()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn prefix_keys_map_to_actions() {
        assert_eq!(prefix_action(&key(KeyCode::Char('c'))), Some(Action::NewTab));
        assert_eq!(prefix_action(&key(KeyCode::Char('3'))), Some(Action::SelectTab(2)));
        assert_eq!(prefix_action(&key(KeyCode::Up)), Some(Action::CycleWorkspace(-1)));
        assert_eq!(
            prefix_action(&KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)),
            Some(Action::SendPrefix)
        );
        assert_eq!(prefix_action(&key(KeyCode::Esc)), None);
        assert_eq!(prefix_action(&key(KeyCode::Char('0'))), None);
    }
}
