//! What to call a tab when nobody named it.
//!
//! Claude's own terminal title comes first, cleaned up. When it has nothing
//! useful (a session that only ran slash commands never gets one), the
//! transcript fills in: the `/rename` name, then Claude's generated title,
//! then the first prompt you typed.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde_json::Value;

/// Names that say nothing about the work in the tab.
const GENERIC: [&str; 2] = ["claude", "claude code"];

/// Claude's terminal title without the status glyph it puts in front
/// (a spinner while working, `✳` when idle). `None` when what's left is
/// empty or just Claude's own name.
pub fn clean(raw: &str) -> Option<String> {
    let text: String = raw.chars().filter(|c| !c.is_control()).collect();
    let text = text.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '/').trim();
    let generic = GENERIC.iter().any(|g| text.eq_ignore_ascii_case(g));
    (!text.is_empty() && !generic).then(|| text.to_owned())
}

/// The best name a transcript has to offer.
pub fn from_transcript(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut custom = None;
    let mut ai = None;
    let mut prompt = None;
    let mut command = None;
    for line in BufReader::new(file).lines() {
        let Ok(line) = line else { break };
        // Most lines are tool calls and replies; skip them before parsing.
        let wanted = line.contains("\"custom-title\"")
            || line.contains("\"ai-title\"")
            || (prompt.is_none() && line.contains("\"human\""))
            || (command.is_none() && line.contains("<command-name>"));
        if !wanted {
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(&line) else { continue };
        let text = |key: &str| record.get(key).and_then(Value::as_str).and_then(clean);
        match record.get("type").and_then(Value::as_str) {
            // Both are last-wins in Claude's own reader.
            Some("custom-title") => custom = text("customTitle").or(custom),
            Some("ai-title") => ai = text("aiTitle").or(ai),
            _ => {}
        }
        if prompt.is_none() && record.pointer("/origin/kind").and_then(Value::as_str) == Some("human") {
            prompt = record.pointer("/message/content").and_then(message_text).and_then(first_line);
        }
        if command.is_none() {
            let content = record.get("content").or_else(|| record.pointer("/message/content"));
            command = content.and_then(message_text).and_then(slash_command);
        }
    }
    custom.or(ai).or(prompt).or(command)
}

/// The text of a message, whether it's a plain string or content blocks.
fn message_text(content: &Value) -> Option<&str> {
    match content {
        Value::String(s) => Some(s),
        Value::Array(blocks) => blocks.iter().find_map(|b| b.get("text")?.as_str()),
        _ => None,
    }
}

/// A long prompt only needs its first line to name a tab.
fn first_line(text: &str) -> Option<String> {
    text.lines().map(str::trim).find(|l| !l.is_empty()).and_then(clean)
}

/// `/code-review spec.md` from Claude's `<command-name>` and
/// `<command-args>` tags, without the slash.
fn slash_command(text: &str) -> Option<String> {
    let tag = |name: &str| {
        let open = format!("<{name}>");
        let start = text.find(&open)? + open.len();
        let end = start + text[start..].find(&format!("</{name}>"))?;
        Some(text[start..end].trim())
    };
    let name = tag("command-name")?.trim_start_matches('/');
    let args = tag("command-args").unwrap_or("");
    clean(format!("{name} {args}").trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_claudes_status_glyph() {
        assert_eq!(clean("✳ Remaining roadmap item").as_deref(), Some("Remaining roadmap item"));
        assert_eq!(clean("⠂ Fix the auth flow").as_deref(), Some("Fix the auth flow"));
        assert_eq!(clean("  plain title ").as_deref(), Some("plain title"));
    }

    #[test]
    fn claudes_own_name_is_not_a_title() {
        assert_eq!(clean("✳ Claude Code"), None);
        assert_eq!(clean("claude"), None);
        assert_eq!(clean("✳ "), None);
        assert_eq!(clean("\x07"), None);
    }

    fn transcript(name: &str, lines: &[&str]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("clawsh-title-{name}-{}.jsonl", std::process::id()));
        std::fs::write(&path, lines.join("\n")).unwrap();
        path
    }

    const PROMPT: &str = r#"{"type":"user","origin":{"kind":"human"},"message":{"role":"user","content":"fix the login bug\nit breaks on refresh"}}"#;
    const AI: &str = r#"{"type":"ai-title","aiTitle":"Fix login refresh bug"}"#;

    #[test]
    fn rename_beats_generated_title_beats_prompt() {
        let renamed = r#"{"type":"custom-title","customTitle":"auth work"}"#;
        let p = transcript("rename", &[PROMPT, AI, renamed, AI]);
        assert_eq!(from_transcript(&p).as_deref(), Some("auth work"));
        let p = transcript("ai", &[PROMPT, AI]);
        assert_eq!(from_transcript(&p).as_deref(), Some("Fix login refresh bug"));
        let p = transcript("prompt", &[PROMPT]);
        assert_eq!(from_transcript(&p).as_deref(), Some("fix the login bug"));
    }

    #[test]
    fn a_slash_command_session_is_named_after_the_command() {
        let cmd = r#"{"type":"system","subtype":"local_command","content":"<command-name>/code-review</command-name>\n<command-message>code-review</command-message>\n<command-args>spec.md</command-args>"}"#;
        let p = transcript("cmd", &[cmd]);
        assert_eq!(from_transcript(&p).as_deref(), Some("code-review spec.md"));
    }

    #[test]
    fn prompts_not_typed_by_you_are_skipped() {
        let caveat = r#"{"type":"user","message":{"role":"user","content":"Caveat: the messages below were generated"}}"#;
        let p = transcript("caveat", &[caveat, PROMPT]);
        assert_eq!(from_transcript(&p).as_deref(), Some("fix the login bug"));
        assert_eq!(from_transcript(Path::new("/nonexistent/x.jsonl")), None);
    }
}
