//! Forwards kitty images from each pane's emulator to the host terminal.
//!
//! Claude draws images with kitty Unicode placeholders: it uploads an image
//! once, makes a virtual placement for it (`U=1`), and then prints placeholder
//! cells whose foreground color is the image ID. libghostty stores all of that
//! per pane. Here we upload the same pixels to the host under an ID of our own,
//! so two panes can never collide, and the renderer rewrites placeholder colors
//! to match. The host does the actual drawing, and the image scrolls and clips
//! with its cells for free.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use libghostty_vt::kitty::graphics::ImageFormat;

use crate::pane::{Emulator, ImageData};

/// Kitty caps a direct-transmission chunk at 4096 bytes of base64.
const CHUNK: usize = 4096;

/// The low 24 bits the placeholder color can carry.
const LOW_MASK: u32 = 0x00FF_FFFF;

/// Where host IDs start, so we're unlikely to clash with images some other
/// program left in the same terminal window.
const FIRST_HOST_ID: u32 = 0x00C1_A000;

struct Sent {
    /// Full host ID: the pane image's high byte plus our own low 24 bits.
    host_id: u32,
    generation: u64,
    /// Distinct placement sizes; the host placement ID is the index + 1.
    sizes: Vec<Size>,
}

/// A virtual placement's size in cells (0 lets the terminal work it out).
type Size = (u32, u32);

#[derive(Default)]
struct PaneImages {
    seen_generation: Option<u64>,
    /// Keyed by the pane's own image ID.
    images: HashMap<u32, Sent>,
}

pub struct ImageBridge {
    next_low: u32,
    panes: HashMap<usize, PaneImages>,
}

impl ImageBridge {
    pub fn new() -> Self {
        Self {
            next_low: FIRST_HOST_ID,
            panes: HashMap::new(),
        }
    }

    /// Brings the host up to date with one pane's images, appending the kitty
    /// commands to `out`. Cheap when nothing changed.
    pub fn sync(&mut self, pane: usize, emu: &mut Emulator, out: &mut Vec<u8>) -> Result<()> {
        let generation = emu.graphics_generation()?;
        let state = self.panes.entry(pane).or_default();
        if state.seen_generation == Some(generation) {
            return Ok(());
        }

        // libghostty gives every placement an ID, including the ones it makes
        // up for each `a=T` re-upload, so the same size can show up dozens of
        // times. Placeholders only need one placement per size.
        let mut wanted: BTreeMap<u32, Vec<Size>> = BTreeMap::new();
        for p in emu.virtual_placements()? {
            let sizes = wanted.entry(p.image_id).or_default();
            if !sizes.contains(&(p.cols, p.rows)) {
                sizes.push((p.cols, p.rows));
            }
        }

        // Images the pane dropped (or stopped placing) go away on the host too.
        state.images.retain(|id, sent| {
            let keep = wanted.contains_key(id);
            if !keep {
                delete(out, sent.host_id);
            }
            keep
        });

        for (id, sizes) in wanted {
            let Some(img_gen) = emu.image_generation(id)? else {
                continue;
            };
            let sent = state.images.get(&id);
            let stale = sent.is_none_or(|s| s.generation != img_gen);
            if stale {
                let Some(img) = emu.image(id)? else { continue };
                let host_id = match sent {
                    Some(s) => s.host_id,
                    None => {
                        let low = self.next_low;
                        self.next_low = next_low(low);
                        (id & !LOW_MASK) | low
                    }
                };
                // A fresh upload replaces the old pixels and drops the old
                // placements, so every placement goes out again.
                if !transmit(out, host_id, &img) {
                    continue;
                }
                for (i, size) in sizes.iter().enumerate() {
                    place(out, host_id, i, *size);
                }
                state.images.insert(
                    id,
                    Sent {
                        host_id,
                        generation: img_gen,
                        sizes,
                    },
                );
            } else if let Some(s) = state.images.get_mut(&id)
                && s.sizes != sizes
            {
                // Same placement ID, same slot: a changed size replaces the old one.
                for (i, size) in sizes.iter().enumerate() {
                    if s.sizes.get(i) != Some(size) {
                        place(out, s.host_id, i, *size);
                    }
                }
                s.sizes = sizes;
            }
        }

        state.seen_generation = Some(generation);
        Ok(())
    }

