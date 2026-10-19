//! The git calls behind the review view. Everything shells out to `git`.
//!
//! Both scopes diff two trees. The "after" side is always a snapshot of the
//! worktree as it is now, untracked files included, written through a
//! throwaway index so the real index and stash are never touched. The
//! "before" side is `HEAD` (uncommitted) or the snapshot the hook took when
//! the tab's last prompt was sent (last turn).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};

/// One changed file and its line counts; `None` counts mean binary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileStat {
    pub path: String,
    pub added: Option<u32>,
    pub deleted: Option<u32>,
}

fn git(dir: &Path) -> Command {
    let mut cmd = Command::new("git");
    // Paths are file names, never patterns. No surprise gc in the middle of
    // a snapshot either.
    cmd.args(["--literal-pathspecs", "-c", "gc.auto=0", "-C"])
        .arg(dir)
        .stdin(Stdio::null());
    cmd
}

fn run(cmd: &mut Command) -> Result<String> {
    let out = cmd.output()?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("{}", err.lines().next().unwrap_or("git failed").trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The top of the repository `dir` is in.
pub fn root(dir: &Path) -> Result<PathBuf> {
    Ok(PathBuf::from(run(git(dir).args(["rev-parse", "--show-toplevel"]))?.trim()))
}

/// Writes the whole worktree, untracked files included, as a tree object and
/// returns its ID. Starts from a copy of the real index so unchanged files
/// don't get hashed again.
pub fn snapshot(dir: &Path) -> Result<String> {
    let index = dir.join(run(git(dir).args(["rev-parse", "--git-path", "index"]))?.trim());
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("clawsh-index-{}-{nanos}", std::process::id()));
    if index.exists() {
        std::fs::copy(&index, &tmp)?;
    }
    let tree = run(git(dir).env("GIT_INDEX_FILE", &tmp).args(["add", "-A"]))
        .and_then(|_| run(git(dir).env("GIT_INDEX_FILE", &tmp).arg("write-tree")));
    let _ = std::fs::remove_file(&tmp);
    Ok(tree?.trim().to_owned())
}

/// `HEAD`'s tree, or the empty tree in a repository with no commits yet.
pub fn head_tree(root: &Path) -> Result<String> {
    match run(git(root).args(["rev-parse", "--verify", "-q", "HEAD^{tree}"])) {
        Ok(tree) => Ok(tree.trim().to_owned()),
        // `mktree` with nothing on stdin writes the empty tree, in whatever
        // hash the repository uses.
        Err(_) => Ok(run(git(root).arg("mktree"))?.trim().to_owned()),
    }
}

/// Files that differ between two trees. Renames show as a delete plus an add.
pub fn changed(root: &Path, base: &str, tree: &str) -> Result<Vec<FileStat>> {
    let out = run(git(root).args(["diff", "--numstat", "-z", "--no-renames", base, tree]))?;
    Ok(parse_numstat(&out))
}

/// The unified diff of one file between two trees.
pub fn file_diff(root: &Path, base: &str, tree: &str, path: &str) -> Result<String> {
    run(git(root)
        .args(["diff", "--no-color", "--no-ext-diff", "--no-textconv", "--no-renames", "-U3"])
        .args([base, tree, "--", path]))
}

/// `added\tdeleted\tpath\0` records; binary files have `-` for both counts.
fn parse_numstat(out: &str) -> Vec<FileStat> {
    out.split('\0')
        .filter_map(|rec| {
            let mut parts = rec.splitn(3, '\t');
            let added = parts.next()?;
            let deleted = parts.next()?;
            let path = parts.next().filter(|p| !p.is_empty())?;
            Some(FileStat {
                path: path.to_owned(),
                added: added.parse().ok(),
                deleted: deleted.parse().ok(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A real repository in the temp dir, with one commit.
    fn repo(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clawsh-git-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        run(git(&dir).args(["init", "-q"])).unwrap();
        std::fs::write(dir.join("a.txt"), "one\n").unwrap();
        run(git(&dir).args(["add", "a.txt"])).unwrap();
        run(git(&dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
            .args(["commit", "-q", "--no-verify", "-m", "init"]))
        .unwrap();
        dir
    }

    #[test]
    fn numstat_records_parse() {
        let out = "3\t1\tsrc/a b.rs\0-\t-\timg.png\0\0";
        assert_eq!(
            parse_numstat(out),
            vec![
                FileStat { path: "src/a b.rs".into(), added: Some(3), deleted: Some(1) },
                FileStat { path: "img.png".into(), added: None, deleted: None },
            ]
        );
    }

    #[test]
    fn snapshots_see_untracked_files_and_leave_the_index_alone() {
        let dir = repo("snap");
        let status = || run(git(&dir).args(["status", "--porcelain"])).unwrap();
        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(dir.join("new.txt"), "hi\n").unwrap();
        let before = status();

        let start = snapshot(&dir).unwrap();
        assert_eq!(status(), before, "real index untouched");
        let head = head_tree(&dir).unwrap();
        let files = changed(&dir, &head, &start).unwrap();
        let paths: Vec<_> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["a.txt", "new.txt"]);

        // A later snapshot only differs by what changed in between.
        std::fs::write(dir.join("new.txt"), "hi\nthere\n").unwrap();
        let now = snapshot(&dir).unwrap();
        let turn = changed(&dir, &start, &now).unwrap();
        assert_eq!(turn, vec![FileStat { path: "new.txt".into(), added: Some(1), deleted: Some(0) }]);
        let diff = file_diff(&dir, &start, &now, "new.txt").unwrap();
        assert!(diff.contains("+there"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_repo_without_commits_diffs_against_the_empty_tree() {
        let dir = std::env::temp_dir().join(format!("clawsh-git-empty-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        run(git(&dir).args(["init", "-q"])).unwrap();
        std::fs::write(dir.join("x.md"), "# hi\n").unwrap();
        let base = head_tree(&dir).unwrap();
        let files = changed(&dir, &base, &snapshot(&dir).unwrap()).unwrap();
        assert_eq!(files.len(), 1);
        assert!(root(&dir).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
