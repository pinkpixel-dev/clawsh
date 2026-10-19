use std::io::Write;
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use libghostty_vt::kitty::graphics::ImageFormat;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use super::*;
use crate::host::CellPx;

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

fn emulator() -> (Emulator, Arc<Mutex<Vec<u8>>>) {
    let replies = Arc::new(Mutex::new(Vec::new()));
    let writer: Arc<Mutex<Box<dyn Write + Send>>> =
        Arc::new(Mutex::new(Box::new(Sink(Arc::clone(&replies)))));
    let emu = Emulator::new(20, 4, CellPx { w: 10, h: 20 }, writer).unwrap();
    (emu, replies)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Uploads a 2x1 RGB image as Claude does: transmit plus a virtual placement.
fn upload_rgb(emu: &mut Emulator, id: u32) {
    let pixels = STANDARD.encode([255, 0, 0, 0, 0, 255]);
    let cmd = format!("\x1b_Ga=T,U=1,q=2,i={id},f=24,s=2,v=1,c=2,r=1;{pixels}\x1b\\");
    emu.feed(cmd.as_bytes());
}

#[test]
fn claude_sees_a_kitty_capable_terminal() {
    let (mut emu, replies) = emulator();
    // The same probes Claude sends at startup.
    emu.feed(b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\");
    emu.feed(b"\x1b[>0q");
    let replies = text(&replies.lock().unwrap());
    assert!(replies.contains("i=31;OK"), "graphics query: {replies:?}");
    assert!(replies.contains("ghostty "), "xtversion: {replies:?}");
}

#[test]
fn forwards_new_images_once() {
    let (mut emu, _) = emulator();
    upload_rgb(&mut emu, 7);
    let placements = emu.virtual_placements().unwrap();
    assert_eq!(placements.len(), 1);
    assert_eq!((placements[0].image_id, placements[0].cols, placements[0].rows), (7, 2, 1));

    let mut bridge = ImageBridge::new();
    let mut out = Vec::new();
    bridge.sync(0, &mut emu, &mut out).unwrap();
    let host = FIRST_HOST_ID;
    let out = text(&out);
    assert!(out.contains(&format!("a=t,q=2,i={host},f=24,s=2,v=1,m=0;/wAAAAD/")), "{out:?}");
    assert!(out.contains(&format!("a=p,U=1,q=2,i={host},p=1,c=2,r=1")), "{out:?}");
    assert_eq!(bridge.remap(0, 7), Some(host));

    let mut again = Vec::new();
    bridge.sync(0, &mut emu, &mut again).unwrap();
    assert!(again.is_empty(), "nothing changed, nothing sent");
}

#[test]
fn panes_never_share_host_ids() {
    let (mut a, _) = emulator();
    let (mut b, _) = emulator();
    upload_rgb(&mut a, 1);
    upload_rgb(&mut b, 1);
    let mut bridge = ImageBridge::new();
    let mut out = Vec::new();
    bridge.sync(0, &mut a, &mut out).unwrap();
    bridge.sync(1, &mut b, &mut out).unwrap();
    assert_ne!(bridge.remap(0, 1), bridge.remap(1, 1));
}

#[test]
fn deleted_images_leave_the_host() {
    let (mut emu, _) = emulator();
    upload_rgb(&mut emu, 7);
    let mut bridge = ImageBridge::new();
    let mut out = Vec::new();
    bridge.sync(0, &mut emu, &mut out).unwrap();

    emu.feed(b"\x1b_Ga=d,d=I,i=7,q=2\x1b\\");
    let mut out = Vec::new();
    bridge.sync(0, &mut emu, &mut out).unwrap();
    assert_eq!(text(&out), format!("\x1b_Ga=d,d=I,q=2,i={FIRST_HOST_ID}\x1b\\"));
    assert_eq!(bridge.remap(0, 7), None);
}

#[test]
fn placeholders_render_with_the_host_id() {
    let (mut emu, _) = emulator();
    upload_rgb(&mut emu, 7);
    // Two placeholder cells for image 7 (256-color fg), row 0, columns 0 and 1,
    // then one for an image that doesn't exist.
    emu.feed("\x1b[38;5;7m\u{10EEEE}\u{0305}\u{0305}\u{10EEEE}\u{0305}\u{030D}".as_bytes());
    emu.feed("\x1b[38;5;9m\u{10EEEE}\u{0305}\u{0305}\x1b[39m".as_bytes());

    let mut bridge = ImageBridge::new();
    bridge.sync(0, &mut emu, &mut Vec::new()).unwrap();

    let area = Rect::new(0, 0, 20, 4);
    let mut buf = Buffer::empty(area);
    emu.render(area, &mut buf, |id| bridge.remap(0, id)).unwrap();

    let host = FIRST_HOST_ID;
    let expected = Color::Rgb((host >> 16) as u8, (host >> 8) as u8, host as u8);
    assert!(buf[(0, 0)].symbol().starts_with('\u{10EEEE}'));
    assert_eq!(buf[(0, 0)].fg, expected);
    assert_eq!(buf[(1, 0)].symbol(), "\u{10EEEE}\u{0305}\u{030D}");
    assert_eq!(buf[(1, 0)].fg, expected);
    assert_eq!(buf[(2, 0)].symbol(), " ", "unknown images draw blank");
}

#[test]
fn png_uploads_are_decoded() {
    let mut png_bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut png_bytes, 2, 1);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&[10, 20, 30, 40, 50, 60]).unwrap();
    }
    let (mut emu, _) = emulator();
    let cmd = format!(
        "\x1b_Ga=T,U=1,q=2,i=3,f=100,c=1,r=1;{}\x1b\\",
        STANDARD.encode(&png_bytes)
    );
    emu.feed(cmd.as_bytes());

    let img = emu.image(3).unwrap().expect("png stored");
    assert_eq!(img.format, ImageFormat::Rgba);
    assert_eq!((img.width, img.height), (2, 1));
    assert_eq!(img.data, vec![10, 20, 30, 255, 40, 50, 60, 255]);
}

