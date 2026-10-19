//! Conversation search (`ctrl+b /`): looks through Claude's saved
//! transcripts for a query, one result per conversation, so you can resume
//! the one you were thinking of.
//!
//! There's no index. Each search scans `~/.claude/projects/*/*.jsonl` on a
//! few background threads and streams results back as they turn up. Only
//! your typed prompts and Claude's replies count, not tool calls or tool
//! output. Subagent transcripts live a folder deeper and are never listed.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::time::SystemTime;

use serde_json::Value;

use crate::event::AppEvent;
use crate::persist::SessionRef;
use crate::title;

/// More than this and only the newest are kept.
pub const MAX_HITS: usize = 200;
/// Characters of context kept on each side of the match.
const BEFORE: usize = 40;
const AFTER: usize = 160;
/// Scanning threads, at most.
const MAX_THREADS: usize = 8;

/// Every search gets its own number, so results from one you replaced are
/// dropped when they arrive.
static GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    You,
    Claude,
}

/// The first message that matched, cut down to one line around the match.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snippet {
    pub who: Who,
    pub before: String,
    pub matched: String,
    pub after: String,
}

/// One conversation that matched.
#[derive(Clone, Debug)]
pub struct Hit {
    pub session: SessionRef,
    /// The folder Claude ran in, which is where it has to be resumed.
    pub cwd: PathBuf,
    pub title: String,
    pub modified: SystemTime,
    /// How many messages matched.
    pub count: usize,
    pub snippet: Snippet,
}

/// The search overlay's state.
pub struct Search {
    pub query: String,
    /// The query the results are for, once one has run.
    pub searched: Option<String>,
    /// Newest first.
    pub hits: Vec<Hit>,
    /// The selected conversation's id; `None` or gone means the first row.
    selected: Option<String>,
    pub scanning: bool,
    /// Some results were dropped to stay under `MAX_HITS`.
    pub capped: bool,
    generation: u64,
    cancel: Arc<AtomicBool>,
}

impl Search {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            searched: None,
            hits: Vec::new(),
            selected: None,
            scanning: false,
            capped: false,
            generation: 0,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The query was changed since the results came in, so Enter searches
    /// again instead of opening a result.
    pub fn stale(&self) -> bool {
        self.searched.as_deref() != Some(self.query.trim())
    }

    /// Starts scanning for the query, replacing any search still running.
    /// Does nothing for an empty query.
    pub fn start(&mut self, root: Option<PathBuf>, tx: Sender<AppEvent>) {
        let query = self.query.trim().to_owned();
        if query.is_empty() {
            return;
        }
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
        self.hits.clear();
        self.selected = None;
        self.capped = false;
        self.searched = Some(query.clone());
        let Some(root) = root else {
            return;
        };
        self.scanning = true;
        let (generation, cancel) = (self.generation, self.cancel.clone());
        std::thread::spawn(move || run(&root, &Query::new(&query), generation, &cancel, &tx));
    }

    /// A result arrived. Keeps newest first and drops the oldest past
    /// `MAX_HITS`. Results from an older search are ignored.
    pub fn add(&mut self, generation: u64, hit: Hit) {
        if generation != self.generation {
            return;
        }
        let at = self.hits.partition_point(|h| h.modified >= hit.modified);
        self.hits.insert(at, hit);
        if self.hits.len() > MAX_HITS {
            self.hits.truncate(MAX_HITS);
            self.capped = true;
        }
    }

    pub fn finish(&mut self, generation: u64) {
        if generation == self.generation {
            self.scanning = false;
        }
    }

    #[cfg(test)]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Where the selection is in `hits`.
    pub fn selected_index(&self) -> usize {
        self.selected
            .as_deref()
            .and_then(|id| self.hits.iter().position(|h| h.session.id == id))
            .unwrap_or(0)
    }

    pub fn selected_hit(&self) -> Option<&Hit> {
        self.hits.get(self.selected_index())
    }

    pub fn select(&mut self, index: usize) {
        if let Some(hit) = self.hits.get(index) {
            self.selected = Some(hit.session.id.clone());
        }
    }

    /// Moves the selection, clamped at both ends.
    pub fn move_by(&mut self, by: isize) {
        if let Some(last) = self.hits.len().checked_sub(1) {
            self.select(self.selected_index().saturating_add_signed(by).min(last));
        }
    }
}

