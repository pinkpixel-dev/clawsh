//! Review comments and the message they become when sent to Claude.

use super::diff::DiffLine;

/// Most quoted lines per comment in the message; the rest become `…`.
const MAX_QUOTE: usize = 12;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub path: String,
    /// The (old, new) line numbers it covers, which is how it finds its lines
    /// again after the diff reloads.
    pub spots: Vec<(Option<u32>, Option<u32>)>,
    /// The covered lines as diff text, for the message.
    pub quote: Vec<String>,
    pub text: String,
}

impl Comment {
    pub fn on(path: &str, lines: &[&DiffLine]) -> Self {
        Self {
            path: path.to_owned(),
            spots: lines.iter().map(|l| (l.old, l.new)).collect(),
            quote: lines.iter().map(|l| l.quoted()).collect(),
            text: String::new(),
        }
    }

    pub fn covers(&self, path: &str, line: &DiffLine) -> bool {
        line.is_code() && self.path == path && self.spots.contains(&(line.old, line.new))
    }

    /// `38`, `38-40`, or `removed 12-13` when it only covers deleted lines.
    pub fn label(&self) -> String {
        let range = |nums: Vec<u32>| match (nums.iter().min(), nums.iter().max()) {
            (Some(a), Some(b)) if a == b => Some(a.to_string()),
            (Some(a), Some(b)) => Some(format!("{a}-{b}")),
            _ => None,
        };
        range(self.spots.iter().filter_map(|s| s.1).collect())
            .or_else(|| range(self.spots.iter().filter_map(|s| s.0).collect()).map(|r| format!("removed {r}")))
            .unwrap_or_default()
    }
}

/// Everything you wrote, as one paste for Claude's prompt.
pub fn message(comments: &[Comment]) -> String {
    let mut out = String::from("Review comments on the current changes:\n");
    for c in comments {
        let label = c.label();
        let at = if label.starts_with("removed") {
            format!("{} ({label})", c.path)
        } else {
            format!("{}:{label}", c.path)
        };
        out.push_str(&format!("\n{at}\n```diff\n"));
        for line in c.quote.iter().take(MAX_QUOTE) {
            out.push_str(line);
            out.push('\n');
        }
        if c.quote.len() > MAX_QUOTE {
            out.push_str("…\n");
        }
        out.push_str("```\n");
        out.push_str(c.text.trim());
        out.push('\n');
    }
    out.trim_end().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::review::diff::Kind;

    fn line(kind: Kind, old: Option<u32>, new: Option<u32>, text: &str) -> DiffLine {
        DiffLine { kind, old, new, text: text.into() }
    }

    #[test]
    fn labels_prefer_new_line_numbers() {
        let add = line(Kind::Add, None, Some(40), "b");
        let ctx = line(Kind::Context, Some(36), Some(38), "a");
        let del = line(Kind::Del, Some(12), None, "x");
        assert_eq!(Comment::on("f", &[&ctx, &add]).label(), "38-40");
        assert_eq!(Comment::on("f", &[&add]).label(), "40");
        assert_eq!(Comment::on("f", &[&del]).label(), "removed 12");
    }

    #[test]
    fn covers_matches_path_and_numbers() {
        let add = line(Kind::Add, None, Some(4), "b");
        let c = Comment::on("src/a.rs", &[&add]);
        assert!(c.covers("src/a.rs", &add));
        assert!(!c.covers("src/b.rs", &add));
        assert!(!c.covers("src/a.rs", &line(Kind::Add, None, Some(5), "b")));
        assert!(!c.covers("src/a.rs", &line(Kind::Hunk, None, None, "@@")));
    }

    #[test]
    fn message_quotes_each_comment() {
        let add = line(Kind::Add, None, Some(7), "let x = 1;");
        let del = line(Kind::Del, Some(3), None, "old()");
        let mut a = Comment::on("src/a.rs", &[&add]);
        a.text = "  why 1?  ".into();
        let mut b = Comment::on("src/b.rs", &[&del]);
        b.text = "keep this".into();
        assert_eq!(
            message(&[a, b]),
            "Review comments on the current changes:\n\n\
             src/a.rs:7\n```diff\n+let x = 1;\n```\nwhy 1?\n\n\
             src/b.rs (removed 3)\n```diff\n-old()\n```\nkeep this"
        );
    }
}
