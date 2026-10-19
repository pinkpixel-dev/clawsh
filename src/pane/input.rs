use anyhow::Result;
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, ModifierKeyCode, MouseButton,
    MouseEvent, MouseEventKind,
};
use libghostty_vt::key::{self, Key, Mods};
use libghostty_vt::mouse;
use libghostty_vt::terminal::{Mode, ScrollViewport};
use libghostty_vt::{Terminal, focus, paste};
use ratatui::layout::Rect;

use crate::host::CellPx;

/// Lines moved per wheel tick when the app is not tracking the mouse.
const WHEEL_LINES: isize = 3;

/// Turns crossterm input events into the exact bytes the program in the pane
/// expects, honoring its modes (kitty keyboard flags, mouse format, etc.).
pub struct InputEncoder {
    key_enc: key::Encoder<'static>,
    key_ev: key::Event<'static>,
    mouse_enc: mouse::Encoder<'static>,
    mouse_ev: mouse::Event<'static>,
    out: Vec<u8>,
}

impl InputEncoder {
    pub fn new() -> Result<Self> {
        Ok(Self {
            key_enc: key::Encoder::new()?,
            key_ev: key::Event::new()?,
            mouse_enc: mouse::Encoder::new()?,
            mouse_ev: mouse::Event::new()?,
            out: Vec::with_capacity(64),
        })
    }

    pub fn key(&mut self, term: &Terminal<'_, '_>, ev: &KeyEvent) -> Result<&[u8]> {
        self.out.clear();
        let Some((key, unshifted, extra_mods)) = map_key(ev.code) else {
            return Ok(&self.out);
        };
        let mods = to_mods(ev.modifiers, ev.state) | extra_mods;

        // Plain and alt-modified printable keys carry their text; ctrl/super
        // combos are left for the encoder to build from the key itself.
        let text = match ev.code {
            KeyCode::Char(c) if !mods.intersects(Mods::CTRL | Mods::SUPER) => Some(c.to_string()),
            _ => None,
        };
        let consumed = if text.is_some() && mods.contains(Mods::SHIFT) {
            Mods::SHIFT
        } else {
            Mods::empty()
        };
        let action = match ev.kind {
            KeyEventKind::Press => key::Action::Press,
            KeyEventKind::Repeat => key::Action::Repeat,
            KeyEventKind::Release => key::Action::Release,
        };

        self.key_ev
            .set_action(action)
            .set_key(key)
            .set_mods(mods)
            .set_consumed_mods(consumed)
            .set_unshifted_codepoint(unshifted)
            .set_utf8(text);
        self.key_enc
            .set_options_from_terminal(term)
            .encode_to_vec(&self.key_ev, &mut self.out)?;
        Ok(&self.out)
    }

    /// `area` is where the pane is drawn on the host screen. Wheel events
    /// scroll the scrollback when the program is not tracking the mouse.
    pub fn mouse(
        &mut self,
        term: &mut Terminal<'_, '_>,
        ev: &MouseEvent,
        area: Rect,
        cell: CellPx,
    ) -> Result<&[u8]> {
        self.out.clear();
        if !area.contains((ev.column, ev.row).into()) {
            return Ok(&self.out);
        }

        let tracking = term.is_mouse_tracking()?;
        let wheel = match ev.kind {
            MouseEventKind::ScrollUp => Some((mouse::Button::Four, -WHEEL_LINES)),
            MouseEventKind::ScrollDown => Some((mouse::Button::Five, WHEEL_LINES)),
            MouseEventKind::ScrollLeft => Some((mouse::Button::Six, 0)),
            MouseEventKind::ScrollRight => Some((mouse::Button::Seven, 0)),
            _ => None,
        };
        if let Some((_, lines)) = wheel
            && !tracking
        {
            if lines != 0 {
                term.scroll_viewport(ScrollViewport::Delta(lines));
            }
            return Ok(&self.out);
        }

        // Aim for the middle of the cell so pixel-based formats agree with
        // cell-based ones.
        let x = (ev.column - area.x) as f32 * cell.w as f32 + cell.w as f32 / 2.0;
        let y = (ev.row - area.y) as f32 * cell.h as f32 + cell.h as f32 / 2.0;
        let held = matches!(ev.kind, MouseEventKind::Down(_) | MouseEventKind::Drag(_));

        self.mouse_ev
            .set_mods(to_mods(ev.modifiers, KeyEventState::empty()))
            .set_position(mouse::Position { x, y });
        self.mouse_enc
            .set_options_from_terminal(term)
            .set_size(mouse::EncoderSize {
                screen_width: area.width as u32 * cell.w,
                screen_height: area.height as u32 * cell.h,
                cell_width: cell.w,
                cell_height: cell.h,
                padding_top: 0,
                padding_bottom: 0,
                padding_right: 0,
                padding_left: 0,
            })
            .set_any_button_pressed(held)
            .set_track_last_cell(true);

        let steps: &[(mouse::Action, Option<mouse::Button>)] = match ev.kind {
            MouseEventKind::Down(b) => &[(mouse::Action::Press, Some(button(b)))],
            MouseEventKind::Up(b) => &[(mouse::Action::Release, Some(button(b)))],
            MouseEventKind::Drag(b) => &[(mouse::Action::Motion, Some(button(b)))],
            MouseEventKind::Moved => &[(mouse::Action::Motion, None)],
            _ => match wheel {
                Some((b, _)) => &[
                    (mouse::Action::Press, Some(b)),
                    (mouse::Action::Release, Some(b)),
                ],
                None => &[],
            },
        };
        for &(action, btn) in steps {
            self.mouse_ev.set_action(action).set_button(btn);
            self.mouse_enc.encode_to_vec(&self.mouse_ev, &mut self.out)?;
        }
        Ok(&self.out)
    }
}

