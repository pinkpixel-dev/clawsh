//! The phone access popup: a QR code of the link, the link itself, and why
//! it's localhost only when Tailscale couldn't be used.

use qrcode::QrCode;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Widget};

use super::WebView;
use crate::theme;

/// Light margin around the code, in modules. Scanners want some.
const QUIET: usize = 2;
/// Fixed colors, not the theme's: cameras read dark modules on light far more
/// reliably than the other way around.
const DARK: Color = Color::Rgb(0x17, 0x18, 0x1a);
const LIGHT: Color = Color::Rgb(0xed, 0xed, 0xed);
const MIN_TEXT: u16 = 40;

pub fn draw(buf: &mut Buffer, screen: Rect, web: &WebView) {
    let code = QrCode::new(web.url.as_bytes()).ok();
    let modules = code.as_ref().map_or(0, |c| c.width() + 2 * QUIET);
    let (qr_w, qr_h) = (modules as u16, modules.div_ceil(2) as u16);

    // Room for the borders and one cell of padding on each side.
    let max_inner = screen.width.saturating_sub(4);
    let inner_w = qr_w.max(MIN_TEXT).min(max_inner);
    let url_lines = wrap(&web.url, inner_w as usize);
    let text_h = url_lines.len() as u16 + 1 + u16::from(web.note.is_some()) + 1;
    let fits = code.is_some() && qr_w <= inner_w && qr_h + text_h + 3 <= screen.height;
    let inner_h = if fits { qr_h + 1 + text_h } else { text_h + 1 };

    let w = (inner_w + 4).min(screen.width);
    let h = (inner_h + 2).min(screen.height);
    let area = Rect::new(
        screen.x + screen.width.saturating_sub(w) / 2,
        screen.y + screen.height.saturating_sub(h) / 3,
        w,
        h,
    );
    let block = Block::bordered()
        .title(Span::styled(" Phone access ", theme::accent()))
        .border_style(theme::border())
        .style(theme::panel());
    let inner = block.inner(area).inner(ratatui::layout::Margin::new(1, 0));
    Clear.render(area, buf);
    block.render(area, buf);

    let mut y = inner.y;
    if let (true, Some(code)) = (fits, &code) {
        let x0 = inner.x + inner.width.saturating_sub(qr_w) / 2;
        let colors = code.to_colors();
        let width = code.width();
        let dark = |mx: usize, my: usize| {
            mx >= QUIET
                && my >= QUIET
                && mx - QUIET < width
                && my - QUIET < width
                && colors[(my - QUIET) * width + (mx - QUIET)] == qrcode::Color::Dark
        };
        for row in 0..qr_h {
            for col in 0..qr_w {
                let (mx, my) = (col as usize, row as usize * 2);
                let symbol = match (dark(mx, my), dark(mx, my + 1)) {
                    (true, true) => "█",
                    (true, false) => "▀",
                    (false, true) => "▄",
                    (false, false) => " ",
                };
                let cell = &mut buf[(x0 + col, y + row)];
                cell.set_symbol(symbol).set_style(Style::new().fg(DARK).bg(LIGHT));
            }
        }
        y += qr_h + 1;
    } else if code.is_some() {
        Paragraph::new(Line::styled("Make the window bigger to see the QR code", theme::muted()))
            .render(Rect::new(inner.x, y, inner.width, 1), buf);
        y += 2;
    }

    let mut lines: Vec<Line> = url_lines.into_iter().map(Line::raw).collect();
    if let Some(note) = &web.note {
        lines.push(Line::styled(format!("Localhost only: {note}"), Style::new().fg(theme::palette().warn)));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled("any key closes · ctrl+b W stops", theme::muted()));
    let rest = Rect::new(inner.x, y, inner.width, inner.bottom().saturating_sub(y));
    Paragraph::new(lines).render(rest, buf);
}

/// Splits a link with no spaces into lines of `width` characters.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars.chunks(width.max(1)).map(|c| c.iter().collect()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_links_wrap_by_characters() {
        assert_eq!(wrap("abcdefg", 3), vec!["abc", "def", "g"]);
        assert_eq!(wrap("", 3), Vec::<String>::new());
    }

    #[test]
    fn a_tailnet_link_fits_a_normal_terminal() {
        let url = "https://sizzlebop-h18.tail1234.ts.net:7878/?t=854a523cb5f2a990d6fe88c2456ff1bc";
        let screen = Rect::new(0, 0, 120, 40);
        let mut buf = Buffer::empty(screen);
        draw(&mut buf, screen, &WebView { url: url.into(), note: None, connected: false });
        let text: String = buf.content.iter().map(|c| c.symbol()).collect();
        assert!(text.contains('█'), "QR drawn");
        assert!(text.contains("tail1234"), "link shown");
    }
}
