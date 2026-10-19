//! The fuzzy pickers: folders for a new workspace (with typed paths), and
//! saved workspace templates.

use std::path::{Path, PathBuf};

use crate::workspace::folder_name;

/// What a picker lists, which decides its title and what Enter does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Folders,
    Templates,
}

/// One row: the name that's matched against, a faint detail beside it, and
/// the path it stands for (a folder, or a template file).
pub struct Entry {
    pub name: String,
    pub detail: String,
    pub path: PathBuf,
}

pub struct Picker {
    pub kind: Kind,
    pub query: String,
    pub selected: usize,
    entries: Vec<Entry>,
    matches: Vec<usize>,
}

impl Picker {
    pub fn new(kind: Kind, entries: Vec<Entry>) -> Self {
        let mut picker = Self {
            kind,
            query: String::new(),
            selected: 0,
            entries,
            matches: Vec::new(),
        };
        picker.refilter();
        picker
    }

    /// Folders, each named by its last part with its parent beside it.
    pub fn folders(candidates: Vec<PathBuf>) -> Self {
        let entries = candidates
            .into_iter()
            .map(|path| Entry {
                name: folder_name(&path),
                detail: path.parent().map(|p| p.display().to_string()).unwrap_or_default(),
                path,
            })
            .collect();
        Self::new(Kind::Folders, entries)
    }

    /// The launch folder, its sibling folders, and the children of every
    /// folder in `CLAWSH_PROJECTS` (colon separated). Hidden folders are skipped.
    pub fn candidates(launch: &Path, extra_roots: Option<&str>) -> Vec<PathBuf> {
        let mut out = vec![launch.to_path_buf()];
        let mut roots: Vec<PathBuf> = launch.parent().map(Path::to_path_buf).into_iter().collect();
        if let Some(extra) = extra_roots {
            roots.extend(extra.split(':').filter(|s| !s.is_empty()).map(expand_home));
        }
        for root in roots {
            let Ok(entries) = std::fs::read_dir(&root) else {
                continue;
            };
            let mut dirs: Vec<PathBuf> = entries
                .flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                .map(|e| e.path())
                .collect();
            dirs.sort_by_key(|p| folder_name(p).to_lowercase());
            for d in dirs {
                if !out.contains(&d) {
                    out.push(d);
                }
            }
        }
        out
    }

    pub fn push(&mut self, c: char) {
        self.query.push(c);
        self.refilter();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.refilter();
    }

    pub fn move_by(&mut self, delta: isize) {
        let len = self.matches.len();
        if len > 0 {
            self.selected = (self.selected as isize + delta).clamp(0, len as isize - 1) as usize;
        }
    }

    /// The visible matches, best first.
    pub fn items(&self) -> impl Iterator<Item = &Entry> {
        self.matches.iter().map(|&i| &self.entries[i])
    }

    /// A typed path that names an existing folder (folder pickers only).
    pub fn typed_dir(&self) -> Option<PathBuf> {
        let q = self.query.trim();
        if self.kind != Kind::Folders || !(q.starts_with('/') || q.starts_with('~')) {
            return None;
        }
        Some(expand_home(q)).filter(|p| p.is_dir())
    }

    /// What Enter opens: a typed folder wins, then the selected match.
    pub fn choice(&self) -> Option<PathBuf> {
        self.typed_dir()
            .or_else(|| self.items().nth(self.selected).map(|e| e.path.clone()))
    }

    pub fn select(&mut self, index: usize) {
        if index < self.matches.len() {
            self.selected = index;
        }
    }

    fn refilter(&mut self) {
        let q = self.query.trim().to_lowercase();
        let mut scored: Vec<(i32, usize)> = self
            .entries
            .iter()
            .enumerate()
            .filter_map(|(i, e)| score(&q, &e.name.to_lowercase()).map(|s| (s, i)))
            .collect();
        // Stable sort keeps the launch folder and alphabetical order on ties.
        scored.sort_by_key(|&(s, _)| -s);
        self.matches = scored.into_iter().map(|(_, i)| i).collect();
        self.selected = 0;
    }
}

/// Subsequence match: every query char appears in order. Prefix and
/// consecutive hits score higher. `None` when it doesn't match.
fn score(query: &str, name: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let mut total = 0;
    let mut last: Option<usize> = None;
    let mut chars = name.char_indices();
    for qc in query.chars() {
        let (i, _) = chars.find(|&(_, c)| c == qc)?;
        total += match last {
            None if i == 0 => 10,
            Some(prev) if i == prev + 1 => 5,
            _ => 1,
        };
        last = Some(i);
    }
    Some(total - name.len() as i32 / 8)
}

pub fn expand_home(path: &str) -> PathBuf {
    match (path.strip_prefix('~'), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => PathBuf::from(home).join(rest.trim_start_matches('/')),
        _ => PathBuf::from(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_tree(name: &str, dirs: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("clawsh-picker-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for d in dirs {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        std::fs::write(root.join("not-a-dir.txt"), "").unwrap();
        root
    }

    #[test]
    fn lists_launch_folder_then_siblings() {
        let root = temp_tree("siblings", &["clawsh", "quota", "Slate", ".hidden"]);
        let got = Picker::candidates(&root.join("clawsh"), None);
        let names: Vec<String> = got.iter().map(|p| folder_name(p)).collect();
        assert_eq!(names, ["clawsh", "quota", "Slate"]);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fuzzy_filter_prefers_prefixes() {
        let mut p = Picker::folders(
            ["/p/packrat", "/p/quota", "/p/quota-cli", "/p/slate"]
                .map(PathBuf::from)
                .to_vec(),
        );
        for c in "qt".chars() {
            p.push(c);
        }
        let names: Vec<&str> = p.items().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["quota", "quota-cli"]);
        p.backspace();
        p.backspace();
        p.push('z');
        assert_eq!(p.items().count(), 0);
        assert_eq!(p.choice(), None);
    }

    #[test]
    fn typed_folders_win() {
        let root = temp_tree("typed", &["deep/folder"]);
        let typed = root.join("deep/folder");
        let mut p = Picker::folders(vec![PathBuf::from("/p/quota")]);
        for c in typed.to_string_lossy().chars() {
            p.push(c);
        }
        assert_eq!(p.choice(), Some(typed.clone()));

        // Template pickers match names only; a path is just a query.
        let entry = Entry { name: "slate".into(), detail: String::new(), path: "/t/slate.toml".into() };
        let mut t = Picker::new(Kind::Templates, vec![entry]);
        for c in typed.to_string_lossy().chars() {
            t.push(c);
        }
        assert_eq!(t.typed_dir(), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn selection_stays_in_bounds() {
        let mut p = Picker::folders(["/a", "/b"].map(PathBuf::from).to_vec());
        p.move_by(5);
        assert_eq!(p.selected, 1);
        p.move_by(-9);
        assert_eq!(p.selected, 0);
    }
}
