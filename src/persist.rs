//! Remembers workspaces and each tab's Claude conversation between runs.
//!
//! Saved to `$XDG_STATE_HOME/clawsh/session.json` (or
//! `~/.local/state/clawsh/session.json`) whenever it changes, so closing the
//! window or a crash loses nothing. On the next launch each tab comes back
//! with `claude --resume <id>`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::workspace::{Workspaces, folder_name};

const VERSION: u32 = 1;

/// One Claude conversation, as its hooks report it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRef {
    pub id: String,
    /// Claude only writes this once the first prompt is sent.
    pub transcript: Option<PathBuf>,
}

impl SessionRef {
    /// Session IDs go straight into `claude` arguments, so only accept the
    /// shape Claude uses (letters, digits, dashes).
    pub fn new(id: &str, transcript: Option<PathBuf>) -> Option<Self> {
        let valid = !id.is_empty()
            && id.len() <= 128
            && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        valid.then(|| Self {
            id: id.to_owned(),
            transcript,
        })
    }

    /// `--resume <id>` when there's a conversation to go back to. A tab that
    /// never got a prompt has no transcript and just starts fresh.
    pub fn resume_args(&self) -> Vec<String> {
        if self.transcript.as_deref().is_some_and(Path::is_file) {
            vec!["--resume".to_owned(), self.id.clone()]
        } else {
            Vec::new()
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub version: u32,
    /// Desktop notifications on or off (`ctrl+b m`). Kept even by `--fresh`.
    #[serde(default = "on")]
    pub notifications: bool,
    /// Sidebar in activity order (`ctrl+b a`). Kept even by `--fresh`.
    #[serde(default)]
    pub by_activity: bool,
    /// Terminal drawers open on the right instead of the bottom.
    #[serde(default)]
    pub drawer_right: bool,
    /// How much of the pane area a drawer takes, in percent.
    #[serde(default = "drawer_percent")]
    pub drawer_percent: u16,
    pub active: usize,
    pub workspaces: Vec<SavedWorkspace>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedWorkspace {
    pub path: PathBuf,
    /// A display name given from the menu; `None` shows the folder's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub active: usize,
    pub tabs: Vec<Option<SessionRef>>,
    /// Names given with `ctrl+b ,`, lined up with `tabs`. Missing in files
    /// from before renaming existed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<Option<String>>,
}

impl Saved {
    pub fn from_spaces<T>(
        spaces: &Workspaces<T>,
        (notifications, by_activity): (bool, bool),
        (drawer_right, drawer_percent): (bool, u16),
        session: impl Fn(&T) -> Option<SessionRef>,
        name: impl Fn(&T) -> Option<String>,
    ) -> Self {
        Self {
            version: VERSION,
            notifications,
            by_activity,
            drawer_right,
            drawer_percent,
            active: spaces.active,
            workspaces: spaces
                .list
                .iter()
                .map(|ws| SavedWorkspace {
                    path: ws.path.clone(),
                    name: (ws.name != folder_name(&ws.path)).then(|| ws.name.clone()),
                    active: ws.active,
                    tabs: ws.tabs.iter().map(&session).collect(),
                    names: match ws.tabs.iter().map(&name).collect::<Vec<_>>() {
                        names if names.iter().all(Option::is_none) => Vec::new(),
                        names => names,
                    },
                })
                .collect(),
        }
    }
}

fn on() -> bool {
    true
}

fn drawer_percent() -> u16 {
    crate::ui::drawer::DEFAULT_PERCENT
}

pub fn default_path() -> Option<PathBuf> {
    let state = match std::env::var_os("XDG_STATE_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".local/state"),
    };
    Some(state.join("clawsh/session.json"))
}

/// The last saved session. Missing, unreadable, or from another version
/// means starting clean.
pub fn load(path: &Path) -> Option<Saved> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str::<Saved>(&raw)
        .ok()
        .filter(|s| s.version == VERSION)
}

/// Writes to a temp file and renames it over the old one, so a crash
/// mid-write can't leave a half-written session behind.
pub fn save(path: &Path, saved: &Saved) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(saved)?)?;
    fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clawsh-persist-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn only_claude_shaped_ids_are_accepted() {
        assert!(SessionRef::new("8fdc263f-6e38-4b1e-ac3c-96e8816ff770", None).is_some());
        assert!(SessionRef::new("", None).is_none());
        assert!(SessionRef::new("--dangerous flag", None).is_none());
    }

    #[test]
    fn resumes_only_when_the_transcript_exists() {
        let dir = temp("resume");
        fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("abc.jsonl");
        let s = SessionRef::new("abc", Some(transcript.clone())).unwrap();
        assert!(s.resume_args().is_empty(), "no prompt sent yet");
        fs::write(&transcript, "{}\n").unwrap();
        assert_eq!(s.resume_args(), ["--resume", "abc"]);
        assert!(SessionRef::new("abc", None).unwrap().resume_args().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn snapshots_workspaces_and_round_trips() {
        let mut spaces = Workspaces::new();
        spaces.add_workspace(PathBuf::from("/p/quota"), 1u32);
        spaces.add_tab(2);
        spaces.add_workspace(PathBuf::from("/p/slate"), 3);
        spaces.select_tab(0, 0);
        spaces.rename(Path::new("/p/slate"), "notes");
        let saved = Saved::from_spaces(
            &spaces,
            (false, true),
            (true, 50),
            |&t| SessionRef::new(&format!("s{t}"), None),
            |&t| (t == 2).then(|| "auth work".to_owned()),
        );
        assert!(!saved.notifications);
        assert!(saved.by_activity);
        assert!(saved.drawer_right);
        assert_eq!(saved.drawer_percent, 50);
        assert_eq!(saved.workspaces[0].names, [None, Some("auth work".to_owned())]);
        assert!(saved.workspaces[1].names.is_empty(), "no names, nothing saved");
        assert_eq!(saved.workspaces[0].name, None, "folder name isn't saved");
        assert_eq!(saved.workspaces[1].name.as_deref(), Some("notes"));
        assert_eq!(saved.active, 0);
        assert_eq!(saved.workspaces[0].tabs.len(), 2);
        assert_eq!(saved.workspaces[1].tabs[0].as_ref().unwrap().id, "s3");

        let dir = temp("roundtrip");
        let path = dir.join("nested/session.json");
        save(&path, &saved).unwrap();
        assert_eq!(load(&path), Some(saved));
        assert!(!path.with_extension("json.tmp").exists());

        // Files from before the settings existed load with the defaults.
        fs::write(&path, r#"{"version":1,"active":0,"workspaces":[]}"#).unwrap();
        let old = load(&path).unwrap();
        assert!(old.notifications && !old.by_activity);
        assert!(!old.drawer_right);
        assert_eq!(old.drawer_percent, crate::ui::drawer::DEFAULT_PERCENT);

        fs::write(&path, "not json").unwrap();
        assert_eq!(load(&path), None, "corrupt file means a clean start");
        fs::remove_dir_all(dir).unwrap();
    }
}