impl Drop for Search {
    /// Closing the overlay stops a scan that's still going.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Where Claude keeps its transcripts, one folder per project.
pub fn projects_dir() -> Option<PathBuf> {
    let dir = match std::env::var_os("CLAUDE_CONFIG_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".claude"),
    };
    Some(dir.join("projects"))
}

/// A query, ready to match against text.
struct Query {
    /// Lowercased, one char each.
    chars: Vec<char>,
    /// Lowercased ASCII bytes for a quick check of the raw JSON line, when
    /// the query has nothing JSON would escape or that ASCII folding misses.
    raw: Option<Vec<u8>>,
}

impl Query {
    fn new(query: &str) -> Self {
        let plain = query.bytes().all(|b| b.is_ascii() && !b.is_ascii_control() && b != b'"' && b != b'\\');
        Self {
            chars: query.chars().map(fold).collect(),
            raw: plain.then(|| query.to_ascii_lowercase().into_bytes()),
        }
    }

    /// Whether a raw JSON line could hold a match.
    fn might_match(&self, line: &str) -> bool {
        match &self.raw {
            Some(raw) => line.as_bytes().windows(raw.len()).any(|w| w.eq_ignore_ascii_case(raw)),
            None => true,
        }
    }
}

/// Scans every transcript under `root`, newest first, sending each match as
/// it's found and `SearchDone` at the end.
fn run(root: &Path, query: &Query, generation: u64, cancel: &AtomicBool, tx: &Sender<AppEvent>) {
    let files = transcripts(root);
    let next = AtomicUsize::new(0);
    let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).min(MAX_THREADS);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                while !cancel.load(Ordering::Relaxed) {
                    let Some((path, modified)) = files.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        break;
                    };
                    if let Some(hit) = scan(path, *modified, query, cancel) {
                        let _ = tx.send(AppEvent::SearchHit(generation, Box::new(hit)));
                    }
                }
            });
        }
    });
    if !cancel.load(Ordering::Relaxed) {
        let _ = tx.send(AppEvent::SearchDone(generation));
    }
}

/// `root/<project>/<session>.jsonl`, newest first.
fn transcripts(root: &Path) -> Vec<(PathBuf, SystemTime)> {
    let mut out = Vec::new();
    for project in std::fs::read_dir(root).into_iter().flatten().flatten() {
        for file in std::fs::read_dir(project.path()).into_iter().flatten().flatten() {
            let path = file.path();
            if path.extension().is_some_and(|e| e == "jsonl")
                && let Ok(meta) = file.metadata()
                && meta.is_file()
            {
                out.push((path, meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)));
            }
        }
    }
    out.sort_by_key(|&(_, modified)| std::cmp::Reverse(modified));
    out
}

/// One transcript as a result, if any of its messages match and it can
/// still be resumed (its folder is still there).
fn scan(path: &Path, modified: SystemTime, query: &Query, cancel: &AtomicBool) -> Option<Hit> {
    let id = path.file_stem()?.to_str()?;
    let session = SessionRef::new(id, Some(path.to_path_buf()))?;
    let file = File::open(path).ok()?;
    let mut count = 0;
    let mut snippet = None;
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        // Most lines are tool calls and output; skip them before parsing.
        if !(line.contains("\"assistant\"") || line.contains("\"human\"")) || !query.might_match(&line) {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(&line) else { continue };
        let Some((who, texts)) = message(&record) else { continue };
        let found = texts.iter().find_map(|t| find(t, &query.chars).map(|range| (t, range)));
        if let Some((text, range)) = found {
            count += 1;
            snippet.get_or_insert_with(|| cut(who, text, range));
        }
    }
    let snippet = snippet?;
    let cwd = first_cwd(path).filter(|d| d.is_dir())?;
    Some(Hit {
        title: title::from_transcript(path).unwrap_or_else(|| "untitled".into()),
        session,
        cwd,
        modified,
        count,
        snippet,
    })
}

/// Who wrote a record and its readable text: a prompt you typed, or the
/// text blocks of Claude's reply. Everything else (tool results, system
/// notes, subagent turns) is `None`.
fn message(record: &Value) -> Option<(Who, Vec<&str>)> {
    if record.get("isSidechain").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    let content = record.pointer("/message/content")?;
    let who = match record.get("type")?.as_str()? {
        "assistant" => Who::Claude,
        "user" if record.pointer("/origin/kind").and_then(Value::as_str) == Some("human") => Who::You,
        _ => return None,
    };
    let texts = match content {
        Value::String(s) => vec![s.as_str()],
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|b| b.get("text")?.as_str())
            .collect(),
        _ => return None,
    };
    Some((who, texts))
}

/// The folder from the first record that names one.
fn first_cwd(path: &Path) -> Option<PathBuf> {
    let file = File::open(path).ok()?;
    BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter(|line| line.contains("\"cwd\""))
        .find_map(|line| {
            let record: Value = serde_json::from_str(&line).ok()?;
            record.get("cwd")?.as_str().map(PathBuf::from)
        })
}

