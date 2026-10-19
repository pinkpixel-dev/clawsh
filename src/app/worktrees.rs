//! Git worktrees (`ctrl+b g`): a picker of the active workspace's worktrees,
//! adding one on a new or existing branch, and removing one nothing has open.

use std::path::{Path, PathBuf};

use anyhow::Result;

use super::{App, Mode};
use crate::menu::{Choice, Menu};
use crate::picker::{Entry, Kind, Picker, Target};
use crate::review::git;
use crate::template::display_path;
use crate::worktree::{self, Worktree};

impl App {
    /// Rereads every workspace's branch. It's a couple of small file reads
    /// each, so this runs on tab switches, hook reports, and idle ticks.
    pub(super) fn refresh_heads(&mut self) {
        for ws in &mut self.spaces.list {
            ws.head = worktree::head(&ws.path);
        }
    }

    /// `ctrl+b g`: "+ new branch", then every worktree of the active
    /// workspace's repository.
    pub(super) fn open_worktree_picker(&mut self) {
        let Some(dir) = self.spaces.current().map(|ws| ws.path.clone()) else {
            return;
        };
        let list = match worktree::list(&dir) {
            Ok(list) => list,
            Err(_) => {
                self.notice = Some(format!("{} isn't in a git repository", display_path(&dir)));
                return;
            }
        };
        let from = worktree::head(&dir).map(|h| h.label).unwrap_or_default();
        let mut entries = vec![Entry {
            name: "+ new branch".to_owned(),
            detail: format!("from {from}"),
            target: Target::Command(Choice::NewWorktree(dir)),
        }];
        for wt in list {
            let open = self.open_in(&wt.path);
            let tag = match (wt.main, &open, wt.locked) {
                (true, _, _) => "main",
                (_, Some(_), _) => "open",
                (_, _, true) => "locked",
                _ => "",
            };
            entries.push(Entry {
                name: wt.label(),
                detail: format!("{tag:<6}  {}", display_path(&wt.path)),
                // An open one switches to its workspace, even when that
                // workspace was opened through a symlink.
                target: match open {
                    Some(ws) => Target::Workspace(ws),
                    None => Target::Folder(wt.path),
                },
            });
        }
        self.mode = Mode::Picker(Picker::new(Kind::Worktrees, entries));
    }

    /// The open workspace inside `path`, if any. Paths are compared after
    /// resolving symlinks, since git always reports real paths.
    fn open_in(&self, path: &Path) -> Option<PathBuf> {
        let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let path = real(path);
        self.spaces.list.iter().find(|ws| real(&ws.path).starts_with(&path)).map(|ws| ws.path.clone())
    }

    /// Adds a worktree for `branch` next to the main checkout and opens it.
    pub(super) fn add_worktree(&mut self, dir: &Path, branch: &str) -> Result<()> {
        let added = main_checkout(dir).and_then(|main| worktree::add(dir, &main, branch));
        match added {
            Ok(path) => self.open_workspace(path, &[]),
            Err(err) => {
                self.notice = Some(format!("couldn't add the worktree: {err:#}"));
                Ok(())
            }
        }
    }

    /// `ctrl+x` in the worktree picker. Only a worktree that no workspace
    /// has open can go, and the confirmation shows its uncommitted changes.
    pub(super) fn start_remove_worktree(&mut self, target: Target) {
        self.mode = Mode::Normal;
        let (Target::Folder(path) | Target::Workspace(path)) = target else {
            return;
        };
        // Git runs from the active workspace, which is in the same repository
        // and is never the one being removed, since that one is open.
        let Some(repo) = self.spaces.current().map(|ws| ws.path.clone()) else {
            return;
        };
        let found = worktree::list(&repo).ok().and_then(|list| list.into_iter().find(|w| same(&w.path, &path)));
        let Some(wt) = found else {
            return;
        };
        if let Some(reason) = self.cant_remove(&wt) {
            self.notice = Some(reason);
            return;
        }
        match worktree::changes(&wt.path) {
            Ok(changes) => {
                let menu = Menu::confirm_worktree(self.menu_anchor(), repo, wt.path.clone(), &wt.label(), changes);
                self.mode = Mode::Menu(menu);
            }
            Err(err) => self.notice = Some(format!("couldn't check {}: {err:#}", wt.label())),
        }
    }

    /// Why a worktree has to stay, if it does.
    fn cant_remove(&self, wt: &Worktree) -> Option<String> {
        if wt.main {
            return Some("the main checkout can't be removed".to_owned());
        }
        let ws = self.open_in(&wt.path)?;
        let name = self.spaces.list.iter().find(|w| w.path == ws).map_or_else(String::new, |w| w.name.clone());
        Some(format!("{name} is open, close it first"))
    }

    /// Removes the worktree's folder after the confirmation. The branch stays.
    pub(super) fn remove_worktree(&mut self, repo: &Path, path: &Path, force: bool) {
        // Checked again, since a tab could have opened it in the meantime.
        if let Some(ws) = self.open_in(path) {
            self.notice = Some(format!("{} is open, close it first", display_path(&ws)));
            return;
        }
        self.notice = Some(match worktree::remove(repo, path, force) {
            Ok(()) => format!("removed {}, the branch is still there", display_path(path)),
            Err(err) => format!("couldn't remove the worktree: {err:#}"),
        });
    }
}

/// The main checkout's folder, or the repository top for a bare repository
/// (which has no checkout of its own).
fn main_checkout(dir: &Path) -> Result<PathBuf> {
    match worktree::list(dir)?.into_iter().find(|w| w.main) {
        Some(main) => Ok(main.path),
        None => git::root(dir),
    }
}

fn same(a: &Path, b: &Path) -> bool {
    a == b || matches!((std::fs::canonicalize(a), std::fs::canonicalize(b)), (Ok(x), Ok(y)) if x == y)
}
