//! Device-only attachment thumbnails (109.15, Q109-14a). No fetch path.
use std::collections::{HashMap, VecDeque};
use std::io::Cursor;
use std::sync::{LazyLock, Mutex};

use image::{ImageFormat, ImageReader};

use super::attachments::{self, OpenError, Staging, ATTACHMENT_MAX_BYTES};
use super::sessions::{read, text, Source};
use serde_json::Value;

pub const LONG_EDGE: u32 = 256;
const MAX_PIXELS: u64 = 40_000_000;
const CACHE_ENTRIES: usize = 64;

type Cache = (HashMap<blake3::Hash, Vec<u8>>, VecDeque<blake3::Hash>);
static CACHE: LazyLock<Mutex<Cache>> = LazyLock::new(Mutex::default);

/// Request routing belongs to core, not to the shell's protocol adapter.
pub fn requested(uri: &str) -> bool {
    uri.split_once('?')
        .is_some_and(|(_, query)| query.split('&').any(|part| part == "thumbnail=1"))
}

/// Only raster formats supported by the bounded decoder. HEIC stays a file chip.
pub fn supported(mime: &str) -> bool {
    matches!(
        mime,
        "image/png" | "image/jpeg" | "image/webp" | "image/gif"
    )
}

/// Decode with dimension and allocation limits before allocating a raster; cache
/// at most 64 small PNGs. Invalid and oversized input is never served as an image.
pub fn render(bytes: &[u8]) -> Result<Vec<u8>, OpenError> {
    if bytes.len() as u64 > ATTACHMENT_MAX_BYTES {
        return Err(OpenError::NotHere);
    }
    let hash = blake3::hash(bytes);
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(png) = cache.0.get(&hash) {
        return Ok(png.clone());
    }
    let fail = |_| OpenError::NotHere;
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(fail)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(MAX_PIXELS * 4);
    reader.limits(limits);
    // Pixel count is stricter than the independent width/height bounds.
    let dimensions = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(fail)?
        .into_dimensions()
        .map_err(|_| OpenError::NotHere)?;
    if u64::from(dimensions.0) * u64::from(dimensions.1) > MAX_PIXELS {
        return Err(OpenError::NotHere);
    }
    let image = reader.decode().map_err(|_| OpenError::NotHere)?;
    let thumb = image.thumbnail(LONG_EDGE, LONG_EDGE);
    let mut png = Cursor::new(Vec::new());
    thumb
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|_| OpenError::NotHere)?;
    let png = png.into_inner();
    if cache.0.len() == CACHE_ENTRIES {
        if let Some(old) = cache.1.pop_front() {
            cache.0.remove(&old);
        }
    }
    cache.1.push_back(hash);
    cache.0.insert(hash, png.clone());
    Ok(png)
}

/// A path preview can name only a file already read into the composer's staging
/// allow-set. The opaque handle is not a filesystem path, even after decoding.
pub fn staged(staging: &Staging, uri: &str) -> Result<Vec<u8>, OpenError> {
    let rest = crate::asset_url::strip(uri, attachments::SCHEME).ok_or(OpenError::NotHere)?;
    // convertFileSrc uses a localhost host on Apple/Linux, but the handle is
    // directly under the protocol's .localhost host on Windows/Android.
    let path = rest.strip_prefix("localhost/").unwrap_or(rest);
    let handle = path.split('?').next().ok_or(OpenError::NotHere)?;
    if handle.is_empty() || !handle.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(OpenError::NotHere);
    }
    staging.thumbnail(handle)
}

/// Authorization precedes the thumbnail cache; another member's image is never
/// previewed, even if its bytes happen to be cached. A cache miss never downloads.
pub fn blob(source: &Source, blob_id: &str) -> Result<Vec<u8>, OpenError> {
    let rows = read(source,
        "SELECT a.mime FROM attachments a JOIN messages m ON m.id = a.message_id AND m.session_id = a.session_id WHERE json_extract(a.blob_ref, '$.blobId') = ?1 AND m.author_actor = ?2 AND m.role = 'person' LIMIT 1",
        &[Value::from(blob_id), Value::from(source.actor.as_str())]);
    let row = rows.first().ok_or(OpenError::NotHere)?;
    if !supported(text(row, "mime")) {
        return Err(OpenError::NotHere);
    }
    let bytes = source
        .replica
        .cached_blob(blob_id)
        .map_err(|_| OpenError::NotHere)?;
    render(&bytes)
}
