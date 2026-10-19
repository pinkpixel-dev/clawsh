use std::io::Cursor;

use libghostty_vt::alloc::{Allocator, Bytes};
use libghostty_vt::kitty::graphics::{DecodePng, DecodedImage};
use png::{ColorType, Transformations};

/// Decodes PNGs sent with the kitty protocol (`f=100`) into the RGBA8 pixels
/// libghostty stores.
///
/// libghostty-vt 0.2.2 ships `RustPngDecoder`, but it hands `next_frame` a
/// zero-length buffer, so every decode fails. This one sizes the buffer.
pub struct PngDecoder;

impl DecodePng for PngDecoder {
    fn decode_png<'alloc>(
        &mut self,
        alloc: &'alloc Allocator<'_>,
        data: &[u8],
    ) -> Option<DecodedImage<'alloc>> {
        let mut decoder = png::Decoder::new(Cursor::new(data));
        // 8 bits per channel with alpha. Grayscale stays two channels and is
        // widened below.
        decoder.set_transformations(Transformations::normalize_to_color8() | Transformations::ALPHA);
        let mut reader = decoder.read_info().ok()?;
        let mut buf = vec![0; reader.output_buffer_size()?];
        let info = reader.next_frame(&mut buf).ok()?;
        let pixels = &buf[..info.buffer_size()];

        let rgba = match info.color_type {
            ColorType::Rgba => pixels.to_vec(),
            ColorType::GrayscaleAlpha => pixels
                .chunks_exact(2)
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect(),
            _ => return None,
        };

        let mut bytes = Bytes::new_with_alloc(alloc, rgba.len()).ok()?;
        bytes.copy_from_slice(&rgba);
        Some(DecodedImage {
            width: info.width,
            height: info.height,
            data: bytes,
        })
    }
}
