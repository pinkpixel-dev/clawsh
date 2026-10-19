//! Small pop-up menus: right-clicking a tab or workspace, the footer's menu
//! button (`ctrl+b o`), and the "are you sure" step before closing things.

use std::path::PathBuf;

/// What picking an item does. Tabs are named by pane id and workspaces by
/// folder, so a target that goes away while the menu is open is just skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    NewTab(PathBuf),
    RenameTab(usize),
    /// Asks first.
    CloseTab(usize),
    RenameWorkspace(PathBuf),
    /// Asks first.
    CloseWorkspace(PathBuf),
    ConfirmCloseTab(usize),
    ConfirmCloseWorkspace(PathBuf),
    /// Shows or hides a tab's terminal drawer, starting a shell if needed.
    ToggleTerminal(usize),
    /// Asks first.
    CloseTerminal(usize),
    ConfirmCloseTerminal(usize),
    /// Drawers on the right instead of the bottom, or back.
    ToggleTerminalSide,
    Cancel,
    /// Opens the command palette.
    Palette,
    /// Copy mode in the focused pane.
    CopyMode,
    NewWorkspace,
    OpenTemplate,
    SaveTemplate,
    Snippet,
    Review,
    /// Every tab at a glance.
    Radar,
    /// Search every saved conversation.
    Search,
    ToggleOrder,
    ToggleNotifications,
    ToggleSidebar,
    Quit,
}

pub struct Item {
    pub label: String,
    /// The `ctrl+b` key that does the same thing, shown faint on the right.
    pub key: Option<&'static str>,
    pub choice: Choice,
    pub danger: bool,
    /// Draw a divider above this item.
    pub divider: bool,
}

impl Item {
    fn new(label: impl Into<String>, choice: Choice) -> Self {
        Self {
            label: label.into(),
            key: None,
            choice,
            danger: false,
            divider: false,
        }
    }

    fn key(mut self, key: &'static str) -> Self {
        self.key = Some(key);
        self
    }

    fn danger(mut self) -> Self {
        self.danger = true;
        self
    }

    fn divider(mut self) -> Self {
        self.divider = true;
        self
    }
}

/// The toggles the main menu labels depend on.
pub struct Settings {
    pub by_activity: bool,
    pub terminal_right: bool,
    pub notifications: bool,
    pub sidebar: bool,
}

pub struct Menu {
    pub title: Option<String>,
    pub items: Vec<Item>,
    pub selected: usize,
    /// The cell it was opened from. The menu opens just below it, or above
    /// when there's no room.
    pub anchor: (u16, u16),
}

impl Menu {
    fn new(anchor: (u16, u16), items: Vec<Item>) -> Self {
        Self {
            title: None,
            items,
            selected: 0,
            anchor,
        }
    }

    /// `terminal` is `None` when the tab has no drawer, otherwise whether
    /// it's shown.
    pub fn for_tab(anchor: (u16, u16), id: usize, workspace: PathBuf, terminal: Option<bool>) -> Self {
        let mut items = vec![
            Item::new("New tab", Choice::NewTab(workspace)),
            Item::new("Rename", Choice::RenameTab(id)),
            Item::new(terminal_label(terminal), Choice::ToggleTerminal(id)).divider(),
        ];
        if terminal.is_some() {
            items.push(Item::new("Close terminal", Choice::CloseTerminal(id)).danger());
        }
        items.push(Item::new("Close", Choice::CloseTab(id)).danger().divider());
        Self::new(anchor, items)
    }

    pub fn for_workspace(anchor: (u16, u16), path: PathBuf) -> Self {
        Self::new(
            anchor,
            vec![
                Item::new("New tab", Choice::NewTab(path.clone())),
                Item::new("Rename", Choice::RenameWorkspace(path.clone())),
                Item::new("Close", Choice::CloseWorkspace(path)).danger(),
            ],
        )
    }

