use std::io::Write;
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::*;
use crate::host::CellPx;
use crate::pane::pty::SharedWriter;

struct Sink;

impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// A 20x5 pane showing `lines`, one per row, with the cursor on the last.
fn emulator(lines: &[&str]) -> Emulator {
    let writer: SharedWriter = Arc::new(Mutex::new(Box::new(Sink)));
    let mut emu = Emulator::new(20, 5, CellPx { w: 10, h: 20 }, Default::default(), writer).unwrap();
    emu.feed(lines.join("\r\n").as_bytes());
    emu
}

fn press(emu: &mut Emulator, copy: &mut CopyMode, keys: &str) -> CopyOutcome {
    let mut last = CopyOutcome::Stay;
    for ch in keys.chars() {
        let code = match ch {
            '\n' => KeyCode::Enter,
            '\x1b' => KeyCode::Esc,
            c => KeyCode::Char(c),
        };
        last = copy.key(emu, &KeyEvent::new(code, KeyModifiers::NONE)).unwrap();
    }
    last
}

/// The text the pane sent to the host clipboard, decoded.
fn clipboard(emu: &mut Emulator) -> String {
    let out = String::from_utf8(emu.take_host_output()).unwrap();
    let b64 = out
        .strip_prefix("\x1b]52;c;")
        .and_then(|s| s.strip_suffix("\x1b\\"))
        .unwrap_or_else(|| panic!("not an OSC 52 write: {out:?}"));
    String::from_utf8(STANDARD.decode(b64).unwrap()).unwrap()
}

#[test]
fn starts_at_the_programs_cursor() {
    let mut emu = emulator(&["one", "two", "three"]);
    let copy = CopyMode::enter(&mut emu).unwrap();
    assert_eq!(copy.cursor_in_view(&emu).unwrap(), Some(Position::new(5, 2)));
}

#[test]
fn selects_and_copies_with_v_and_y() {
    let mut emu = emulator(&["one", "two words", "three"]);
    let mut copy = CopyMode::enter(&mut emu).unwrap();
    // Up a line, to its start, select five cells, copy. (Trailing spaces
    // are trimmed, so ending on the space would copy just "two".)
    assert_eq!(press(&mut emu, &mut copy, "k0vllll"), CopyOutcome::Stay);
    assert_eq!(press(&mut emu, &mut copy, "y"), CopyOutcome::Done);
    assert_eq!(clipboard(&mut emu), "two w");
}

#[test]
fn y_without_a_selection_copies_the_line() {
    let mut emu = emulator(&["one", "two words", "three"]);
    let mut copy = CopyMode::enter(&mut emu).unwrap();
    press(&mut emu, &mut copy, "k\n");
    assert_eq!(clipboard(&mut emu), "two words");
}

#[test]
fn reaches_into_the_scrollback() {
    let lines: Vec<String> = (1..=30).map(|i| format!("line {i}")).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let mut emu = emulator(&refs);
    let mut copy = CopyMode::enter(&mut emu).unwrap();
    // Jump to the very top: the view follows, cursor in its first row.
    press(&mut emu, &mut copy, "g");
    assert_eq!(copy.cursor_in_view(&emu).unwrap(), Some(Position::new(0, 0)));
    press(&mut emu, &mut copy, "y");
    assert_eq!(clipboard(&mut emu), "line 1");
}

#[test]
fn leaving_returns_to_the_live_screen() {
    let lines: Vec<String> = (1..=30).map(|i| format!("line {i}")).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let mut emu = emulator(&refs);
    let mut copy = CopyMode::enter(&mut emu).unwrap();
    press(&mut emu, &mut copy, "gv");
    assert_eq!(press(&mut emu, &mut copy, "q"), CopyOutcome::Done);
    leave(&mut emu).unwrap();
    assert!(emu.take_host_output().is_empty(), "q copies nothing");
    let bar = emu.term.scrollbar().unwrap();
    assert_eq!(bar.offset + bar.len, bar.total, "back at the bottom");
}

#[test]
fn mouse_drags_select_by_view_position() {
    let mut emu = emulator(&["alpha beta", "gamma"]);
    let from = emu.spot_at_view(6, 0).unwrap();
    let to = emu.spot_at_view(9, 0).unwrap();
    emu.select(from, to).unwrap();
    assert!(emu.copy_selection().unwrap());
    assert_eq!(clipboard(&mut emu), "beta");
    // A selection of nothing but blanks isn't worth a clipboard write.
    let blank = emu.spot_at_view(15, 3).unwrap();
    emu.select(blank, emu.spot_at_view(18, 3).unwrap()).unwrap();
    assert!(!emu.copy_selection().unwrap());
}
