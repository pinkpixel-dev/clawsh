use std::cell::Cell;
use std::io::Write;
use std::rc::Rc;

use anyhow::Result;
use libghostty_vt::kitty::graphics::{self as kitty, ImageFormat, PlacementIterator};
use libghostty_vt::render::{CellIterator, CursorVisualStyle, RenderState, RowIterator};
use libghostty_vt::screen::CellWide;
use libghostty_vt::style::{RgbColor, StyleColor, Underline};
use libghostty_vt::terminal::{
    ConformanceLevel, DeviceAttributeFeature, DeviceAttributes, DeviceType, Options,
    PrimaryDeviceAttributes, SecondaryDeviceAttributes, SizeReportSize,
};
use libghostty_vt::Terminal;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier, Style};

use super::png::PngDecoder;
use super::pty::SharedWriter;
use crate::host::CellPx;

const SCROLLBACK_LINES: usize = 10_000;

/// How much image data one pane may hold before libghostty evicts old images.
const IMAGE_STORAGE_BYTES: u64 = 64 * 1024 * 1024;

/// The kitty Unicode placeholder. A cell holding it shows part of an image
/// whose ID is the cell's foreground color.
pub const PLACEHOLDER: char = '\u{10EEEE}';

/// Claude only enables kitty graphics when XTVERSION names kitty or Ghostty.
/// The emulator is Ghostty's VT core, so we say so and add our own name.
const XTVERSION: &str = concat!("ghostty libghostty-vt (clawsh ", env!("CARGO_PKG_VERSION"), ")");

/// A kitty placement made for Unicode placeholders (`U=1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtualPlacement {
    pub image_id: u32,
    pub placement_id: u32,
    pub cols: u32,
    pub rows: u32,
}

/// A copy of one stored image, ready to send to the host terminal.
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    pub format: ImageFormat,
    pub compressed: bool,
    pub data: Vec<u8>,
}

/// Where the pane's cursor is and how it should look, in pane-local cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorState {
    pub position: Position,
    pub style: CursorVisualStyle,
    pub blinking: bool,
}

/// A libghostty-vt terminal plus the reusable render iterators it needs.
pub struct Emulator {
    term: Terminal<'static, 'static>,
    render: RenderState<'static>,
    rows: RowIterator<'static>,
    cells: CellIterator<'static>,
    placements: PlacementIterator<'static>,
    size: Rc<Cell<SizeReportSize>>,
    text: String,
}

impl Emulator {
    pub fn new(cols: u16, rows: u16, cell: CellPx, writer: SharedWriter) -> Result<Self> {
        let mut term = Terminal::new(Options {
            cols,
            rows,
            max_scrollback: SCROLLBACK_LINES,
        })?;
        // Options carry no pixel size, so set it right away. Size reports
        // and image placement both depend on it.
        term.resize(cols, rows, cell.w, cell.h)?;

        // The decoder is a thread-local, and every emulator lives on the
        // thread that creates it, so setting it here covers each one.
        kitty::set_png_decoder(Some(Box::new(PngDecoder)))?;
        // Storage starts at zero, which rejects every image. Claude also
        // sends pixels by file path and shared memory.
        term.set_kitty_image_storage_limit(IMAGE_STORAGE_BYTES)?
            .set_kitty_image_from_file_allowed(true)?
            .set_kitty_image_from_temp_file_allowed(true)?
            .set_kitty_image_from_shared_mem_allowed(true)?;

        let size = Rc::new(Cell::new(size_report(cols, rows, cell)));

        term.on_pty_write(move |_t, data| {
            if let Ok(mut w) = writer.lock() {
                let _ = w.write_all(data);
                let _ = w.flush();
            }
        })?
        .on_size({
            let size = Rc::clone(&size);
            move |_t| Some(size.get())
        })?
        .on_device_attributes(|_t| {
            Some(DeviceAttributes {
                primary: PrimaryDeviceAttributes::new(
                    ConformanceLevel::VT220,
                    &[
                        DeviceAttributeFeature::COLUMNS_132,
                        DeviceAttributeFeature::SELECTIVE_ERASE,
                        DeviceAttributeFeature::ANSI_COLOR,
                    ],
                ),
                secondary: SecondaryDeviceAttributes {
                    device_type: DeviceType::VT220,
                    firmware_version: 1,
                    rom_cartridge: 0,
                },
                tertiary: Default::default(),
            })
        })?
        .on_xtversion(|_t| Some(XTVERSION))?;

        Ok(Self {
            term,
            render: RenderState::new()?,
            rows: RowIterator::new()?,
            cells: CellIterator::new()?,
            placements: PlacementIterator::new()?,
            size,
            text: String::with_capacity(16),
        })
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.term.vt_write(bytes);
    }

