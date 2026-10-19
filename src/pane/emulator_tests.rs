use super::*;
use std::sync::{Arc, Mutex};

fn emulator(cols: u16, rows: u16) -> (Emulator, Arc<Mutex<Vec<u8>>>) {
    let replies = Arc::new(Mutex::new(Vec::new()));
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let writer: SharedWriter = Arc::new(Mutex::new(Box::new(Sink(Arc::clone(&replies)))));
    let emu = Emulator::new(cols, rows, CellPx { w: 10, h: 20 }, HostColors::default(), writer).unwrap();
    (emu, replies)
}

#[test]
fn renders_text_and_colors() {
    let (mut emu, _) = emulator(10, 2);
    emu.feed(b"hi \x1b[31mR\x1b[38;2;1;2;3mT\x1b[0m");
    let area = Rect::new(0, 0, 10, 2);
    let mut buf = Buffer::empty(area);
    let cursor = emu.render(area, &mut buf, |_| None).unwrap().unwrap();

    assert_eq!(buf[(0, 0)].symbol(), "h");
    assert_eq!(buf[(1, 0)].symbol(), "i");
    assert_eq!(buf[(3, 0)].fg, Color::Indexed(1));
    assert_eq!(buf[(4, 0)].fg, Color::Rgb(1, 2, 3));
    assert_eq!(buf[(5, 0)].fg, Color::Reset);
    assert_eq!(cursor.position, Position::new(5, 0));
}

#[test]
fn wide_glyph_skips_spacer() {
    let (mut emu, _) = emulator(6, 1);
    emu.feed("a漢b".as_bytes());
    let area = Rect::new(0, 0, 6, 1);
    let mut buf = Buffer::empty(area);
    emu.render(area, &mut buf, |_| None).unwrap();
    assert_eq!(buf[(1, 0)].symbol(), "漢");
    assert_eq!(buf[(3, 0)].symbol(), "b");
}

#[test]
fn reports_the_host_colors_to_programs() {
    let replies = Arc::new(Mutex::new(Vec::new()));
    struct Sink(Arc<Mutex<Vec<u8>>>);
    impl Write for Sink {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let writer: SharedWriter = Arc::new(Mutex::new(Box::new(Sink(Arc::clone(&replies)))));
    let light = HostColors {
        fg: Some(RgbColor { r: 0x22, g: 0x22, b: 0x22 }),
        bg: Some(RgbColor { r: 0xfa, g: 0xf8, b: 0xf5 }),
    };
    let mut emu = Emulator::new(20, 2, CellPx { w: 10, h: 20 }, light, writer).unwrap();
    // What Claude asks: the background color, then light or dark.
    emu.feed(b"\x1b]11;?\x1b\\\x1b[?996n");
    let replies = String::from_utf8_lossy(&replies.lock().unwrap()).into_owned();
    assert!(replies.contains("rgb:fafa/f8f8/f5f5"), "{replies:?}");
    assert!(replies.contains("\x1b[?997;2n"), "light scheme: {replies:?}");
}

#[test]
fn clipboard_writes_go_to_the_host() {
    let (mut emu, replies) = emulator(20, 2);
    // What Claude sends to copy "hello" (ST and BEL terminators).
    emu.feed(b"\x1b]52;c;aGVsbG8=\x1b\\");
    assert_eq!(emu.take_host_output(), b"\x1b]52;c;aGVsbG8=\x1b\\");
    emu.feed(b"\x1b]52;p;aGk=\x07");
    assert_eq!(emu.take_host_output(), b"\x1b]52;p;aGk=\x1b\\");
    assert!(emu.take_host_output().is_empty(), "taken once");
    // A read request gets no answer, so the clipboard stays private.
    emu.feed(b"\x1b]52;c;?\x1b\\");
    assert!(replies.lock().unwrap().is_empty());
    assert!(emu.take_host_output().is_empty());
}

#[test]
fn answers_size_queries_through_the_pty() {
    let (mut emu, replies) = emulator(80, 24);
    // CSI 18 t: report the text area size in characters.
    emu.feed(b"\x1b[18t");
    assert_eq!(replies.lock().unwrap().as_slice(), b"\x1b[8;24;80t");
}