    /// Everything Claw$h can do outside a pane, with the active workspace as
    /// the target for rename and close and the active tab (its pane id and
    /// drawer state) for the terminal.
    pub fn main(
        anchor: (u16, u16),
        workspace: Option<PathBuf>,
        tab: Option<(usize, Option<bool>)>,
        s: &Settings,
    ) -> Self {
        let mut items = vec![
            Item::new("Command palette", Choice::Palette).key("space"),
            Item::new("New workspace", Choice::NewWorkspace).key("w").divider(),
        ];
        if let Some(path) = workspace {
            items.push(Item::new("Rename workspace", Choice::RenameWorkspace(path.clone())));
            items.push(Item::new("Close workspace", Choice::CloseWorkspace(path)).danger());
        }
        items.extend([
            Item::new("Open template", Choice::OpenTemplate).key("t").divider(),
            Item::new("Save template", Choice::SaveTemplate).key("T"),
            Item::new("Insert snippet", Choice::Snippet).key("i"),
            Item::new("Review changes", Choice::Review).key("r"),
            Item::new("Radar", Choice::Radar).key("v"),
            Item::new("Search conversations", Choice::Search).key("/"),
        ]);
        if tab.is_some() {
            items.push(Item::new("Copy mode", Choice::CopyMode).key("["));
        }
        let side = Item::new(
            if s.terminal_right { "Terminal at the bottom" } else { "Terminal on the right" },
            Choice::ToggleTerminalSide,
        );
        match tab {
            Some((id, terminal)) => items.extend([
                Item::new(terminal_label(terminal), Choice::ToggleTerminal(id)).key("e").divider(),
                side,
            ]),
            None => items.push(side.divider()),
        }
        items.extend([
            Item::new(if s.by_activity { "Sort by open order" } else { "Sort by activity" }, Choice::ToggleOrder)
                .key("a")
                .divider(),
            Item::new(if s.notifications { "Mute notifications" } else { "Unmute notifications" }, Choice::ToggleNotifications)
                .key("m"),
            Item::new(if s.sidebar { "Hide sidebar" } else { "Show sidebar" }, Choice::ToggleSidebar).key("s"),
            Item::new("Quit", Choice::Quit).key("q").divider(),
        ]);
        Self::new(anchor, items)
    }

    /// Asks before closing a tab. Starts on Cancel so a stray enter is safe.
    pub fn confirm_tab(anchor: (u16, u16), id: usize, title: &str) -> Self {
        let mut menu = Self::new(
            anchor,
            vec![
                Item::new("Close tab", Choice::ConfirmCloseTab(id)).danger(),
                Item::new("Cancel", Choice::Cancel),
            ],
        );
        menu.title = Some(format!(" close {}? ", short(title)));
        menu.selected = 1;
        menu
    }

    /// Asks before closing a workspace and every tab in it.
    pub fn confirm_workspace(anchor: (u16, u16), path: PathBuf, name: &str, tabs: usize) -> Self {
        let label = if tabs == 1 { "Close 1 tab".to_owned() } else { format!("Close {tabs} tabs") };
        let mut menu = Self::new(
            anchor,
            vec![
                Item::new(label, Choice::ConfirmCloseWorkspace(path)).danger(),
                Item::new("Cancel", Choice::Cancel),
            ],
        );
        menu.title = Some(format!(" close {}? ", short(name)));
        menu.selected = 1;
        menu
    }

    /// Asks before closing a tab's terminal, which ends whatever runs in it.
    pub fn confirm_terminal(anchor: (u16, u16), id: usize) -> Self {
        let mut menu = Self::new(
            anchor,
            vec![
                Item::new("Close terminal", Choice::ConfirmCloseTerminal(id)).danger(),
                Item::new("Cancel", Choice::Cancel),
            ],
        );
        menu.title = Some(" close terminal? ".to_owned());
        menu.selected = 1;
        menu
    }

    /// Moves the selection, wrapping around.
    pub fn move_by(&mut self, delta: isize) {
        let len = self.items.len() as isize;
        if len > 0 {
            self.selected = (self.selected as isize + delta).rem_euclid(len) as usize;
        }
    }

    pub fn select(&mut self, index: usize) {
        if index < self.items.len() {
            self.selected = index;
        }
    }