pub fn paste_bytes(term: &Terminal<'_, '_>, text: &str) -> Result<Vec<u8>> {
    let bracketed = term.mode(Mode::BRACKETED_PASTE)?;
    let mut data = text.as_bytes().to_vec();
    // Worst case adds the two bracket markers (6 bytes each).
    let mut out = vec![0u8; data.len() + 16];
    let n = paste::encode(&mut data, bracketed, &mut out)?;
    out.truncate(n);
    Ok(out)
}

pub fn focus_bytes(term: &Terminal<'_, '_>, gained: bool) -> Result<Vec<u8>> {
    if !term.mode(Mode::FOCUS_EVENT)? {
        return Ok(Vec::new());
    }
    let ev = if gained {
        focus::Event::Gained
    } else {
        focus::Event::Lost
    };
    let mut out = [0u8; 8];
    let n = ev.encode(&mut out)?;
    Ok(out[..n].to_vec())
}

fn button(b: MouseButton) -> mouse::Button {
    match b {
        MouseButton::Left => mouse::Button::Left,
        MouseButton::Right => mouse::Button::Right,
        MouseButton::Middle => mouse::Button::Middle,
    }
}

fn to_mods(m: KeyModifiers, state: KeyEventState) -> Mods {
    let mut mods = Mods::empty();
    let pairs = [
        (KeyModifiers::SHIFT, Mods::SHIFT),
        (KeyModifiers::ALT, Mods::ALT),
        (KeyModifiers::CONTROL, Mods::CTRL),
        (KeyModifiers::SUPER, Mods::SUPER),
    ];
    for (from, to) in pairs {
        if m.contains(from) {
            mods |= to;
        }
    }
    if state.contains(KeyEventState::CAPS_LOCK) {
        mods |= Mods::CAPS_LOCK;
    }
    if state.contains(KeyEventState::NUM_LOCK) {
        mods |= Mods::NUM_LOCK;
    }
    mods
}

/// Maps a crossterm key to a physical key, its unshifted codepoint ('\0' when
/// it has none), and any modifier the key itself implies (BackTab = shift+tab).
pub(crate) fn map_key(code: KeyCode) -> Option<(Key, char, Mods)> {
    let none = Mods::empty();
    let named = |k| Some((k, '\0', none));
    match code {
        KeyCode::Char(c) => {
            let (key, base) = char_key(c);
            Some((key, base, none))
        }
        KeyCode::Enter => named(Key::Enter),
        KeyCode::Tab => named(Key::Tab),
        KeyCode::BackTab => Some((Key::Tab, '\0', Mods::SHIFT)),
        KeyCode::Backspace => named(Key::Backspace),
        KeyCode::Esc => named(Key::Escape),
        KeyCode::Left => named(Key::ArrowLeft),
        KeyCode::Right => named(Key::ArrowRight),
        KeyCode::Up => named(Key::ArrowUp),
        KeyCode::Down => named(Key::ArrowDown),
        KeyCode::Home => named(Key::Home),
        KeyCode::End => named(Key::End),
        KeyCode::PageUp => named(Key::PageUp),
        KeyCode::PageDown => named(Key::PageDown),
        KeyCode::Insert => named(Key::Insert),
        KeyCode::Delete => named(Key::Delete),
        KeyCode::F(n @ 1..=25) => Key::try_from(Key::F1 as u32 + n as u32 - 1)
            .ok()
            .map(|k| (k, '\0', none)),
        KeyCode::Modifier(m) => named(match m {
            ModifierKeyCode::LeftShift => Key::ShiftLeft,
            ModifierKeyCode::RightShift => Key::ShiftRight,
            ModifierKeyCode::LeftControl => Key::ControlLeft,
            ModifierKeyCode::RightControl => Key::ControlRight,
            ModifierKeyCode::LeftAlt => Key::AltLeft,
            ModifierKeyCode::RightAlt => Key::AltRight,
            ModifierKeyCode::LeftSuper | ModifierKeyCode::LeftMeta => Key::MetaLeft,
            ModifierKeyCode::RightSuper | ModifierKeyCode::RightMeta => Key::MetaRight,
            _ => return None,
        }),
        _ => None,
    }
}

