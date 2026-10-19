//! Git worktrees: which branch a folder is on, listing a repository's
//! worktrees, and adding or removing one. Everything but `head` shells out to
//! `git`; `head` reads files so the sidebar can ask often.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use crate::review::git::{git, run};
use crate::workspace::folder_name;

/// What a folder has checked out, for the sidebar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Head {
    /// The branch name, or a short commit ID when detached.
    pub label: String,
    /// A linked worktree, not the repository's main checkout.
    pub linked: bool,
}

/// Reads the branch of the repository `dir` is in, straight from `HEAD`.
/// `None` outside a repository.
pub fn head(dir: &Path) -> Option<Head> {
    let (top, dot_git) = dir.ancestors().map(|d| (d, d.join(".git"))).find(|(_, g)| g.exists())?;
    let (git_dir, linked) = if dot_git.is_dir() {
        (dot_git, false)
    } else {
        // A linked worktree (or a submodule) has a `.git` file pointing at
        // its real git dir, which for worktrees is `<repo>/.git/worktrees/<name>`.
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let target = top.join(text.strip_prefix("gitdir:")?.trim());
        let linked = target.parent().is_some_and(|p| p.file_name().is_some_and(|n| n == "worktrees"));
        (target, linked)
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    let label = match head.strip_prefix("ref:") {
        Some(r) => {
            let r = r.trim();
            r.strip_prefix("refs/heads/").unwrap_or(r).to_owned()
        }
        None => head.chars().take(7).collect(),
    };
    Some(Head { label, linked })
}

/// One entry from `git worktree list`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: PathBuf,
    /// `None` when detached.
    pub branch: Option<String>,
    pub head: String,
    /// The repository's main checkout, which can't be removed.
    pub main: bool,
    pub locked: bool,
}

impl Worktree {
    /// The branch, or the short commit when detached.
    pub fn label(&self) -> String {
        match &self.branch {
            Some(b) => b.clone(),
            None => format!("detached {}", self.head.chars().take(7).collect::<String>()),
        }
    }
}

/// Every worktree of the repository `dir` is in, main checkout first.
/// Worktrees whose folder is gone are left out, since there's nothing to open.
pub fn list(dir: &Path) -> Result<Vec<Worktree>> {
    let out = run(git(dir).args(["worktree", "list", "--porcelain"]))?;
    Ok(parse_list(&out).into_iter().filter(|w| w.path.is_dir()).collect())
}

fn parse_list(out: &str) -> Vec<Worktree> {
    let mut list = Vec::new();
    for record in out.split("\n\n") {
        let mut wt = None;
        let mut bare = false;
        for line in record.lines() {
            let (key, value) = line.split_once(' ').unwrap_or((line, ""));
            match key {
                "worktree" => {
                    wt = Some(Worktree {
                        path: PathBuf::from(value),
                        branch: None,
                        head: String::new(),
                        main: list.is_empty(),
                        locked: false,
                    });
                }
                "HEAD" => wt.iter_mut().for_each(|w| w.head = value.to_owned()),
                "branch" => {
                    let b = value.strip_prefix("refs/heads/").unwrap_or(value);
                    wt.iter_mut().for_each(|w| w.branch = Some(b.to_owned()));
                }
                "locked" => wt.iter_mut().for_each(|w| w.locked = true),
                "bare" => bare = true,
                _ => {}
            }
        }
        // A bare repository's entry has no checkout to open, but it still
        // counts as the main one.
        if let Some(mut w) = wt {
            if bare {
                w.path = PathBuf::new();
            }
            list.push(w);
        }
    }
    list.retain(|w| !w.path.as_os_str().is_empty());
    list
}

/// Where a new worktree for `branch` goes: next to the main checkout, as
/// `<repo>-<branch>` with slashes turned into dashes.
pub fn new_path(main: &Path, branch: &str) -> PathBuf {
    let name = format!("{}-{}", folder_name(main), branch.replace('/', "-"));
    main.parent().unwrap_or(main).join(name)
}

/// Adds a worktree for `branch` next to the main checkout and returns its
/// folder. An existing branch is checked out; a new one starts from what
/// `dir` has checked out.
pub fn add(dir: &Path, main: &Path, branch: &str) -> Result<PathBuf> {
    let branch = run(git(dir).args(["check-ref-format", "--branch", branch.trim()]))
        .map_err(|_| anyhow::anyhow!("{} isn't a valid branch name", branch.trim()))?;
    let branch = branch.trim();
    let path = new_path(main, branch);
    if path.exists() {
        bail!("{} already exists", path.display());
    }
    let exists = git(dir)
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .status()?
        .success();
    let mut cmd = git(dir);
    cmd.args(["worktree", "add"]);
    if exists {
        cmd.arg(&path).arg(branch);
    } else {
        cmd.args(["-b", branch]).arg(&path);
    }
    run(&mut cmd)?;
    Ok(path)
}

