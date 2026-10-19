//! Turns one file's unified diff into numbered lines for the review view.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// An `@@ ... @@` header.
    Hunk,
    Context,
    Add,
    Del,
    /// `\ No newline at end of file` and the like.
    Note,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: Kind,
    /// Line number on the old side, for context and removed lines.
    pub old: Option<u32>,
    /// Line number on the new side, for context and added lines.
    pub new: Option<u32>,
    /// The line without its `+`/`-`/space prefix, safe to draw.
    pub text: String,
}

impl DiffLine {
    /// Hunk headers and notes aren't code, so they can't take comments.
    pub fn is_code(&self) -> bool {
        matches!(self.kind, Kind::Context | Kind::Add | Kind::Del)
    }

    /// The line as it appears in a diff, prefix included.
    pub fn quoted(&self) -> String {
        let prefix = match self.kind {
            Kind::Add => '+',
            Kind::Del => '-',
            _ => ' ',
        };
        format!("{prefix}{}", self.text)
    }
}

/// Everything from the first hunk on. The file headers before it are skipped.
pub fn parse(diff: &str) -> Vec<DiffLine> {
    let mut out = Vec::new();
    let (mut old, mut new) = (0u32, 0u32);
    let mut in_hunk = false;
    for raw in diff.lines() {
        if raw.starts_with("@@") {
            if let Some((o, n)) = hunk_starts(raw) {
                (old, new) = (o, n);
                in_hunk = true;
                out.push(DiffLine { kind: Kind::Hunk, old: None, new: None, text: clean(raw) });
            }
            continue;
        }
        if !in_hunk {
            continue;
        }
        let (kind, rest) = match raw.chars().next() {
            Some('+') => (Kind::Add, &raw[1..]),
            Some('-') => (Kind::Del, &raw[1..]),
            Some('\\') => (Kind::Note, raw),
            Some(' ') => (Kind::Context, &raw[1..]),
            // Some tools strip the space from empty context lines.
            None => (Kind::Context, ""),
            Some(_) => continue,
        };
        let (o, n) = match kind {
            Kind::Context => (Some(old), Some(new)),
            Kind::Add => (None, Some(new)),
            Kind::Del => (Some(old), None),
            _ => (None, None),
        };
        if o.is_some() {
            old += 1;
        }
        if n.is_some() {
            new += 1;
        }
        out.push(DiffLine { kind, old: o, new: n, text: clean(rest) });
    }
    out
}

/// `@@ -12,3 +14,5 @@` gives (12, 14).
fn hunk_starts(header: &str) -> Option<(u32, u32)> {
    let mut parts = header.split_whitespace().skip(1);
    let start = |part: Option<&str>, sign: char| -> Option<u32> {
        part?.strip_prefix(sign)?.split(',').next()?.parse().ok()
    };
    Some((start(parts.next(), '-')?, start(parts.next(), '+')?))
}

/// Tabs become spaces and control characters go, so a line can't move the
/// cursor or change colors when drawn.
fn clean(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\t' => out.push_str("    "),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = "diff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ b/x\n\
@@ -3,3 +3,4 @@ fn main\n keep\n-gone\n+new\n+\tmore\n \n\\ No newline at end of file\n";

    #[test]
    fn numbers_lines_on_both_sides() {
        let lines = parse(DIFF);
        let got: Vec<_> = lines.iter().map(|l| (l.kind, l.old, l.new)).collect();
        assert_eq!(
            got,
            [
                (Kind::Hunk, None, None),
                (Kind::Context, Some(3), Some(3)),
                (Kind::Del, Some(4), None),
                (Kind::Add, None, Some(4)),
                (Kind::Add, None, Some(5)),
                (Kind::Context, Some(5), Some(6)),
                (Kind::Note, None, None),
            ]
        );
        assert_eq!(lines[4].text, "    more");
        assert_eq!(lines[2].quoted(), "-gone");
        assert!(!lines[0].is_code() && lines[1].is_code());
    }

    #[test]
    fn hunk_headers_without_counts_parse() {
        assert_eq!(hunk_starts("@@ -0,0 +1 @@"), Some((0, 1)));
        assert_eq!(hunk_starts("@@ nonsense"), None);
    }

    #[test]
    fn escapes_are_stripped() {
        let lines = parse("@@ -1 +1 @@\n+\x1b[31mred\n");
        assert_eq!(lines[1].text, "[31mred");
    }
}
