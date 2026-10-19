//! Keys, mouse, and paste: what goes to the active pane and what Claw$h
//! handles itself.

use anyhow::Result;
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Position;

use super::{App, Mode};
use crate::picker::{Kind, Picker};
use crate::ui::Click;

/// Sidebar rows moved per wheel tick.
const SIDEBAR_WHEEL: usize = 3;
/// Longest tab or workspace name accepted, in characters.
pub(super) const MAX_NAME: usize = 60;

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
    ToggleNotifications,
    ToggleOrder,
    CopyMode,
    Review,
    Rename,
    OpenTemplate,
    SaveTemplate,
    Snippet,
    Menu,
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
        KeyCode::Char('m') => Action::ToggleNotifications,
        KeyCode::Char('a') => Action::ToggleOrder,
        KeyCode::Char('[') => Action::CopyMode,
        KeyCode::Char('r') => Action::Review,
        KeyCode::Char(',') => Action::Rename,
        // Without kitty's alternate keys, shift+t comes as `t` plus SHIFT.
        KeyCode::Char('t') if k.modifiers.contains(KeyModifiers::SHIFT) => Action::SaveTemplate,
        KeyCode::Char('t') => Action::OpenTemplate,
        KeyCode::Char('T') => Action::SaveTemplate,
        KeyCode::Char('i') => Action::Snippet,
        KeyCode::Char('o') => Action::Menu,
        KeyCode::Char('q') => Action::Quit,
        _ => return None,
    })
}

/// What a key did to a one-line text field.
enum LineEdit {
    Editing,
    Submit,
    Cancel,
}

/// Shared editing keys for the rename and save-as fields.
fn edit_line(text: &mut String, k: &KeyEvent) -> LineEdit {
    let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
    match k.code {
        KeyCode::Esc => return LineEdit::Cancel,
        KeyCode::Enter => return LineEdit::Submit,
        KeyCode::Backspace => {
            text.pop();
        }
        KeyCode::Char('u') if ctrl => text.clear(),
        KeyCode::Char(c) if !ctrl && !k.modifiers.contains(KeyModifiers::ALT) => push_name_char(text, c),
        _ => {}
    }
    LineEdit::Editing
}

fn push_name_char(name: &mut String, c: char) {
    if !c.is_control() && name.chars().count() < MAX_NAME {
        name.push(c);
    }
}

fn is_prefix(k: &KeyEvent) -> bool {
    k.code == KeyCode::Char('b') && k.modifiers == KeyModifiers::CONTROL
}