/// Simple case folding, one char to one char.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// The byte range of the first case-insensitive match of `needle` (already
/// folded) in `hay`.
fn find(hay: &str, needle: &[char]) -> Option<(usize, usize)> {
    if needle.is_empty() {
        return None;
    }
    let chars: Vec<(usize, char)> = hay.char_indices().collect();
    let start = chars.windows(needle.len()).position(|w| w.iter().zip(needle).all(|(&(_, c), &n)| fold(c) == n))?;
    let end = chars.get(start + needle.len()).map_or(hay.len(), |&(i, _)| i);
    Some((chars[start].0, end))
}

/// One line around the match: whitespace squashed, a little context before,
/// more after, with an ellipsis wherever text was cut.
fn cut(who: Who, text: &str, (start, end): (usize, usize)) -> Snippet {
    let before = squash(&text[..start]);
    let after = squash(&text[end..]);
    let skip = before.chars().count().saturating_sub(BEFORE);
    let mut before_cut: String = before.chars().skip(skip).collect();
    if skip > 0 {
        before_cut.insert(0, '…');
    }
    let mut after_cut: String = after.chars().take(AFTER).collect();
    if after.chars().count() > AFTER {
        after_cut.push('…');
    }
    Snippet {
        who,
        before: before_cut,
        matched: squash(&text[start..end]),
        after: after_cut,
    }
}