    /// The host-side low 24 bits for a placeholder color in `pane`.
    pub fn remap(&self, pane: usize, inner_low: u32) -> Option<u32> {
        let state = self.panes.get(&pane)?;
        state
            .images
            .iter()
            .find(|(id, _)| *id & LOW_MASK == inner_low)
            .map(|(_, s)| s.host_id & LOW_MASK)
    }

    /// Deletes one pane's images from the host, for when the pane closes.
    pub fn forget(&mut self, pane: usize, out: &mut Vec<u8>) {
        if let Some(state) = self.panes.remove(&pane) {
            for sent in state.images.values() {
                delete(out, sent.host_id);
            }
        }
    }

    /// Deletes every image this bridge put on the host.
    pub fn clear(&mut self, out: &mut Vec<u8>) {
        for state in self.panes.values() {
            for sent in state.images.values() {
                delete(out, sent.host_id);
            }
        }
        self.panes.clear();
    }
}

fn next_low(low: u32) -> u32 {
    let next = (low + 1) & LOW_MASK;
    if next == 0 { 1 } else { next }
}

/// Appends an `a=t` upload. Returns false for formats we can't forward.
fn transmit(out: &mut Vec<u8>, host_id: u32, img: &ImageData) -> bool {
    let widened;
    let (format, pixels): (u8, &[u8]) = match img.format {
        ImageFormat::Rgb => (24, &img.data),
        ImageFormat::Rgba => (32, &img.data),
        ImageFormat::Png => (100, &img.data),
        // Kitty has no grayscale formats on the wire, so widen them.
        ImageFormat::Gray if !img.compressed => {
            widened = img.data.iter().flat_map(|&g| [g, g, g]).collect::<Vec<u8>>();
            (24, &widened)
        }
        ImageFormat::GrayAlpha if !img.compressed => {
            widened = img
                .data
                .chunks_exact(2)
                .flat_map(|p| [p[0], p[0], p[0], p[1]])
                .collect::<Vec<u8>>();
            (32, &widened)
        }
        _ => return false,
    };
    if pixels.is_empty() {
        return false;
    }

    let encoded = STANDARD.encode(pixels);
    let mut chunks = encoded.as_bytes().chunks(CHUNK).peekable();
    let mut first = true;
    while let Some(chunk) = chunks.next() {
        let more = u8::from(chunks.peek().is_some());
        out.extend_from_slice(b"\x1b_G");
        if first {
            let _ = write!(out, "a=t,q=2,i={host_id},f={format}");
            if format != 100 {
                let _ = write!(out, ",s={},v={}", img.width, img.height);
            }
            if img.compressed {
                out.extend_from_slice(b",o=z");
            }
            out.push(b',');
            first = false;
        }
        let _ = write!(out, "m={more};");
        out.extend_from_slice(chunk);
        out.extend_from_slice(b"\x1b\\");
    }
    true
}

/// Appends a virtual placement for Unicode placeholders. Placement IDs are
/// ours (slot + 1), so sending the same slot again replaces it.
fn place(out: &mut Vec<u8>, host_id: u32, slot: usize, (cols, rows): Size) {
    let _ = write!(out, "\x1b_Ga=p,U=1,q=2,i={host_id},p={}", slot + 1);
    if cols != 0 {
        let _ = write!(out, ",c={cols}");
    }
    if rows != 0 {
        let _ = write!(out, ",r={rows}");
    }
    out.extend_from_slice(b"\x1b\\");
}

/// Appends a delete that also frees the image data (`d=I`).
fn delete(out: &mut Vec<u8>, host_id: u32) {
    let _ = write!(out, "\x1b_Ga=d,d=I,q=2,i={host_id}\x1b\\");
}

#[cfg(test)]
#[path = "graphics_tests.rs"]
mod tests;
