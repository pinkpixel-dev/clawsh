//! The command palette (`ctrl+b space`): commands, tabs, workspaces,
//! snippets, and templates in one fuzzy picker.

use anyhow::Result;

use super::templates::template_entries;
use super::{App, Mode};
use crate::menu::Choice;
use crate::picker::{Entry, Kind, Picker, Target};
use crate::template::display_path;

impl App {
    /// Lists commands first, then every tab, workspace, snippet, and template.
    pub(super) fn open_palette(&mut self) {
        let mut entries = self.palette_commands();
        for ws in &self.spaces.list {
            entries.extend(ws.tabs.iter().map(|pane| Entry {
                name: pane.title(),
                detail: labeled("tab", &ws.name),
                target: Target::Tab(pane.id()),
            }));
        }
        entries.extend(self.spaces.list.iter().map(|ws| Entry {
            name: ws.name.clone(),
            detail: labeled("workspace", &display_path(&ws.path)),
            target: Target::Workspace(ws.path.clone()),
        }));
        for (label, more) in [("snippet", self.snippet_entries()), ("template", template_entries())] {
            entries.extend(more.unwrap_or_default().into_iter().map(|e| Entry {
                detail: labeled(label, &e.detail),
                ..e
            }));
        }
        self.mode = Mode::Picker(Picker::new(Kind::Palette, entries));
    }

    /// The active tab's commands, then the main menu's, each with its key.
    fn palette_commands(&self) -> Vec<Entry> {
        let mut commands = Vec::new();
        if let Some(ws) = self.spaces.current() {
            commands.push(("New tab".to_owned(), Some("c"), Choice::NewTab(ws.path.clone())));
        }
        if let Some(pane) = self.spaces.active_tab() {
            commands.push(("Rename tab".to_owned(), Some(","), Choice::RenameTab(pane.id())));
            commands.push(("Close tab".to_owned(), Some("x"), Choice::CloseTab(pane.id())));
        }
        commands.extend(
            self.main_menu()
                .items
                .into_iter()
                .filter(|item| item.choice != Choice::Palette)
                .map(|item| (item.label, item.key, item.choice)),
        );
        commands
            .into_iter()
            .map(|(name, key, choice)| Entry {
                name,
                detail: labeled("command", &key.map(|k| format!("ctrl+b {k}")).unwrap_or_default()),
                target: Target::Command(choice),
            })
            .collect()
    }

    /// Does what a picker entry says.
    pub(super) fn open_target(&mut self, target: Target) -> Result<()> {
        match target {
            Target::Folder(path) => self.open_workspace(path, &[]),
            Target::Template(path) => self.open_template(&path),
            Target::Snippet(path) => self.insert_snippet(&path),
            Target::File(path) => self.insert_file(&path),
            Target::Command(choice) => self.run_choice(choice, self.menu_anchor()),
            Target::Tab(id) => self.select_pane(id).map(drop),
            Target::Workspace(path) => match self.spaces.list.iter().position(|ws| ws.path == path) {
                Some(i) => self.switch(|s| s.select_workspace(i)),
                None => Ok(()),
            },
        }
    }
}

/// A detail with what kind of entry it is in front, lined up in a column.
fn labeled(kind: &str, detail: &str) -> String {
    format!("{kind:<9}  {detail}")
}
