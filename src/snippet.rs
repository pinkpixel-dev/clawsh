//! Prompt snippets: saved prompts pasted into the active tab (`ctrl+b i`).
//!
//! One Markdown file per snippet. The file name is the snippet's name and
//! the whole file is the prompt. Global ones live in
//! `$XDG_CONFIG_HOME/clawsh/snippets/` (or `~/.config/clawsh/snippets/`),
//! and a workspace can add its own in `<folder>/.clawsh/snippets/`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Longest preview shown beside a snippet's name, in characters.
const PREVIEW: usize = 60;

/// Where a snippet came from, shown in the picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Repo,
    Global,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Source::Repo => "repo",
            Source::Global => "global",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Listed {
    pub name: String,
    pub source: Source,
    /// The first non-empty line, cut short.
    pub preview: String,
    pub path: PathBuf,
}

pub fn global_dir() -> Option<PathBuf> {
    let config = match std::env::var_os("XDG_CONFIG_HOME") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => PathBuf::from(std::env::var_os("HOME")?).join(".config"),
    };
    Some(config.join("clawsh/snippets"))
}

pub fn repo_dir(workspace: &Path) -> PathBuf {
    workspace.join(".clawsh/snippets")
}

/// The workspace's snippets first, then the global ones, each by name. Two
/// with the same name both show up, labeled by where they came from.
pub fn list(repo: &Path, global: Option<&Path>) -> Vec<Listed> {
    let mut out = list_dir(repo, Source::Repo);
    if let Some(dir) = global {
        out.extend(list_dir(dir, Source::Global));
    }
    out
}

fn list_dir(dir: &Path, source: Source) -> Vec<Listed> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Listed> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md") && p.is_file())
        .map(|path| Listed {
            name: path.file_stem().unwrap_or_default().to_string_lossy().into_owned(),
            source,
            preview: fs::read_to_string(&path).map(|s| preview(&s)).unwrap_or_default(),
            path,
        })
        .collect();
    out.sort_by_key(|s| s.name.to_lowercase());
    out
}

/// The prompt to paste. Blank lines at the top and trailing whitespace go,
/// so a final newline in the file never submits it.
pub fn load(path: &Path) -> io::Result<String> {
    let raw = fs::read_to_string(path)?;
    Ok(raw.trim_start_matches(['\n', '\r']).trim_end().to_owned())
}

fn preview(body: &str) -> String {
    let line = body.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    if line.chars().count() <= PREVIEW {
        return line.to_owned();
    }
    let mut out: String = line.chars().take(PREVIEW - 1).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clawsh-snippet-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn lists_repo_snippets_before_global_ones() {
        let root = temp("list");
        let repo = repo_dir(&root.join("project"));
        let global = root.join("global");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&global).unwrap();
        fs::write(repo.join("review.md"), "\n\nReview the last commit\nand be picky\n").unwrap();
        fs::write(global.join("Explain.md"), "Explain this").unwrap();
        fs::write(global.join("review.md"), "Global review").unwrap();
        fs::write(global.join("notes.txt"), "not a snippet").unwrap();
        fs::create_dir_all(global.join("folder.md")).unwrap();

        let got = list(&repo, Some(&global));
        let names: Vec<(&str, Source)> = got.iter().map(|s| (s.name.as_str(), s.source)).collect();
        assert_eq!(
            names,
            [("review", Source::Repo), ("Explain", Source::Global), ("review", Source::Global)]
        );
        assert_eq!(got[0].preview, "Review the last commit");
        assert_eq!(load(&got[0].path).unwrap(), "Review the last commit\nand be picky");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_folders_list_nothing() {
        let root = temp("missing");
        assert!(list(&root.join("nope"), Some(&root.join("also-nope"))).is_empty());
    }

    #[test]
    fn long_previews_are_cut() {
        let long = "x".repeat(100);
        assert_eq!(preview(&long).chars().count(), PREVIEW);
        assert!(preview(&long).ends_with('…'));
        assert_eq!(preview("  \n  hi  \n"), "hi");
    }
}
