//! The snippet picker and pasting a snippet into the active tab (`ctrl+b i`).

use std::path::Path;

use anyhow::Result;

use super::{App, Mode};
use crate::picker::{Entry, Kind, Picker, Target};
use crate::snippet;
use crate::status::Status;

impl App {
    /// `ctrl+b i`: this workspace's snippets, then the global ones.
    pub(super) fn open_snippet_picker(&mut self) {
        if let Some(entries) = self.snippet_entries() {
            self.mode = Mode::Picker(Picker::new(Kind::Snippets, entries));
        }
    }

    /// The snippet picker's entries, or `None` without a workspace.
    pub(super) fn snippet_entries(&self) -> Option<Vec<Entry>> {
        let ws = self.spaces.current()?;
        let global = snippet::global_dir();
        let entries = snippet::list(&snippet::repo_dir(&ws.path), global.as_deref())
            .into_iter()
            .map(|s| Entry {
                name: s.name,
                detail: format!("{:<6}  {}", s.source.label(), s.preview),
                target: Target::Snippet(s.path),
            })
            .collect();
        Some(entries)
    }

    /// Pastes the snippet without pressing Enter, so it can be edited first.
    pub(super) fn insert_snippet(&mut self, file: &Path) -> Result<()> {
        let text = match snippet::load(file) {
            Ok(text) if !text.is_empty() => text,
            Ok(_) => {
                self.notice = Some(format!("{} is empty", file.display()));
                return Ok(());
            }
            Err(err) => {
                self.notice = Some(format!("can't read {}: {err}", file.display()));
                return Ok(());
            }
        };
        let Some(pane) = self.spaces.active_tab_mut() else {
            return Ok(());
        };
        // A paste into a permission prompt would be lost.
        if pane.status == Status::NeedsInput {
            self.notice = Some("Claude is waiting on you, answer it first".into());
            return Ok(());
        }
        pane.paste(&text)?;
        pane.status = pane.status.seen();
        Ok(())
    }
}