impl App {
    pub(super) fn key(&mut self, k: KeyEvent) -> Result<()> {
        if k.kind == KeyEventKind::Press {
            self.notice = None;
        }
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
                    Some(pane) => {
                        pane.status = pane.status.seen();
                        pane.key(&k)
                    }
                    None => Ok(()),
                }
            }
            Mode::Prefix => {
                // With kitty's report-all-keys flag, shift on its own arrives
                // as a key press before the key it modifies.
                if k.kind != KeyEventKind::Press || matches!(k.code, KeyCode::Modifier(_)) {
                    return Ok(());
                }
                self.mode = Mode::Normal;
                match prefix_action(&k) {
                    Some(action) => self.run_action(action, &k),
                    None => Ok(()),
                }
            }
            Mode::Copy => {
                let still_copying = match self.spaces.active_tab_mut() {
                    Some(pane) => pane.copy_key(&k)?,
                    None => false,
                };
                if !still_copying {
                    self.mode = Mode::Normal;
                }
                Ok(())
            }
            Mode::Review => {
                let outcome = match self.spaces.active_tab_mut().and_then(|p| p.review.as_mut()) {
                    Some(review) => review.key(&k),
                    None => crate::review::Outcome::Close,
                };
                self.review_outcome(outcome)
            }
            Mode::Rename(text) => {
                if k.kind == KeyEventKind::Release {
                    return Ok(());
                }
                match edit_line(text, &k) {
                    LineEdit::Editing => {}
                    LineEdit::Cancel => self.mode = Mode::Normal,
                    LineEdit::Submit => {
                        // An empty name hands the tab back to auto-titling.
                        let name = text.trim().to_owned();
                        if let Some(pane) = self.spaces.active_tab_mut() {
                            pane.name = (!name.is_empty()).then_some(name);
                        }
                        self.mode = Mode::Normal;
                    }
                }
                Ok(())
            }
            Mode::RenameWorkspace(path, text) => {
                if k.kind == KeyEventKind::Release {
                    return Ok(());
                }
                match edit_line(text, &k) {
                    LineEdit::Editing => {}
                    LineEdit::Cancel => self.mode = Mode::Normal,
                    LineEdit::Submit => {
                        let (path, name) = (path.clone(), std::mem::take(text));
                        self.spaces.rename(&path, &name);
                        self.mode = Mode::Normal;
                    }
                }
                Ok(())
            }
            Mode::Menu(_) => self.menu_key(&k),
            Mode::SaveTemplate(text) => {
                if k.kind == KeyEventKind::Release {
                    return Ok(());
                }
                match edit_line(text, &k) {
                    LineEdit::Editing => {}
                    LineEdit::Cancel => self.mode = Mode::Normal,
                    // An empty name keeps the field open.
                    LineEdit::Submit if text.trim().is_empty() => {}
                    LineEdit::Submit => {
                        let name = std::mem::take(text);
                        self.mode = Mode::Normal;
                        self.save_template(&name);
                    }
                }
                Ok(())
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
            Action::ToggleNotifications => self.notifications = !self.notifications,
            Action::ToggleOrder => self.by_activity = !self.by_activity,
            Action::CopyMode => {
                if let Some(pane) = self.spaces.active_tab_mut() {
                    pane.enter_copy_mode()?;
                    self.mode = Mode::Copy;
                }
            }
            Action::Review => self.open_review(),
            Action::Rename => self.start_rename_tab(),
            Action::OpenTemplate => self.open_template_picker(),
            Action::SaveTemplate => self.start_save_template(),
            Action::Snippet => self.open_snippet_picker(),
            Action::Menu => self.open_main_menu(),
            Action::Quit => self.quit = true,
            Action::SendPrefix => {
                if let Some(pane) = self.spaces.active_tab_mut() {
                    pane.key(key)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn start_rename_tab(&mut self) {
        if let Some(pane) = self.spaces.active_tab_mut() {
            // Start from what the tab shows now, ready to edit.
            self.mode = Mode::Rename(pane.title().chars().take(MAX_NAME).collect());
        }
    }

    pub(super) fn open_picker(&mut self) {
        let extra = std::env::var("CLAWSH_PROJECTS").ok();
        let candidates = Picker::candidates(&self.launch_dir, extra.as_deref());
        self.mode = Mode::Picker(Picker::folders(candidates));
    }

    fn open_picked(&mut self) -> Result<()> {
        let Mode::Picker(picker) = &self.mode else {
            return Ok(());
        };
        let Some(path) = picker.choice() else {
            return Ok(());
        };
        let kind = picker.kind;
        self.mode = Mode::Normal;
        match kind {
            Kind::Folders => self.open_workspace(path, &[]),
            Kind::Templates => self.open_template(&path),
            Kind::Snippets => self.insert_snippet(&path),
        }
    }

    pub(super) fn mouse(&mut self, m: MouseEvent) -> Result<()> {
        let hit = self.hits.at(m.column, m.row);
        let left_click = m.kind == MouseEventKind::Down(MouseButton::Left);
        let right_click = m.kind == MouseEventKind::Down(MouseButton::Right);

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

        if let Mode::Menu(menu) = &mut self.mode {
            match hit {
                Some(Click::MenuItem(i)) if left_click => {
                    menu.select(i);
                    return self.run_menu();
                }
                // Hovering moves the highlight, like a desktop menu.
                Some(Click::MenuItem(i)) => menu.select(i),
                Some(Click::MenuBackground) => {}
                // A click anywhere else closes the menu. A right-click on
                // something else carries on and opens that thing's menu.
                _ if left_click => self.mode = Mode::Normal,
                _ if right_click => {
                    self.mode = Mode::Normal;
                    if let Some(click) = hit {
                        self.open_context_menu(click, (m.column, m.row));
                    }
                }
                _ => {}
            }
            return Ok(());
        }

        // The wheel over the sidebar's list scrolls it; the next draw clamps.
        if let Some(list) = self.hits.sidebar_list
            && list.contains(Position::new(m.column, m.row))
        {
            match m.kind {
                MouseEventKind::ScrollUp => self.sidebar_scroll = self.sidebar_scroll.saturating_sub(SIDEBAR_WHEEL),
                MouseEventKind::ScrollDown => self.sidebar_scroll += SIDEBAR_WHEEL,
                _ => {}
            }
        }

        if let Some(click) = hit {
            if left_click {
                self.leave_copy_mode()?;
                self.mode = Mode::Normal;
                self.click(click)?;
            } else if right_click {
                self.leave_copy_mode()?;
                self.open_context_menu(click, (m.column, m.row));
            }
            return Ok(());
        }
        if self.pane_area.contains(Position::new(m.column, m.row))
            && let Some(pane) = self.spaces.active_tab_mut()
        {
            if matches!(self.mode, Mode::Review) {
                if let Some(review) = &mut pane.review {
                    let outcome = review.mouse(&m);
                    return self.review_outcome(outcome);
                }
            } else {
                pane.mouse(&m)?;
            }
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
            Click::OpenMenu => {
                self.open_main_menu();
                Ok(())
            }
            Click::PickerItem(_) | Click::PickerBackground | Click::MenuItem(_) | Click::MenuBackground => Ok(()),
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
            Mode::Rename(name) | Mode::SaveTemplate(name) | Mode::RenameWorkspace(_, name) => {
                for c in text.lines().next().unwrap_or("").chars() {
                    push_name_char(name, c);
                }
                Ok(())
            }
            Mode::Review => {
                if let Some(review) = self.spaces.active_tab_mut().and_then(|p| p.review.as_mut()) {
                    review.paste(text);
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
    fn names_skip_control_characters_and_stop_at_the_limit() {
        let mut name = String::new();
        push_name_char(&mut name, 'a');
        push_name_char(&mut name, '\x1b');
        assert_eq!(name, "a");
        for _ in 0..100 {
            push_name_char(&mut name, 'b');
        }
        assert_eq!(name.chars().count(), MAX_NAME);
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
        assert_eq!(prefix_action(&key(KeyCode::Char('['))), Some(Action::CopyMode));
        assert_eq!(prefix_action(&key(KeyCode::Char(','))), Some(Action::Rename));
        assert_eq!(prefix_action(&key(KeyCode::Char('a'))), Some(Action::ToggleOrder));
        assert_eq!(prefix_action(&key(KeyCode::Char('r'))), Some(Action::Review));
        assert_eq!(prefix_action(&key(KeyCode::Char('t'))), Some(Action::OpenTemplate));
        assert_eq!(prefix_action(&key(KeyCode::Char('i'))), Some(Action::Snippet));
        assert_eq!(prefix_action(&key(KeyCode::Char('o'))), Some(Action::Menu));
        assert_eq!(
            prefix_action(&KeyEvent::new(KeyCode::Char('T'), KeyModifiers::NONE)),
            Some(Action::SaveTemplate)
        );
        assert_eq!(
            prefix_action(&KeyEvent::new(KeyCode::Char('t'), KeyModifiers::SHIFT)),
            Some(Action::SaveTemplate)
        );
        assert_eq!(prefix_action(&key(KeyCode::Esc)), None);
        assert_eq!(prefix_action(&key(KeyCode::Char('0'))), None);
    }
}
