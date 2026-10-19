//! The "new workspace" folder picker: candidates, fuzzy filtering, and
//! typed paths.

use std::path::{Path, PathBuf};

use crate::workspace::folder_name;

pub struct Picker {
    pub query: String,
    pub selected: usize,
    candidates: Vec<PathBuf>,
    matches: Vec<usize>,
}

impl Picker {
    pub fn new(candidates: Vec<PathBuf>) -> Self {
        let mut picker = Self {
            query: String::new(),
            selected: 0,
            candidates,
            matches: Vec::new(),
        };
        picker.refilter();
        picker
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
    pub fn items(&self) -> impl Iterator<Item = &Path> {
        self.matches.iter().map(|&i| self.candidates[i].as_path())
    }

    /// A typed path that names an existing folder.
    pub fn typed_dir(&self) -> Option<PathBuf> {
        let q = self.query.trim();
        if !(q.starts_with('/') || q.starts_with('~')) {
            return None;
        }
        Some(expand_home(q)).filter(|p| p.is_dir())
    }

    /// What Enter opens: a typed folder wins, then the selected match.
    pub fn choice(&self) -> Option<PathBuf> {
        self.typed_dir()
            .or_else(|| self.items().nth(self.selected).map(Path::to_path_buf))
    }

    pub fn select(&mut self, index: usize) {
        if index < self.matches.len() {
            self.selected = index;
        }
    }

    fn refilter(&mut self) {
        let q = self.query.trim().to_lowercase();
        let mut scored: Vec<(i32, usize)> = self
            .candidates
            .iter()
            .enumerate()
            .filter_map(|(i, p)| score(&q, &folder_name(p).to_lowercase()).map(|s| (s, i)))
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

fn expand_home(path: &str) -> PathBuf {
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
        let mut p = Picker::new(
            ["/p/packrat", "/p/quota", "/p/quota-cli", "/p/slate"]
                .map(PathBuf::from)
                .to_vec(),
        );
        for c in "qt".chars() {
            p.push(c);
        }
        let names: Vec<String> = p.items().map(folder_name).collect();
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
        let mut p = Picker::new(vec![PathBuf::from("/p/quota")]);
        for c in root.join("deep/folder").to_string_lossy().chars() {
            p.push(c);
        }
        assert_eq!(p.choice(), Some(root.join("deep/folder")));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn selection_stays_in_bounds() {
        let mut p = Picker::new(["/a", "/b"].map(PathBuf::from).to_vec());
        p.move_by(5);
        assert_eq!(p.selected, 1);
        p.move_by(-9);
        assert_eq!(p.selected, 0);
    }
}
