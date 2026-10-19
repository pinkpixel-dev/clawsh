//! Opening and saving workspace templates (`ctrl+b t`, `ctrl+b T`).

use std::path::Path;

use anyhow::Result;

use super::{App, Mode};
use crate::picker::{Entry, Kind, Picker};
use crate::template::{self, Tab, Template};

impl App {
    /// `ctrl+b t`: a picker of every saved template.
    pub(super) fn open_template_picker(&mut self) {
        let Some(dir) = template::default_dir() else {
            return;
        };
        let entries = template::list(&dir)
            .into_iter()
            .map(|(path, t)| match t {
                Ok(t) => Entry {
                    detail: match t.tabs.len() {
                        0 | 1 => t.path.clone(),
                        n => format!("{n} tabs  {}", t.path),
                    },
                    name: t.name,
                    path,
                },
                Err(err) => Entry {
                    name: path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
                    detail: err,
                    path,
                },
            })
            .collect();
        self.mode = Mode::Picker(Picker::new(Kind::Templates, entries));
    }

    /// Opens a template as a new workspace. If its folder is already open,
    /// this just switches there, same as opening the folder.
    pub(super) fn open_template(&mut self, file: &Path) -> Result<()> {
        let t = match template::load(file) {
            Ok(t) => t,
            Err(err) => {
                self.notice = Some(format!("{:#}", err.root_cause()));
                return Ok(());
            }
        };
        let folder = t.folder();
        if !folder.is_dir() {
            self.notice = Some(format!("{}: no folder at {}", t.name, t.path));
            return Ok(());
        }
        if let Some(i) = self.spaces.list.iter().position(|ws| ws.path == folder) {
            return self.switch(|s| s.select_workspace(i));
        }
        for (i, tab) in t.tabs_or_default().into_iter().enumerate() {
            let mut pane = self.spawn_pane(&folder, &tab.spawn_args())?;
            pane.name = tab.name.as_deref().map(str::trim).filter(|n| !n.is_empty()).map(str::to_owned);
            pane.from_template = Some(tab);
            if i == 0 {
                self.switch(|s| s.add_workspace(folder.clone(), pane))?;
            } else {
                self.switch(|s| s.add_tab(pane))?;
            }
        }
        // Land on the first tab, the way the template lists them.
        let wi = self.spaces.active;
        self.switch(|s| s.select_tab(wi, 0))
    }

    /// `ctrl+b T`: ask for a name, starting from the workspace's.
    pub(super) fn start_save_template(&mut self) {
        if let Some(ws) = self.spaces.current() {
            self.mode = Mode::SaveTemplate(ws.name.clone());
        }
    }

    /// Saves the current workspace: its folder, and each tab's name (only
    /// ones you gave it), flags, and prompt. Conversations aren't saved, so
    /// a template always starts fresh.
    pub(super) fn save_template(&mut self, name: &str) {
        let (Some(ws), Some(dir)) = (self.spaces.current(), template::default_dir()) else {
            return;
        };
        let t = Template {
            name: name.trim().to_owned(),
            path: template::display_path(&ws.path),
            tabs: ws
                .tabs
                .iter()
                .map(|p| Tab {
                    name: p.name.clone(),
                    ..p.from_template.clone().unwrap_or_default()
                })
                .collect(),
        };
        self.notice = Some(match template::save(&dir, &t) {
            Ok(path) => format!("saved template {} to {}", t.name, template::display_path(&path)),
            Err(err) => format!("couldn't save the template: {err:#}"),
        });
    }
}
