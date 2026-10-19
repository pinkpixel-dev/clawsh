//! Restoring the last session on launch and saving it as things change.

use std::path::PathBuf;

use anyhow::Result;

use super::App;
use crate::persist::{self, Saved};

impl App {
    /// Brings back the last session's workspaces, each tab resuming its own
    /// conversation. Folders that no longer exist are skipped.
    pub(super) fn restore(&mut self, saved: &Saved) -> Result<()> {
        // Saved workspace index -> where it landed, for restoring focus.
        let mut placed = Vec::with_capacity(saved.workspaces.len());
        for ws in &saved.workspaces {
            if !ws.path.is_dir() || ws.tabs.is_empty() {
                placed.push(None);
                continue;
            }
            for (ti, tab) in ws.tabs.iter().enumerate() {
                let args = tab.as_ref().map(|s| s.resume_args()).unwrap_or_default();
                let mut pane = self.spawn_pane(&ws.path, &args)?;
                pane.session = tab.clone();
                if ti == 0 {
                    self.spaces.add_workspace(ws.path.clone(), pane);
                } else {
                    self.spaces.add_tab(pane);
                }
            }
            let wi = self.spaces.list.len() - 1;
            self.spaces.select_tab(wi, ws.active.min(ws.tabs.len() - 1));
            placed.push(Some(wi));
        }
        if let Some(Some(wi)) = placed.get(saved.active) {
            self.spaces.select_workspace(*wi);
        }
        Ok(())
    }

    /// Makes the launch folder the active workspace. Arguments given to
    /// `clawsh` (like `--resume`) start a new tab there even if the folder was
    /// restored, so they're never silently dropped.
    pub(super) fn open_launch_folder(&mut self, path: PathBuf, args: &[String]) -> Result<()> {
        match self.spaces.list.iter().position(|ws| ws.path == path) {
            Some(i) => {
                self.switch(|s| s.select_workspace(i))?;
                if args.is_empty() {
                    Ok(())
                } else {
                    let pane = self.spawn_pane(&path, args)?;
                    self.switch(|s| s.add_tab(pane))
                }
            }
            None => self.open_workspace(path, args),
        }
    }

    /// Saves the session if anything about it changed since the last save.
    /// Saving is best effort: a full disk shouldn't take the app down.
    pub(super) fn save_session(&mut self) {
        let Some(path) = self.session_file.as_deref() else {
            return;
        };
        let saved = Saved::from_spaces(&self.spaces, self.notifications, |p| p.session.clone());
        if self.last_saved.as_ref() == Some(&saved) {
            return;
        }
        if persist::save(path, &saved).is_ok() {
            self.last_saved = Some(saved);
        }
    }
}
