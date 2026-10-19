//! A small markdown renderer for the review view: headings, emphasis, code,
//! lists, quotes, rules, and tables, drawn with the chrome's colors.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme;

pub fn render(src: &str) -> Vec<Line<'static>> {
    let opts = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let mut r = Renderer::default();
    for event in Parser::new_ext(src, opts) {
        r.event(event);
    }
    r.flush();
    while r.lines.last().is_some_and(|l| l.spans.is_empty()) {
        r.lines.pop();
    }
    r.lines
}

#[derive(Default)]
struct Renderer {
    lines: Vec<Line<'static>>,
    spans: Vec<Span<'static>>,
    styles: Vec<Style>,
    /// Open lists, each with its next number (`None` for bullets).
    lists: Vec<Option<u64>>,
    quotes: usize,
    in_code: bool,
    link: Option<String>,
}

impl Renderer {
    fn style(&self) -> Style {
        self.styles.iter().fold(theme::panel(), |acc, s| acc.patch(*s))
    }

    fn text(&mut self, text: &str) {
        let style = self.style();
        self.spans.push(Span::styled(text.to_owned(), style));
    }

    /// Ends the current line, if it has anything on it.
    fn flush(&mut self) {
        if self.spans.is_empty() {
            return;
        }
        let mut spans = vec![Span::styled("│ ".repeat(self.quotes), theme::faint())];
        spans.append(&mut self.spans);
        self.lines.push(Line::from(spans));
    }

    fn blank(&mut self) {
        self.flush();
        if self.lines.last().is_some_and(|l| !l.spans.is_empty()) {
            self.lines.push(Line::default());
        }
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(t) if self.in_code => {
                for line in t.lines() {
                    self.spans.push(Span::styled(format!("    {line}"), theme::muted()));
                    self.flush();
                }
            }
            Event::Text(t) => self.text(&t),
            Event::Code(t) => {
                let style = self.style().fg(theme::palette().accent);
                self.spans.push(Span::styled(t.to_string(), style));
            }
            Event::InlineMath(t) | Event::DisplayMath(t) => self.text(&t),
            Event::Html(t) | Event::InlineHtml(t) => {
                self.spans.push(Span::styled(t.trim_end().to_owned(), theme::faint()));
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.blank();
                self.lines.push(Line::styled("─".repeat(40), theme::faint()));
                self.lines.push(Line::default());
            }
            Event::TaskListMarker(done) => self.text(if done { "[x] " } else { "[ ] " }),
            Event::FootnoteReference(name) => self.text(&format!("[{name}]")),
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Heading { level, .. } => {
                self.blank();
                let style = match level {
                    HeadingLevel::H1 | HeadingLevel::H2 => theme::accent(),
                    _ => Style::new().add_modifier(Modifier::BOLD),
                };
                self.styles.push(style);
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.quotes += 1;
            }
            Tag::CodeBlock(_) => {
                self.flush();
                self.in_code = true;
            }
            Tag::List(start) => {
                self.flush();
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush();
                let indent = "  ".repeat(self.lists.len().saturating_sub(1));
                let marker = match self.lists.last_mut() {
                    Some(Some(n)) => {
                        *n += 1;
                        format!("{indent}{}. ", *n - 1)
                    }
                    _ => format!("{indent}• "),
                };
                self.spans.push(Span::styled(marker, theme::muted()));
            }
            Tag::Emphasis => self.styles.push(Style::new().add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.styles.push(Style::new().add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self.styles.push(Style::new().add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { dest_url, .. } => {
                self.link = Some(dest_url.to_string());
                self.styles.push(Style::new().add_modifier(Modifier::UNDERLINED));
            }
            Tag::Image { dest_url, .. } => {
                self.link = Some(dest_url.to_string());
                self.text("image: ");
                self.styles.push(Style::new().add_modifier(Modifier::ITALIC));
            }
            Tag::TableHead => self.styles.push(Style::new().add_modifier(Modifier::BOLD)),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::HtmlBlock => {
                self.flush();
                if self.lists.is_empty() {
                    self.blank();
                }
            }
            TagEnd::Heading(_) => {
                self.styles.pop();
                self.blank();
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.quotes = self.quotes.saturating_sub(1);
                self.blank();
            }
            TagEnd::CodeBlock => {
                self.in_code = false;
                self.blank();
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.blank();
                }
            }
            TagEnd::Item => self.flush(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                self.styles.pop();
            }
            TagEnd::Link | TagEnd::Image => {
                self.styles.pop();
                if let Some(url) = self.link.take() {
                    self.spans.push(Span::styled(format!(" ({url})"), theme::faint()));
                }
            }
            TagEnd::TableCell => self.spans.push(Span::styled(" │ ", theme::faint())),
            TagEnd::TableHead => {
                self.styles.pop();
                // Drop the separator after the last cell.
                self.spans.pop();
                self.flush();
                self.lines.push(Line::styled("─".repeat(40), theme::faint()));
            }
            TagEnd::TableRow => {
                self.spans.pop();
                self.flush();
            }
            TagEnd::Table => self.blank(),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    #[test]
    fn renders_common_blocks() {
        let src = "# Title\n\nSome *text* and `code`.\n\n- one\n- two\n  1. nested\n\n> quoted\n\n```\nfn x() {}\n```\n";
        assert_eq!(
            plain(&render(src)),
            [
                "Title",
                "",
                "Some text and code.",
                "",
                "• one",
                "• two",
                "  1. nested",
                "",
                "│ quoted",
                "",
                "    fn x() {}",
            ]
        );
    }

    #[test]
    fn renders_tables_and_links() {
        let src = "| a | b |\n|---|---|\n| 1 | 2 |\n\n[site](https://x.dev)\n";
        let out = plain(&render(src));
        assert_eq!(out[0], "a │ b");
        assert!(out[1].starts_with('─'));
        assert_eq!(out[2], "1 │ 2");
        assert_eq!(out[4], "site (https://x.dev)");
    }
}
