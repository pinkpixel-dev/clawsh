//! The review view (`ctrl+b r`): what changed in a tab's folder, file by file,
//! with comments you can send to that tab's Claude as one paste.
//!
//! Each pane keeps its own `Review`, so closing the view and opening it again
//! keeps your comments until they're sent.

mod comments;
mod diff;
pub mod git;
mod keys;
pub mod markdown;

use std::path::{Path, PathBuf};

use ratatui::layout::Rect;

pub use comments::Comment;
pub use diff::{DiffLine, Kind};
pub use git::FileStat;
pub use keys::Outcome;

/// Diffs bigger than this show a notice instead.
const MAX_DIFF_BYTES: usize = 2 << 20;
const MAX_DIFF_LINES: usize = 50_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// The worktree against `HEAD`, untracked files included.
    Uncommitted,
    /// The worktree against the snapshot taken when the last prompt was sent.
    LastTurn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Files,
    Diff,
}

/// What the right side shows for the selected file.
pub enum Body {
    Diff(Vec<DiffLine>),
    /// Markdown source, rendered when drawn so it follows theme changes.
    Markdown(String),
    Message(String),
}

/// A comment being typed.
pub struct Draft {
    pub text: String,
    /// Which comment is being edited, or `None` for a new one.
    pub editing: Option<usize>,
    /// What a new comment covers.
    pub target: Comment,
}

/// Where things landed in the last draw, for the mouse.
#[derive(Default)]
pub struct Areas {
    pub files: Rect,
    pub diff: Rect,
    pub scopes: [Rect; 2],
}

pub struct Review {
    dir: PathBuf,
    root: Option<PathBuf>,
    /// The two trees being compared, once loaded.
    trees: Option<(String, String)>,
    pub scope: Scope,
    pub files: Vec<FileStat>,
    pub selected: usize,
    pub focus: Focus,
    pub body: Body,
    /// Line in the diff (or row in rendered markdown).
    pub cursor: usize,
    /// Where a `v` selection started.
    pub anchor: Option<usize>,
    /// First diff line on screen; the draw keeps the cursor visible.
    pub scroll: usize,
    pub file_scroll: usize,
    /// Show markdown files rendered instead of as diffs (`m`).
    pub rendered: bool,
    pub comments: Vec<Comment>,
    pub draft: Option<Draft>,
    /// A one-line message for the last thing that didn't work.
    pub notice: Option<String>,
    /// Why there's no file list, when there isn't one.
    pub problem: Option<String>,
    /// Diff rows on screen in the last draw, for paging.
    pub rows: usize,
    pub areas: Areas,
    /// The diff line a mouse drag started on.
    pub(crate) drag_from: Option<usize>,
}

impl Review {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            root: None,
            trees: None,
            scope: Scope::Uncommitted,
            files: Vec::new(),
            selected: 0,
            focus: Focus::Files,
            body: Body::Message(String::new()),
            cursor: 0,
            anchor: None,
            scroll: 0,
            file_scroll: 0,
            rendered: false,
            comments: Vec::new(),
            draft: None,
            notice: None,
            problem: None,
            rows: 20,
            areas: Areas::default(),
            drag_from: None,
        }
    }

    /// Takes a new snapshot and reloads the file list, staying on the same
    /// file when it's still there. `turn_start` is the tree the hook saved
    /// when the tab's last prompt was sent.
    pub fn refresh(&mut self, turn_start: Option<&str>) {
        let keep = self.path().map(str::to_owned);
        self.problem = None;
        self.trees = None;
        self.files.clear();
        match self.load(turn_start) {
            Ok(files) => self.files = files,
            Err(why) => self.problem = Some(why),
        }
        let same = keep.as_deref().and_then(|p| self.files.iter().position(|f| f.path == p));
        self.selected = same.unwrap_or(0);
        self.load_body(same.is_none(), self.rendered);
    }

    fn load(&mut self, turn_start: Option<&str>) -> Result<Vec<FileStat>, String> {
        let root = git::root(&self.dir).map_err(|_| "not a git repository".to_owned())?;
        let base = match self.scope {
            Scope::Uncommitted => git::head_tree(&root).map_err(|e| e.to_string())?,
            Scope::LastTurn => turn_start.ok_or("no turn yet in this tab")?.to_owned(),
        };
        let tree = git::snapshot(&root).map_err(|e| e.to_string())?;
        let files = git::changed(&root, &base, &tree).map_err(|e| e.to_string())?;
        self.root = Some(root);
        self.trees = Some((base, tree));
        Ok(files)
    }

    /// The selected file's path.
    pub fn path(&self) -> Option<&str> {
        self.files.get(self.selected).map(|f| f.path.as_str())
    }

    pub fn is_markdown(&self) -> bool {
        self.path().is_some_and(is_markdown)
    }

    /// Loads what the right side shows for the selected file. `reset` moves
    /// back to the top; otherwise the cursor stays put as far as it can.
    pub(crate) fn load_body(&mut self, reset: bool, rendered: bool) {
        self.anchor = None;
        if reset {
            self.cursor = 0;
            self.scroll = 0;
        }
        self.body = self.read_body(rendered);
        self.cursor = self.cursor.min(self.body_len().saturating_sub(1));
    }

    fn read_body(&self, rendered: bool) -> Body {
        let (Some(file), Some(root), Some((base, tree))) = (self.files.get(self.selected), &self.root, &self.trees)
        else {
            return Body::Message(String::new());
        };
        if rendered && is_markdown(&file.path) {
            // A deleted file has nothing to render, so it falls back to the diff.
            if let Ok(src) = read_capped(&root.join(&file.path)) {
                return Body::Markdown(src);
            }
        }
        if file.added.is_none() {
            return Body::Message("binary file".into());
        }
        match git::file_diff(root, base, tree, &file.path) {
            Ok(text) if text.len() > MAX_DIFF_BYTES => Body::Message("too large to show".into()),
            Ok(text) => {
                let lines = diff::parse(&text);
                if lines.len() > MAX_DIFF_LINES {
                    Body::Message("too large to show".into())
                } else {
                    Body::Diff(lines)
                }
            }
            Err(e) => Body::Message(e.to_string()),
        }
    }

    /// Lines the cursor can move over.
    pub fn body_len(&self) -> usize {
        match &self.body {
            Body::Diff(lines) => lines.len(),
            Body::Markdown(src) => markdown::render(src).len(),
            Body::Message(_) => 0,
        }
    }

    /// The selected diff lines: the `v` range, or just the cursor's line.
    pub fn selection(&self) -> (usize, usize) {
        let a = self.anchor.unwrap_or(self.cursor);
        (a.min(self.cursor), a.max(self.cursor))
    }

    /// The comment on diff line `i` of the selected file, if there is one.
    pub fn comment_at(&self, i: usize) -> Option<usize> {
        let (Body::Diff(lines), Some(path)) = (&self.body, self.path()) else {
            return None;
        };
        let line = lines.get(i)?;
        self.comments.iter().position(|c| c.covers(path, line))
    }

    /// How many comments a file has.
    pub fn comments_on(&self, path: &str) -> usize {
        self.comments.iter().filter(|c| c.path == path).count()
    }

    /// The paste for Claude, or `None` with nothing to send.
    pub fn message(&self) -> Option<String> {
        (!self.comments.is_empty()).then(|| comments::message(&self.comments))
    }
}

fn is_markdown(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

fn read_capped(path: &Path) -> std::io::Result<String> {
    if std::fs::metadata(path)?.len() > MAX_DIFF_BYTES as u64 {
        return Err(std::io::Error::other("too large"));
    }
    std::fs::read_to_string(path)
}
