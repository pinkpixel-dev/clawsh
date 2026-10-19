//! What each Claude tab is doing, learned from Claude Code hooks.
//!
//! Every tab is launched with `claude --settings '<json>'`, which adds hooks
//! that run `clawsh __hook`. That hook reads Claude's JSON on stdin, turns it
//! into a [`Status`], and sends `"<pane> <status>"` to the app's Unix socket.
//! Settings hooks merge with the user's own, so nothing else is replaced.

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::event::AppEvent;
use crate::persist::SessionRef;

pub const PANE_ENV: &str = "CLAWSH_PANE";
pub const SOCKET_ENV: &str = "CLAWSH_SOCKET";

/// The subcommand the hooks run. Unlikely to clash with a real Claude argument.
pub const HOOK_ARG: &str = "__hook";

/// Hook events that change a tab's status.
const EVENTS: &[&str] = &[
    "SessionStart",
    "UserPromptSubmit",
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionRequest",
    "Notification",
    "Stop",
    "StopFailure",
];

/// The hook that marks the start of a turn.
const TURN_EVENT: &str = "UserPromptSubmit";

/// Notification types that mean Claude is waiting on the person.
const INPUT_NOTIFICATIONS: &[&str] = &[
    "permission_prompt",
    "elicitation_dialog",
    "elicitation_url_dialog",
    "agent_needs_input",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Idle,
    Working,
    NeedsInput,
    Done,
    Failed,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Idle => "idle",
            Status::Working => "working",
            Status::NeedsInput => "input",
            Status::Done => "done",
            Status::Failed => "failed",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "idle" => Status::Idle,
            "working" => Status::Working,
            "input" => Status::NeedsInput,
            "done" => Status::Done,
            "failed" => Status::Failed,
            _ => return None,
        })
    }

    /// Done and Failed are news; once you've looked at the tab they're not.
    pub fn seen(self) -> Self {
        match self {
            Status::Done | Status::Failed => Status::Idle,
            other => other,
        }
    }

    /// Which status a workspace shows when its tabs disagree.
    pub fn urgency(self) -> u8 {
        match self {
            Status::Idle => 0,
            Status::Done => 1,
            Status::Working => 2,
            Status::Failed => 3,
            Status::NeedsInput => 4,
        }
    }
}

/// Maps one hook payload to a status. `None` leaves the tab as it was.
pub fn classify(input: &Value) -> Option<Status> {
    let event = input.get("hook_event_name")?.as_str()?;
    let tool = input.get("tool_name").and_then(Value::as_str);
    Some(match event {
        "SessionStart" => Status::Idle,
        "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" => Status::Working,
        "PreToolUse" if tool == Some("AskUserQuestion") => Status::NeedsInput,
        "PreToolUse" => Status::Working,
        "PermissionRequest" => Status::NeedsInput,
        "Notification" => {
            let kind = input.get("notification_type").and_then(Value::as_str)?;
            if !INPUT_NOTIFICATIONS.contains(&kind) {
                return None;
            }
            Status::NeedsInput
        }
        "Stop" => Status::Done,
        "StopFailure" => Status::Failed,
        _ => return None,
    })
}