    pub fn resize(&mut self, cols: u16, rows: u16, cell: CellPx) -> Result<()> {
        self.size.set(size_report(cols, rows, cell));
        self.term.resize(cols, rows, cell.w, cell.h)?;
        Ok(())
    }

    pub fn terminal(&self) -> &Terminal<'static, 'static> {
        &self.term
    }

    pub fn terminal_mut(&mut self) -> &mut Terminal<'static, 'static> {
        &mut self.term
    }

    pub fn title(&self) -> Option<String> {
        self.term
            .title()
            .ok()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    }

    /// Changes whenever anything in the image storage changes.
    pub fn graphics_generation(&self) -> Result<u64> {
        Ok(self.term.kitty_graphics()?.generation()?)
    }

    pub fn virtual_placements(&mut self) -> Result<Vec<VirtualPlacement>> {
        let graphics = self.term.kitty_graphics()?;
        let mut it = self.placements.update(&graphics)?;
        let mut out = Vec::new();
        while let Some(p) = it.next() {
            if p.is_virtual()? {
                out.push(VirtualPlacement {
                    image_id: p.image_id()?,
                    placement_id: p.placement_id()?,
                    cols: p.columns()?,
                    rows: p.rows()?,
                });
            }
        }
        Ok(out)
    }

    /// The image's generation, which changes when its pixels are replaced.
    pub fn image_generation(&self, id: u32) -> Result<Option<u64>> {
        let graphics = self.term.kitty_graphics()?;
        graphics.image(id).map(|img| img.generation()).transpose().map_err(Into::into)
    }

    pub fn image(&self, id: u32) -> Result<Option<ImageData>> {
        let graphics = self.term.kitty_graphics()?;
        let Some(img) = graphics.image(id) else {
            return Ok(None);
        };
        Ok(Some(ImageData {
            width: img.width()?,
            height: img.height()?,
            format: img.format()?,
            compressed: img.compression()? != kitty::Compression::None,
            data: img.data()?.to_vec(),
        }))
    }