/// How many files `git status` would list: changed, staged, or untracked.
pub fn changes(path: &Path) -> Result<usize> {
    Ok(run(git(path).args(["status", "--porcelain", "-z"]))?.split('\0').filter(|s| !s.is_empty()).count())
}

/// Removes a worktree's folder, running git from `repo` (any other folder in
/// the same repository). The branch stays. `force` throws away uncommitted
/// changes.
pub fn remove(repo: &Path, path: &Path, force: bool) -> Result<()> {
    let mut cmd = git(repo);
    cmd.args(["worktree", "remove"]);
    if force {
        cmd.arg("--force");
    }
    run(cmd.arg(path))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn sh(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"])
            .args(args)
            .output()
            .unwrap()
            .status
            .success();
        assert!(ok, "git {args:?}");
    }

    /// A repository with one commit, in a fresh temp folder.
    fn repo(name: &str) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("clawsh-wt-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let main = root.join("proj");
        std::fs::create_dir_all(&main).unwrap();
        sh(&main, &["init", "-q"]);
        std::fs::write(main.join("a.txt"), "a").unwrap();
        sh(&main, &["add", "."]);
        sh(&main, &["commit", "-qm", "init"]);
        (root, main)
    }

    #[test]
    fn parses_porcelain_output() {
        let out = "worktree /r/proj\nHEAD aaaaaaaaaa\nbranch refs/heads/main\n\n\
                   worktree /r/proj-fix\nHEAD bbbbbbbbbb\nbranch refs/heads/fix/x\nlocked\n\n\
                   worktree /r/proj-d\nHEAD cccccccccc\ndetached\n";
        let list = parse_list(out);
        assert_eq!(list.len(), 3);
        assert!(list[0].main && !list[1].main);
        assert_eq!(list[1].branch.as_deref(), Some("fix/x"));
        assert!(list[1].locked);
        assert_eq!(list[2].label(), "detached ccccccc");
        let bare = parse_list("worktree /r/b.git\nbare\n\nworktree /r/b-x\nHEAD dd\nbranch refs/heads/x\n");
        assert_eq!(bare.len(), 1, "the bare entry has nothing to open");
        assert!(!bare[0].main);
    }

    #[test]
    fn new_worktrees_sit_next_to_the_repo() {
        assert_eq!(new_path(Path::new("/p/clawsh"), "fix/search"), PathBuf::from("/p/clawsh-fix-search"));
    }

    #[test]
    fn add_list_and_remove() {
        let (root, main) = repo("cycle");
        assert_eq!(head(&main), Some(Head { label: "main".into(), linked: false }));
        assert!(add(&main, &main, "bad..name").is_err());

        let wt = add(&main, &main, "feature/one").unwrap();
        assert_eq!(wt, root.join("proj-feature-one"));
        assert_eq!(head(&wt), Some(Head { label: "feature/one".into(), linked: true }));
        let list = list(&wt).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[0].main);
        assert_eq!(list[1].branch.as_deref(), Some("feature/one"));
        assert!(add(&main, &main, "feature/one").is_err(), "folder already exists");

        std::fs::write(wt.join("new.txt"), "x").unwrap();
        std::fs::write(wt.join("a.txt"), "changed").unwrap();
        assert_eq!(changes(&wt).unwrap(), 2);
        assert!(remove(&main, &wt, false).is_err(), "git refuses a dirty worktree");
        remove(&main, &wt, true).unwrap();
        assert!(!wt.exists());

        // The branch stays, so adding it again checks it out.
        let again = add(&main, &main, "feature/one").unwrap();
        assert_eq!(head(&again).unwrap().label, "feature/one");
        assert_eq!(changes(&again).unwrap(), 0);
        remove(&main, &again, false).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn detached_heads_show_a_short_id() {
        let (root, main) = repo("detached");
        sh(&main, &["checkout", "-q", "--detach"]);
        let label = head(&main).unwrap().label;
        assert_eq!(label.len(), 7);
        assert!(label.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(head(&root), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