/// The physical key (US layout) and unshifted character for a typed char.
fn char_key(c: char) -> (Key, char) {
    let offset = |base: Key, from: char, ch: char| {
        Key::try_from(base as u32 + (ch as u32 - from as u32)).unwrap_or(Key::Unidentified)
    };
    match c {
        'a'..='z' => (offset(Key::A, 'a', c), c),
        'A'..='Z' => (offset(Key::A, 'A', c), c.to_ascii_lowercase()),
        '0'..='9' => (offset(Key::Digit0, '0', c), c),
        ' ' => (Key::Space, ' '),
        '`' | '~' => (Key::Backquote, '`'),
        '-' | '_' => (Key::Minus, '-'),
        '=' | '+' => (Key::Equal, '='),
        '[' | '{' => (Key::BracketLeft, '['),
        ']' | '}' => (Key::BracketRight, ']'),
        '\\' | '|' => (Key::Backslash, '\\'),
        ';' | ':' => (Key::Semicolon, ';'),
        '\'' | '"' => (Key::Quote, '\''),
        ',' | '<' => (Key::Comma, ','),
        '.' | '>' => (Key::Period, '.'),
        '/' | '?' => (Key::Slash, '/'),
        '!' => (Key::Digit1, '1'),
        '@' => (Key::Digit2, '2'),
        '#' => (Key::Digit3, '3'),
        '$' => (Key::Digit4, '4'),
        '%' => (Key::Digit5, '5'),
        '^' => (Key::Digit6, '6'),
        '&' => (Key::Digit7, '7'),
        '*' => (Key::Digit8, '8'),
        '(' => (Key::Digit9, '9'),
        ')' => (Key::Digit0, '0'),
        _ => (Key::Unidentified, c),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libghostty_vt::terminal::Options;

    fn term() -> Terminal<'static, 'static> {
        Terminal::new(Options {
            cols: 80,
            rows: 24,
            max_scrollback: 0,
        })
        .unwrap()
    }

    fn press(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn maps_shifted_symbols_to_their_physical_key() {
        assert_eq!(char_key('Q'), (Key::Q, 'q'));
        assert_eq!(char_key('?'), (Key::Slash, '/'));
        assert_eq!(char_key('('), (Key::Digit9, '9'));
        assert_eq!(map_key(KeyCode::F(12)).unwrap().0, Key::F12);
        assert_eq!(map_key(KeyCode::BackTab).unwrap().2, Mods::SHIFT);
    }

    #[test]
    fn encodes_legacy_keys() {
        let t = term();
        let mut enc = InputEncoder::new().unwrap();
        assert_eq!(enc.key(&t, &press(KeyCode::Char('a'), KeyModifiers::NONE)).unwrap(), b"a");
        assert_eq!(enc.key(&t, &press(KeyCode::Char('c'), KeyModifiers::CONTROL)).unwrap(), b"\x03");
        assert_eq!(enc.key(&t, &press(KeyCode::Enter, KeyModifiers::NONE)).unwrap(), b"\r");
        assert_eq!(enc.key(&t, &press(KeyCode::Up, KeyModifiers::NONE)).unwrap(), b"\x1b[A");
        assert_eq!(enc.key(&t, &press(KeyCode::Char('x'), KeyModifiers::ALT)).unwrap(), b"\x1bx");
    }

    #[test]
    fn encodes_kitty_keys_once_the_program_asks() {
        let mut t = term();
        // CSI > 1 u: the program pushes "disambiguate" kitty keyboard flags.
        t.vt_write(b"\x1b[>1u");
        let mut enc = InputEncoder::new().unwrap();
        let out = enc.key(&t, &press(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        assert_eq!(out, b"\x1b[13;2u");
    }

    #[test]
    fn brackets_pastes_when_enabled() {
        let mut t = term();
        assert_eq!(paste_bytes(&t, "a\nb").unwrap(), b"a\rb");
        t.vt_write(b"\x1b[?2004h");
        assert_eq!(paste_bytes(&t, "hi").unwrap(), b"\x1b[200~hi\x1b[201~");
    }
}