    /// Copies the visible screen into `buf` at `area` and returns the cursor
    /// if it is visible. `remap` turns the image ID in a placeholder's color
    /// into the host terminal's ID; placeholders it can't map draw blank.
    pub fn render(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        remap: impl Fn(u32) -> Option<u32>,
    ) -> Result<Option<CursorState>> {
        let snapshot = self.render.update(&self.term)?;

        let cursor = match snapshot.cursor_viewport()? {
            Some(vp) if snapshot.cursor_visible()? => Some(CursorState {
                position: Position::new(vp.x, vp.y),
                style: snapshot.cursor_visual_style()?,
                blinking: snapshot.cursor_blinking()?,
            }),
            _ => None,
        };

        let mut rows = self.rows.update(&snapshot)?;
        let mut y = 0u16;
        while let Some(row) = rows.next() {
            if y >= area.height {
                break;
            }
            let mut cells = self.cells.update(row)?;
            let mut x = 0u16;
            while let Some(cell) = cells.next() {
                if x >= area.width {
                    break;
                }
                let pos = (area.x + x, area.y + y);
                x += 1;

                // Ratatui skips the cell after a wide glyph on its own, so
                // spacer cells are left alone.
                if matches!(
                    cell.raw_cell()?.wide()?,
                    CellWide::SpacerTail | CellWide::SpacerHead
                ) {
                    continue;
                }

                let ghostty_style = if cell.has_styling()? {
                    Some(cell.style()?)
                } else {
                    None
                };
                let mut style = ghostty_style.map(to_ratatui_style).unwrap_or_default();

                let target = &mut buf[pos];
                if cell.graphemes_len()? == 0 {
                    // Blank cells can still carry a background (erase with color).
                    if style.bg.is_none() {
                        if let Some(bg) = cell.bg_color()? {
                            style.bg = Some(rgb(bg));
                        }
                    }
                    target.set_symbol(" ");
                } else {
                    self.text.clear();
                    cell.graphemes_utf8(&mut self.text)?;
                    if self.text.starts_with(PLACEHOLDER) {
                        let inner = ghostty_style.map_or(0, |s| placeholder_id(s.fg_color));
                        match remap(inner) {
                            Some(host) => {
                                style.fg = Some(Color::Rgb((host >> 16) as u8, (host >> 8) as u8, host as u8));
                                // Underline color would name a placement by the
                                // pane's own ID, which the host doesn't have.
                                style.underline_color = None;
                                target.set_symbol(&self.text);
                            }
                            None => {
                                style = Style::default();
                                target.set_symbol(" ");
                            }
                        }
                    } else {
                        target.set_symbol(&self.text);
                    }
                }
                if cell.is_selected()? {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                target.set_style(style);
            }
            y += 1;
        }

        Ok(cursor.filter(|c| c.position.x < area.width && c.position.y < area.height))
    }
}

fn size_report(cols: u16, rows: u16, cell: CellPx) -> SizeReportSize {
    SizeReportSize {
        rows,
        columns: cols,
        cell_width: cell.w,
        cell_height: cell.h,
    }
}

/// The low 24 bits of an image ID, as a placeholder's foreground encodes them
/// (a 256-color index for small IDs, otherwise truecolor). The high byte lives
/// in the third diacritic, which we never touch.
pub(crate) fn placeholder_id(fg: StyleColor) -> u32 {
    match fg {
        StyleColor::None => 0,
        StyleColor::Palette(i) => i.0 as u32,
        StyleColor::Rgb(c) => (c.r as u32) << 16 | (c.g as u32) << 8 | c.b as u32,
    }
}

fn rgb(c: RgbColor) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

/// Palette colors stay indexed so the host terminal's own theme is used.
fn to_color(c: StyleColor) -> Option<Color> {
    match c {
        StyleColor::None => None,
        StyleColor::Palette(i) => Some(Color::Indexed(i.0)),
        StyleColor::Rgb(c) => Some(rgb(c)),
    }
}

pub(crate) fn to_ratatui_style(s: libghostty_vt::style::Style) -> Style {
    let mut style = Style::default();
    if let Some(fg) = to_color(s.fg_color) {
        style = style.fg(fg);
    }
    if let Some(bg) = to_color(s.bg_color) {
        style = style.bg(bg);
    }
    if let Some(ul) = to_color(s.underline_color) {
        style = style.underline_color(ul);
    }
    let flags = [
        (s.bold, Modifier::BOLD),
        (s.faint, Modifier::DIM),
        (s.italic, Modifier::ITALIC),
        (s.blink, Modifier::SLOW_BLINK),
        (s.inverse, Modifier::REVERSED),
        (s.invisible, Modifier::HIDDEN),
        (s.strikethrough, Modifier::CROSSED_OUT),
        (s.underline != Underline::None, Modifier::UNDERLINED),
    ];
    for (on, m) in flags {
        if on {
            style = style.add_modifier(m);
        }
    }
    style
}

#[cfg(test)]
mod tests {
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
        let emu = Emulator::new(cols, rows, CellPx { w: 10, h: 20 }, writer).unwrap();
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
    fn answers_size_queries_through_the_pty() {
        let (mut emu, replies) = emulator(80, 24);
        // CSI 18 t: report the text area size in characters.
        emu.feed(b"\x1b[18t");
        assert_eq!(replies.lock().unwrap().as_slice(), b"\x1b[8;24;80t");
    }
}
