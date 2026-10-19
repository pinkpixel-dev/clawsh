//! Workspaces (one per folder) and the tabs inside them.
//!
//! Generic over the tab type so the index bookkeeping can be tested without
//! spawning real programs. The app uses `Workspaces<Pane>`.

use std::path::{Path, PathBuf};

pub struct Workspace<T> {
    pub name: String,
    pub path: PathBuf,
    pub tabs: Vec<T>,
    pub active: usize,
}

pub struct Workspaces<T> {
    pub list: Vec<Workspace<T>>,
    pub active: usize,
}

impl<T> Workspaces<T> {
    pub fn new() -> Self {
        Self {
            list: Vec::new(),
            active: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// Adds a workspace with its first tab and switches to it.
    pub fn add_workspace(&mut self, path: PathBuf, first_tab: T) {
        self.list.push(Workspace {
            name: folder_name(&path),
            path,
            tabs: vec![first_tab],
            active: 0,
        });
        self.active = self.list.len() - 1;
    }

    /// Adds a tab to the active workspace and switches to it.
    pub fn add_tab(&mut self, tab: T) {
        if let Some(ws) = self.list.get_mut(self.active) {
            ws.tabs.push(tab);
            ws.active = ws.tabs.len() - 1;
        }
    }

    pub fn current(&self) -> Option<&Workspace<T>> {
        self.list.get(self.active)
    }

    pub fn active_tab(&self) -> Option<&T> {
        let ws = self.list.get(self.active)?;
        ws.tabs.get(ws.active)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut T> {
        let ws = self.list.get_mut(self.active)?;
        ws.tabs.get_mut(ws.active)
    }

    pub fn select_workspace(&mut self, index: usize) {
        if index < self.list.len() {
            self.active = index;
        }
    }

    pub fn select_tab(&mut self, workspace: usize, tab: usize) {
        if let Some(ws) = self.list.get_mut(workspace)
            && tab < ws.tabs.len()
        {
            ws.active = tab;
            self.active = workspace;
        }
    }

    /// Moves to the next (`delta > 0`) or previous tab, wrapping around.
    pub fn cycle_tab(&mut self, delta: isize) {
        if let Some(ws) = self.list.get_mut(self.active) {
            ws.active = wrap(ws.active, delta, ws.tabs.len());
        }
    }

    pub fn cycle_workspace(&mut self, delta: isize) {
        self.active = wrap(self.active, delta, self.list.len());
    }

    /// Removes the first tab matching `pred`. A workspace left with no tabs
    /// is removed too. Returns the removed tab.
    pub fn remove_tab(&mut self, pred: impl Fn(&T) -> bool) -> Option<T> {
        let (wi, ti) = self.list.iter().enumerate().find_map(|(wi, ws)| {
            ws.tabs.iter().position(&pred).map(|ti| (wi, ti))
        })?;
        let ws = &mut self.list[wi];
        let tab = ws.tabs.remove(ti);
        if ti < ws.active || ws.active >= ws.tabs.len() {
            ws.active = ws.active.saturating_sub(1);
        }
        if ws.tabs.is_empty() {
            self.list.remove(wi);
            if wi < self.active || self.active >= self.list.len() {
                self.active = self.active.saturating_sub(1);
            }
        }
        Some(tab)
    }

    /// Removes the active tab (and its workspace if it was the last one).
    pub fn remove_active_tab(&mut self) -> Option<T> {
        let ws = self.list.get(self.active)?;
        let target = std::ptr::from_ref(ws.tabs.get(ws.active)?);
        self.remove_tab(|t| std::ptr::eq(t, target))
    }

    pub fn tabs_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.list.iter_mut().flat_map(|ws| ws.tabs.iter_mut())
    }

    pub fn find_mut(&mut self, pred: impl Fn(&T) -> bool) -> Option<&mut T> {
        self.tabs_mut().find(|t| pred(t))
    }
}

fn wrap(index: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (index as isize + delta).rem_euclid(len as isize) as usize
}

pub fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Workspaces<u32> {
        let mut w = Workspaces::new();
        w.add_workspace(PathBuf::from("/p/quota"), 1);
        w.add_tab(2);
        w.add_tab(3);
        w.add_workspace(PathBuf::from("/p/slate"), 4);
        w
    }

    #[test]
    fn new_things_become_active() {
        let mut w = sample();
        assert_eq!(w.current().unwrap().name, "slate");
        assert_eq!(w.active_tab(), Some(&4));
        w.select_workspace(0);
        assert_eq!(w.active_tab(), Some(&3), "workspace remembers its own tab");
    }

    #[test]
    fn cycling_wraps() {
        let mut w = sample();
        w.select_tab(0, 2);
        w.cycle_tab(1);
        assert_eq!(w.active_tab(), Some(&1));
        w.cycle_tab(-1);
        assert_eq!(w.active_tab(), Some(&3));
        w.cycle_workspace(1);
        assert_eq!(w.active, 1);
        w.cycle_workspace(1);
        assert_eq!(w.active, 0);
    }

    #[test]
    fn removing_a_tab_keeps_the_right_one_active() {
        let mut w = sample();
        w.select_tab(0, 2);
        assert_eq!(w.remove_tab(|&t| t == 1), Some(1));
        assert_eq!(w.active_tab(), Some(&3), "still on the same tab");
        assert_eq!(w.remove_active_tab(), Some(3));
        assert_eq!(w.active_tab(), Some(&2));
    }

    #[test]
    fn empty_workspaces_disappear() {
        let mut w = sample();
        w.select_workspace(0);
        assert_eq!(w.remove_tab(|&t| t == 4), Some(4));
        assert_eq!(w.list.len(), 1);
        assert_eq!(w.active, 0);
        for t in [1, 2, 3] {
            w.remove_tab(|&x| x == t);
        }
        assert!(w.is_empty());
        assert_eq!(w.active_tab(), None);
    }

    #[test]
    fn removing_an_earlier_workspace_keeps_focus() {
        let mut w = sample();
        // Active is "slate" (index 1); drop all of "quota".
        for t in [1, 2, 3] {
            w.remove_tab(|&x| x == t);
        }
        assert_eq!(w.current().unwrap().name, "slate");
        assert_eq!(w.active_tab(), Some(&4));
    }
}