/// The `--settings` JSON that adds our hooks for every status event.
pub fn hook_settings(exe: &Path) -> String {
    let command = format!("{} {HOOK_ARG}", shell_quote(&exe.to_string_lossy()));
    let group = json!([{ "hooks": [{ "type": "command", "command": command, "timeout": 5 }] }]);
    let hooks: serde_json::Map<String, Value> = EVENTS
        .iter()
        .map(|e| (e.to_string(), group.clone()))
        .collect();
    json!({ "hooks": hooks }).to_string()
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// `clawsh __hook`: report one hook event and get out of Claude's way.
/// Never prints and never fails, whatever happens.
pub fn run_hook() {
    let mut raw = String::new();
    let _ = std::io::stdin().take(1 << 20).read_to_string(&mut raw);
    let (Ok(pane), Ok(socket)) = (std::env::var(PANE_ENV), std::env::var(SOCKET_ENV)) else {
        return;
    };
    let Ok(input) = serde_json::from_str::<Value>(&raw) else {
        return;
    };
    let mut lines = String::new();
    // Every payload names its conversation, so `/clear` and `/resume` inside
    // Claude are picked up too. The transcript path goes last since it can
    // contain spaces.
    if let Some(id) = input.get("session_id").and_then(Value::as_str) {
        let transcript = input.get("transcript_path").and_then(Value::as_str).unwrap_or("");
        lines.push_str(&format!("{pane} session {id} {transcript}\n"));
    }
    if let Some(status) = classify(&input) {
        lines.push_str(&format!("{pane} {}\n", status.as_str()));
    }
    report(&socket, &lines);

    // Snapshot the worktree before Claude reads the prompt, for the review
    // view's last-turn scope. Claude waits for this hook, so nothing it
    // writes can land before the snapshot. Status goes first so the tab
    // shows working right away.
    if input.get("hook_event_name").and_then(Value::as_str) == Some(TURN_EVENT) {
        let dir = input
            .get("cwd")
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .or_else(|| std::env::current_dir().ok());
        if let Some(tree) = dir.and_then(|d| crate::review::git::snapshot(&d).ok()) {
            report(&socket, &format!("{pane} turn {tree}\n"));
        }
    }
}

fn report(socket: &str, lines: &str) {
    if lines.is_empty() {
        return;
    }
    if let Ok(mut stream) = UnixStream::connect(socket) {
        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
        let _ = stream.write_all(lines.as_bytes());
    }
}

/// Listens for hook reports. The socket file is removed when this drops.
pub struct StatusServer {
    path: PathBuf,
}

impl StatusServer {
    pub fn start(tx: Sender<AppEvent>) -> std::io::Result<Self> {
        let dir = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = dir.join(format!("clawsh-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;

        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                for line in BufReader::new(stream).lines().map_while(Result::ok) {
                    if let Some(event) = parse_report(&line)
                        && tx.send(event).is_err()
                    {
                        return;
                    }
                }
            }
        });
        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for StatusServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// `"<pane> <status>"`, `"<pane> session <id> <transcript path>"`, or
/// `"<pane> turn <tree id>"`.
fn parse_report(line: &str) -> Option<AppEvent> {
    let (pane, rest) = line.trim_end_matches(['\r', '\n']).split_once(' ')?;
    let pane: usize = pane.parse().ok()?;
    if let Some(tree) = rest.strip_prefix("turn ") {
        // SHA-1 or SHA-256 object IDs only; this ends up in git's argv.
        let valid = matches!(tree.len(), 40 | 64) && tree.bytes().all(|b| b.is_ascii_hexdigit());
        return valid.then(|| AppEvent::Turn(pane, tree.to_owned()));
    }
    if let Some(session) = rest.strip_prefix("session ") {
        let (id, transcript) = session.split_once(' ').unwrap_or((session, ""));
        let transcript = (!transcript.is_empty()).then(|| PathBuf::from(transcript));
        return Some(AppEvent::Session(pane, SessionRef::new(id, transcript)?));
    }
    Some(AppEvent::Status(pane, Status::parse(rest.trim())?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn ev(v: Value) -> Option<Status> {
        classify(&v)
    }

    #[test]
    fn hook_events_map_to_statuses() {
        assert_eq!(ev(json!({"hook_event_name": "UserPromptSubmit"})), Some(Status::Working));
        assert_eq!(ev(json!({"hook_event_name": "Stop"})), Some(Status::Done));
        assert_eq!(ev(json!({"hook_event_name": "PermissionRequest"})), Some(Status::NeedsInput));
        assert_eq!(
            ev(json!({"hook_event_name": "PreToolUse", "tool_name": "AskUserQuestion"})),
            Some(Status::NeedsInput)
        );
        assert_eq!(
            ev(json!({"hook_event_name": "PreToolUse", "tool_name": "Bash"})),
            Some(Status::Working)
        );
        assert_eq!(
            ev(json!({"hook_event_name": "Notification", "notification_type": "permission_prompt"})),
            Some(Status::NeedsInput)
        );
        assert_eq!(
            ev(json!({"hook_event_name": "Notification", "notification_type": "idle_prompt"})),
            None
        );
        assert_eq!(ev(json!({"hook_event_name": "SubagentStop"})), None);
        assert_eq!(ev(json!({"nope": 1})), None);
    }

    #[test]
    fn settings_json_has_a_hook_per_event() {
        let s: Value = serde_json::from_str(&hook_settings(Path::new("/opt/it's here/clawsh"))).unwrap();
        let hooks = s["hooks"].as_object().unwrap();
        assert_eq!(hooks.len(), EVENTS.len());
        let cmd = hooks["Stop"][0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(cmd, r"'/opt/it'\''s here/clawsh' __hook");
    }

    #[test]
    fn reports_round_trip_over_the_socket() {
        let (tx, rx) = mpsc::channel();
        let server = StatusServer::start(tx).unwrap();
        let mut stream = UnixStream::connect(server.path()).unwrap();
        writeln!(stream, "7 input").unwrap();
        writeln!(stream, "garbage").unwrap();
        writeln!(stream, "3 session ab-12 /home/me/My Projects/ab-12.jsonl").unwrap();
        writeln!(stream, "3 session bad;id /x").unwrap();
        writeln!(stream, "3 turn {}", "ab12".repeat(10)).unwrap();
        writeln!(stream, "3 turn --output=/tmp/x").unwrap();
        writeln!(stream, "3 done").unwrap();
        drop(stream);
        let next = || rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(next(), AppEvent::Status(7, Status::NeedsInput)));
        match next() {
            AppEvent::Session(3, s) => {
                assert_eq!(s.id, "ab-12");
                assert_eq!(s.transcript, Some(PathBuf::from("/home/me/My Projects/ab-12.jsonl")));
            }
            _ => panic!("expected a session report"),
        }
        assert!(matches!(next(), AppEvent::Turn(3, t) if t == "ab12".repeat(10)), "bad ids are dropped");
        assert!(matches!(next(), AppEvent::Status(3, Status::Done)), "bad trees are dropped");
        let path = server.path().to_path_buf();
        drop(server);
        assert!(!path.exists(), "socket cleaned up");
    }

    #[test]
    fn urgent_states_win_and_news_clears() {
        assert!(Status::NeedsInput.urgency() > Status::Working.urgency());
        assert!(Status::Working.urgency() > Status::Done.urgency());
        assert_eq!(Status::Done.seen(), Status::Idle);
        assert_eq!(Status::Working.seen(), Status::Working);
    }
}
