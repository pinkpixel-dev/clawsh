//! The built phone client in `web-dist`, which trunk builds from `web/`.
//! Debug builds read the folder from disk on every request, so a `trunk build`
//! shows up without rebuilding Claw$h. Release builds carry the files inside
//! the binary.

use std::borrow::Cow;

use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "web-dist"]
#[allow_missing = true]
struct Dist;

pub const NOT_BUILT: &str = "The phone client isn't built yet. Run `trunk build --release` in web/, then reload.";

/// A file's bytes and content type.
pub fn get(name: &str) -> Option<(Cow<'static, [u8]>, &'static str)> {
    let file = Dist::get(name)?;
    Some((file.data, content_type(name)))
}

fn content_type(name: &str) -> &'static str {
    match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("json") => "application/json",
        _ => "application/octet-stream",
    }
}