/// Every run of whitespace, newlines included, as one space.
fn squash(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut gap = false;
    for c in s.chars() {
        if c.is_whitespace() {
            gap = true;
        } else {
            if gap {
                out.push(' ');
            }
            gap = false;
            out.push(c);
        }
    }
    if gap {
        out.push(' ');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    fn folded(q: &str) -> Vec<char> {
        q.chars().map(fold).collect()
    }

    #[test]
    fn finds_regardless_of_case_and_returns_byte_ranges() {
        assert_eq!(find("Fix the Login bug", &folded("login")), Some((8, 13)));
        assert_eq!(find("café ÉTÉ", &folded("été")), Some((6, 11)));
        assert_eq!(find("short", &folded("much longer")), None);
        assert_eq!(find("anything", &[]), None);
    }

    #[test]
    fn snippets_are_one_line_with_cut_ends() {
        let text = format!("{}\n\nthe   Needle\nand then {}", "a".repeat(60), "b".repeat(200));
        let range = find(&text, &folded("needle")).unwrap();
        let s = cut(Who::Claude, &text, range);
        assert_eq!(s.matched, "Needle");
        assert!(s.before.starts_with('…') && s.before.ends_with("the "), "{:?}", s.before);
        assert_eq!(s.before.chars().count(), BEFORE + 1);
        assert!(s.after.starts_with(" and then b") && s.after.ends_with('…'));
        let short = cut(Who::You, "needle", (0, 6));
        assert_eq!((short.before.as_str(), short.after.as_str()), ("", ""));
    }

    #[test]
    fn raw_prefilter_only_when_safe() {
        let q = Query::new("Auth Flow");
        assert!(q.might_match(r#"{"text":"the auth flow broke"}"#));
        assert!(!q.might_match(r#"{"text":"nothing here"}"#));
        // JSON escapes quotes, so a quoted query can't be checked raw.
        assert!(Query::new("say \"hi\"").raw.is_none());
        assert!(Query::new("été").raw.is_none());
    }

    const HUMAN: &str = r#"{"type":"user","isSidechain":false,"origin":{"kind":"human"},"cwd":"CWD","message":{"role":"user","content":"how should the Radar sort tabs?"}}"#;
    const REPLY: &str = r#"{"type":"assistant","isSidechain":false,"cwd":"CWD","message":{"role":"assistant","content":[{"type":"tool_use","name":"Read","input":{"note":"radar in a tool call"}},{"type":"text","text":"The radar puts needs-input first."}]}}"#;
    const TOOL_RESULT: &str = r#"{"type":"user","cwd":"CWD","message":{"role":"user","content":[{"type":"tool_result","content":"radar.rs contents"}]}}"#;
    const SIDECHAIN: &str = r#"{"type":"assistant","isSidechain":true,"cwd":"CWD","message":{"content":[{"type":"text","text":"radar from a subagent"}]}}"#;
    const TITLE: &str = r#"{"type":"ai-title","aiTitle":"Radar ordering"}"#;

    /// A fake `projects` folder with one transcript per (session id, lines).
    fn projects(name: &str, sessions: &[(&str, &[&str])]) -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!("clawsh-search-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let cwd = root.join("work");
        let project = root.join("projects").join("-work");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(project.join("subagents")).unwrap();
        let cwd_json = cwd.display().to_string();
        for (id, lines) in sessions {
            let body = lines.iter().map(|l| l.replace("CWD", &cwd_json)).collect::<Vec<_>>().join("\n");
            std::fs::write(project.join(format!("{id}.jsonl")), &body).unwrap();
            std::fs::write(project.join("subagents").join(format!("{id}.jsonl")), &body).unwrap();
        }
        (root, cwd)
    }

    #[test]
    fn counts_prompts_and_replies_but_not_tools_or_subagents() {
        let (root, cwd) = projects("scan", &[("abc-1", &[TITLE, HUMAN, TOOL_RESULT, REPLY, SIDECHAIN])]);
        let path = root.join("projects/-work/abc-1.jsonl");
        let never = AtomicBool::new(false);
        let hit = scan(&path, SystemTime::now(), &Query::new("RADAR"), &never).unwrap();
        assert_eq!(hit.count, 2);
        assert_eq!(hit.session.id, "abc-1");
        assert_eq!(hit.cwd, cwd);
        assert_eq!(hit.title, "Radar ordering");
        assert_eq!(hit.snippet.who, Who::You);
        assert_eq!(hit.snippet.matched, "Radar");
        assert!(scan(&path, SystemTime::now(), &Query::new("contents"), &never).is_none(), "tool output");
        assert!(scan(&path, SystemTime::now(), &Query::new("subagent"), &never).is_none(), "sidechain");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_folder_that_is_gone_cannot_be_resumed() {
        let gone = HUMAN.replace("CWD", "/nonexistent/clawsh-test");
        let (root, _) = projects("gone", &[("abc-2", &[&gone])]);
        let path = root.join("projects/-work/abc-2.jsonl");
        assert!(scan(&path, SystemTime::now(), &Query::new("radar"), &AtomicBool::new(false)).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_full_search_streams_hits_then_finishes() {
        let (root, _) = projects("run", &[("one", &[HUMAN]), ("two", &[REPLY]), ("three", &[TOOL_RESULT])]);
        let (tx, rx) = mpsc::channel();
        let mut search = Search::new();
        search.query = "  radar ".into();
        search.start(Some(root.join("projects")), tx);
        assert!(search.scanning && !search.stale());
        loop {
            match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
                AppEvent::SearchHit(g, hit) => search.add(g, *hit),
                AppEvent::SearchDone(g) => {
                    search.finish(g);
                    break;
                }
                _ => {}
            }
        }
        assert!(!search.scanning);
        let mut ids: Vec<&str> = search.hits.iter().map(|h| h.session.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, ["one", "two"], "subagent copies are never listed");
        search.query.push('x');
        assert!(search.stale());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn hit(id: &str, secs: u64) -> Hit {
        Hit {
            session: SessionRef::new(id, None).unwrap(),
            cwd: PathBuf::from("/w"),
            title: id.into(),
            modified: SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
            count: 1,
            snippet: Snippet { who: Who::You, before: String::new(), matched: id.into(), after: String::new() },
        }
    }

    #[test]
    fn hits_stay_newest_first_and_ignore_old_searches() {
        let mut s = Search::new();
        s.query = "q".into();
        s.start(None, mpsc::channel().0);
        let g = s.generation;
        s.add(g, hit("mid", 20));
        s.add(g, hit("new", 30));
        s.add(g, hit("old", 10));
        s.add(g - 1, hit("stale", 99));
        let ids: Vec<&str> = s.hits.iter().map(|h| h.session.id.as_str()).collect();
        assert_eq!(ids, ["new", "mid", "old"]);
    }

    #[test]
    fn selection_follows_the_conversation_and_clamps() {
        let mut s = Search::new();
        s.query = "q".into();
        s.start(None, mpsc::channel().0);
        let g = s.generation;
        s.add(g, hit("a", 10));
        s.add(g, hit("b", 5));
        s.move_by(1);
        assert_eq!(s.selected_hit().unwrap().session.id, "b");
        // A newer result landing on top doesn't move the selection.
        s.add(g, hit("c", 20));
        assert_eq!(s.selected_hit().unwrap().session.id, "b");
        s.move_by(9);
        assert_eq!(s.selected_index(), 2);
        s.move_by(-9);
        assert_eq!(s.selected_hit().unwrap().session.id, "c");
    }

    #[test]
    fn keeps_only_the_newest_past_the_cap() {
        let mut s = Search::new();
        s.query = "q".into();
        s.start(None, mpsc::channel().0);
        let g = s.generation;
        for i in 0..=MAX_HITS as u64 {
            s.add(g, hit(&format!("s{i}"), i));
        }
        assert_eq!(s.hits.len(), MAX_HITS);
        assert!(s.capped);
        assert_eq!(s.hits.last().unwrap().session.id, "s1");
    }
}