    pub fn choice(&self) -> Option<&Choice> {
        self.items.get(self.selected).map(|i| &i.choice)
    }
}

fn terminal_label(terminal: Option<bool>) -> &'static str {
    if terminal == Some(true) { "Hide terminal" } else { "Show terminal" }
}

/// Longest name shown in a confirmation title, in characters.
const MAX_TITLE: usize = 24;

fn short(name: &str) -> String {
    if name.chars().count() <= MAX_TITLE {
        return name.to_owned();
    }
    let mut out: String = name.chars().take(MAX_TITLE - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(on: bool) -> Settings {
        Settings {
            by_activity: on,
            terminal_right: on,
            notifications: on,
            sidebar: on,
        }
    }

    #[test]
    fn selection_wraps_both_ways() {
        let mut m = Menu::for_tab((0, 0), 3, PathBuf::from("/p"), None);
        m.move_by(-1);
        assert_eq!(m.choice(), Some(&Choice::CloseTab(3)));
        m.move_by(1);
        assert_eq!(m.choice(), Some(&Choice::NewTab(PathBuf::from("/p"))));
        m.select(99);
        assert_eq!(m.selected, 0, "out of range is ignored");
    }

    #[test]
    fn tab_menu_offers_closing_a_terminal_only_when_there_is_one() {
        let labels = |t| Menu::for_tab((0, 0), 1, PathBuf::from("/p"), t).items.into_iter().map(|i| i.label).collect::<Vec<_>>();
        assert_eq!(labels(None), ["New tab", "Rename", "Show terminal", "Close"]);
        assert_eq!(labels(Some(false)), ["New tab", "Rename", "Show terminal", "Close terminal", "Close"]);
        assert_eq!(labels(Some(true))[2], "Hide terminal");
        assert_eq!(Menu::confirm_terminal((0, 0), 1).choice(), Some(&Choice::Cancel));
    }

    #[test]
    fn confirmations_start_on_cancel() {
        assert_eq!(Menu::confirm_tab((0, 0), 1, "x").choice(), Some(&Choice::Cancel));
        let m = Menu::confirm_workspace((0, 0), PathBuf::from("/p"), "p", 3);
        assert_eq!(m.choice(), Some(&Choice::Cancel));
        assert_eq!(m.items[0].label, "Close 3 tabs");
        assert_eq!(m.title.as_deref(), Some(" close p? "));
        let long = Menu::confirm_tab((0, 0), 1, &"x".repeat(40));
        assert_eq!(long.title.unwrap().chars().count(), MAX_TITLE + 9);
    }

    #[test]
    fn main_menu_labels_follow_the_toggles() {
        let labels = |m: &Menu| m.items.iter().map(|i| i.label.clone()).collect::<Vec<_>>();
        let on = labels(&Menu::main((0, 0), None, None, &settings(true)));
        assert!(on.contains(&"Terminal at the bottom".to_owned()));
        assert!(!on.iter().any(|l| l.ends_with("terminal")), "no tab, no toggle");
        assert!(!on.contains(&"Copy mode".to_owned()), "no tab, nothing to copy");
        assert!(on.contains(&"Sort by open order".to_owned()));
        assert!(on.contains(&"Mute notifications".to_owned()));
        assert!(on.contains(&"Hide sidebar".to_owned()));
        assert!(!on.contains(&"Close workspace".to_owned()), "no workspace, nothing to close");
        assert_eq!(on[0], "Command palette");
        let off = labels(&Menu::main((0, 0), Some(PathBuf::from("/p")), Some((1, Some(true))), &settings(false)));
        assert!(off.contains(&"Hide terminal".to_owned()));
        assert!(off.contains(&"Copy mode".to_owned()));
        assert!(off.contains(&"Terminal on the right".to_owned()));
        assert!(off.contains(&"Sort by activity".to_owned()));
        assert!(off.contains(&"Unmute notifications".to_owned()));
        assert!(off.contains(&"Show sidebar".to_owned()));
        assert!(off.contains(&"Close workspace".to_owned()));
    }
}