#[test]
fn large_images_are_chunked() {
    let img = ImageData {
        width: 64,
        height: 64,
        format: ImageFormat::Rgba,
        compressed: false,
        data: vec![7; 64 * 64 * 4],
    };
    let mut out = Vec::new();
    assert!(transmit(&mut out, 42, &img));
    let out = text(&out);
    // 16384 bytes -> 21848 base64 chars -> 6 chunks, only the last with m=0.
    assert_eq!(out.matches("\x1b_G").count(), 6);
    assert_eq!(out.matches("m=1;").count(), 5);
    assert_eq!(out.matches("m=0;").count(), 1);
    assert!(out.starts_with("\x1b_Ga=t,q=2,i=42,f=32,s=64,v=64,m=1;"));
}

#[test]
fn reuploads_send_one_placement_per_size() {
    let (mut emu, _) = emulator();
    let mut bridge = ImageBridge::new();
    // Claude re-sends an animated image with a=T each frame, and libghostty
    // keeps a new placement for every one of them.
    for _ in 0..5 {
        upload_rgb(&mut emu, 7);
    }
    let mut out = Vec::new();
    bridge.sync(0, &mut emu, &mut out).unwrap();
    let out = text(&out);
    assert_eq!(out.matches("a=p").count(), 1, "{out:?}");

    upload_rgb(&mut emu, 7);
    let mut out = Vec::new();
    bridge.sync(0, &mut emu, &mut out).unwrap();
    let out = text(&out);
    assert_eq!(out.matches("a=t").count(), 1, "new pixels go up again");
    assert_eq!(out.matches("a=p").count(), 1, "{out:?}");
}

#[test]
fn placeholders_drop_the_pane_placement_id() {
    let (mut emu, _) = emulator();
    upload_rgb(&mut emu, 7);
    emu.feed("\x1b[38;5;7;58;5;3m\u{10EEEE}\u{0305}\u{0305}".as_bytes());
    let mut bridge = ImageBridge::new();
    bridge.sync(0, &mut emu, &mut Vec::new()).unwrap();
    let area = Rect::new(0, 0, 20, 4);
    let mut buf = Buffer::empty(area);
    emu.render(area, &mut buf, |id| bridge.remap(0, id)).unwrap();
    assert_eq!(buf[(0, 0)].underline_color, Color::Reset);
}
