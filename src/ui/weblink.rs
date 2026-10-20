//! The phone access popup: a QR code of the one-time pairing link, the same
//! code written out for typing, and where paired phones go.

use qrcode::QrCode;
use ratatui::buffer::Buffer;
use ratatui::layout::{Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
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
const MIN_TEXT: u16 = 50;

pub fn draw(buf: &mut Buffer, screen: Rect, web: &WebView) {
    let code = web.link.as_deref().and_then(|link| QrCode::new(link.as_bytes()).ok());
    let modules = code.as_ref().map_or(0, |c| c.width() + 2 * QUIET);
    let (qr_w, qr_h) = (modules as u16, modules.div_ceil(2) as u16);

    // Room for the borders and one cell of padding on each side.
    let inner_w = qr_w.max(MIN_TEXT).min(screen.width.saturating_sub(4));
    let lines = text(web, inner_w as usize);
    let text_h = lines.len() as u16;
    let fits = code.is_some() && qr_w <= inner_w && qr_h + text_h + 3 <= screen.height;
    let inner_h = if fits { qr_h + 1 + text_h } else { text_h };

    let w = (inner_w + 4).min(screen.width);
    let h = (inner_h + 2).min(screen.height);
    let area = Rect::new(
        screen.x + screen.width.saturating_sub(w) / 2,
        screen.y + screen.height.saturating_sub(h) / 3,
        w,
        h,
    );
    let block = Block::bordered()
        .title(Span::styled(" Pair a phone ", theme::accent()))
        .border_style(theme::border())
        .style(theme::panel());
    let inner = block.inner(area).inner(Margin::new(1, 0));
    Clear.render(area, buf);
    block.render(area, buf);

    let mut y = inner.y;
    if let (true, Some(code)) = (fits, &code) {
        draw_qr(buf, inner.x + inner.width.saturating_sub(qr_w) / 2, y, code, (qr_w, qr_h));
        y += qr_h + 1;
    }
    let rest = Rect::new(inner.x, y, inner.width, inner.bottom().saturating_sub(y));
    Paragraph::new(lines).render(rest, buf);
}

fn draw_qr(buf: &mut Buffer, x0: u16, y0: u16, code: &QrCode, (w, h): (u16, u16)) {
    let colors = code.to_colors();
    let width = code.width();
    let dark = |mx: usize, my: usize| {
        mx >= QUIET
            && my >= QUIET
            && mx - QUIET < width
            && my - QUIET < width
            && colors[(my - QUIET) * width + (mx - QUIET)] == qrcode::Color::Dark
    };
    for row in 0..h {
        for col in 0..w {
            let (mx, my) = (col as usize, row as usize * 2);
            let symbol = match (dark(mx, my), dark(mx, my + 1)) {
                (true, true) => "█",
                (true, false) => "▀",
                (false, true) => "▄",
                (false, false) => " ",
            };
            buf[(x0 + col, y0 + row)].set_symbol(symbol).set_style(Style::new().fg(DARK).bg(LIGHT));
        }
    }
}

fn text(web: &WebView, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    match &web.code {
        Some(code) => {
            let mut first = vec![
                Span::styled("Code ", theme::muted()),
                Span::styled(code.clone(), Style::new().add_modifier(Modifier::BOLD)),
                Span::styled(format!("  {} min left", web.minutes_left), theme::muted()),
            ];
            if web.watch_only {
                first.push(Span::styled("  watch only", Style::new().fg(theme::palette().warn)));
            }
            lines.push(Line::from(first));
        }
        None => lines.push(Line::styled("That code was used or ran out.", theme::muted())),
    }
    lines.push(Line::styled("Paired phones open", theme::muted()));
    lines.extend(wrap(&web.home, width).into_iter().map(Line::raw));
    lines.push(Line::raw(""));
    let watch = if web.watch_only { "full access" } else { "watch only" };
    lines.push(Line::styled(format!("r {watch} · n new code · any key closes"), theme::muted()));
    lines
}

/// Splits a link with no spaces into lines of `width` characters.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    chars.chunks(width.max(1)).map(|c| c.iter().collect()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(link: Option<&str>) -> WebView {
        WebView {
            link: link.map(Into::into),
            code: link.map(|_| "K7QM-2XPV".into()),
            minutes_left: 5,
            watch_only: false,
            home: "https://sizzlebop-h18.tail1234.ts.net:7878/".into(),
            connected: false,
        }
    }

    fn render(web: &WebView) -> String {
        let screen = Rect::new(0, 0, 120, 40);
        let mut buf = Buffer::empty(screen);
        draw(&mut buf, screen, web);
        buf.content.iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn long_links_wrap_by_characters() {
        assert_eq!(wrap("abcdefg", 3), vec!["abc", "def", "g"]);
        assert_eq!(wrap("", 3), Vec::<String>::new());
    }

    #[test]
    fn a_pairing_link_draws_a_qr_and_the_code() {
        let text = render(&view(Some("https://sizzlebop-h18.tail1234.ts.net:7878/pair?c=K7QM2XPV")));
        assert!(text.contains('█'), "QR drawn");
        assert!(text.contains("K7QM-2XPV"));
        assert!(text.contains("tail1234"), "home link shown");
    }

    #[test]
    fn a_used_code_says_so_and_has_no_qr() {
        let text = render(&view(None));
        assert!(!text.contains('█'));
        assert!(text.contains("used or ran out"));
    }
}
