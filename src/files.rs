//! The files a workspace offers to the file picker (`ctrl+b f`), as paths
//! relative to the workspace folder. In a git repository that's what git
//! tracks plus untracked files it doesn't ignore; anywhere else it's a walk.

use std::path::Path;

use crate::review::git::{git, run};

/// Most files listed. Past this the picker gets slow to filter, and a
/// folder that big is probably not one you'd pick from by name anyway.
pub const MAX_FILES: usize = 20_000;

/// Folders the walk skips outside git, along with hidden ones.
const SKIP: [&str; 2] = ["node_modules", "target"];

/// Relative paths of the files under `dir`, sorted, at most `MAX_FILES`.
pub fn list(dir: &Path) -> Vec<String> {
    let mut files = match git_files(dir) {
        Some(files) => files,
        None => {
            let mut files = Vec::new();
            walk(dir, dir, &mut files);
            files
        }
    };
    files.sort();
    files.truncate(MAX_FILES);
    files
}

/// `None` outside a repository. Tracked files that were deleted and
/// submodules (which git lists as paths too) are left out.
fn git_files(dir: &Path) -> Option<Vec<String>> {
    let out = run(git(dir).args(["ls-files", "--cached", "--others", "--exclude-standard", "-z"])).ok()?;
    let mut files: Vec<String> =
        out.split('\0').filter(|p| !p.is_empty() && dir.join(p).is_file()).map(str::to_owned).collect();
    // A file that's tracked and also changed shows up once per stage when
    // there's a merge conflict.
    files.dedup();
    Some(files)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_FILES {
            return;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        // `file_type` doesn't follow symlinks, so a link to a folder is
        // never walked into (no loops).
        match entry.file_type() {
            Ok(t) if t.is_dir() && !SKIP.contains(&name.as_ref()) => walk(root, &entry.path(), out),
            Ok(t) if t.is_file() => {
                if let Ok(rel) = entry.path().strip_prefix(root) {
                    out.push(rel.to_string_lossy().into_owned());
                }
            }
            _ => {}
        }
    }
}

/// What gets pasted into Claude's input for a file: an `@` mention and a
/// space, so the next word doesn't run into the path.
pub fn mention(path: &str) -> String {
    format!("@{path} ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    fn temp(name: &str, files: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("clawsh-files-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for f in files {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        root
    }

    #[test]
    fn walks_plain_folders_skipping_the_noise() {
        let root = temp("walk", &["a.txt", "src/b.rs", ".git-not/c", "node_modules/x/d.js", "target/e", ".env"]);
        assert_eq!(list(&root), ["a.txt", "src/b.rs"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn git_lists_respect_gitignore() {
        let root = temp("git", &["a.txt", "src/b.rs", "build/out.o", ".gitignore", "gone.txt"]);
        std::fs::write(root.join(".gitignore"), "build/\n").unwrap();
        let git = |args: &[&str]| {
            assert!(Command::new("git").arg("-C").arg(&root).args(args).output().unwrap().status.success());
        };
        git(&["init", "-q"]);
        git(&["add", "a.txt", "gone.txt"]);
        std::fs::remove_file(root.join("gone.txt")).unwrap();
        assert_eq!(list(&root), [".gitignore", "a.txt", "src/b.rs"]);
        // From a subfolder, paths are relative to it.
        assert_eq!(list(&root.join("src")), ["b.rs"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mentions_end_with_a_space() {
        assert_eq!(mention("src/app.rs"), "@src/app.rs ");
    }
}
