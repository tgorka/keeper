//! Device-only attachment thumbnails (109.15, Q109-14a). No fetch path.
use std::collections::{HashMap, VecDeque};
use std::io::{Cursor, Read};
use std::sync::{LazyLock, Mutex};

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};

use super::attachments::{self, OpenError, Staging, ATTACHMENT_MAX_BYTES};
use super::sessions::{read, text, Source};
use serde_json::Value;

pub const LONG_EDGE: u32 = 256;
const MAX_PIXELS: u64 = 40_000_000;
const CACHE_ENTRIES: usize = 64;
const MAX_METADATA_BYTES: u64 = 8 * 1024 * 1024;

fn bounded_reader(bytes: &[u8]) -> Result<ImageReader<Cursor<&[u8]>>, OpenError> {
    // png 0.18 silently ignores an ICC decompression-limit error. Refuse oversized
    // profiles explicitly, streaming to a sink rather than allocating the profile.
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let mut chunks = &bytes[8..];
        while chunks.len() >= 12 {
            let length = u32::from_be_bytes(chunks[..4].try_into().map_err(|_| OpenError::NotHere)?)
                as usize;
            let end = length.checked_add(12).ok_or(OpenError::NotHere)?;
            let chunk = chunks.get(..end).ok_or(OpenError::NotHere)?;
            if &chunk[4..8] == b"iCCP" {
                let data = &chunk[8..8 + length];
                let name_end = data
                    .iter()
                    .position(|b| *b == 0)
                    .ok_or(OpenError::NotHere)?;
                let compressed = data.get(name_end + 2..).ok_or(OpenError::NotHere)?;
                let mut profile =
                    flate2::read::ZlibDecoder::new(compressed).take(MAX_METADATA_BYTES + 1);
                let size = std::io::copy(&mut profile, &mut std::io::sink())
                    .map_err(|_| OpenError::NotHere)?;
                if size > MAX_METADATA_BYTES {
                    return Err(OpenError::NotHere);
                }
            }
            chunks = &chunks[end..];
        }
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| OpenError::NotHere)?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(MAX_PIXELS * 4);
    reader.limits(limits);
    Ok(reader)
}

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
    // One bounded decoder handles dimensions, metadata and pixels. Never parse
    // dimensions through a second reader with the library's default limits.
    let mut decoder = bounded_reader(bytes)?
        .into_decoder()
        .map_err(|_| OpenError::NotHere)?;
    let dimensions = decoder.dimensions();
    if u64::from(dimensions.0) * u64::from(dimensions.1) > MAX_PIXELS {
        return Err(OpenError::NotHere);
    }
    // from_decoder does not reserve the output buffer against ImageReader limits.
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_PIXELS * 4);
    limits
        .reserve(decoder.total_bytes())
        .map_err(|_| OpenError::NotHere)?;
    decoder.set_limits(limits).map_err(|_| OpenError::NotHere)?;
    let orientation = decoder.orientation().map_err(|_| OpenError::NotHere)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(|_| OpenError::NotHere)?;
    image.apply_orientation(orientation);
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

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageEncoder, Rgb, RgbImage};

    fn oriented_jpeg(orientation: u16) -> Vec<u8> {
        let image = RgbImage::from_fn(640, 320, |x, _| {
            if x < 320 {
                Rgb([240, 0, 0])
            } else {
                Rgb([0, 0, 240])
            }
        });
        // Little-endian TIFF with one SHORT Orientation entry in IFD0.
        let mut exif = b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0".to_vec();
        exif.extend_from_slice(&orientation.to_le_bytes());
        exif.extend_from_slice(&[0; 6]);
        let mut jpeg = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg);
        encoder.set_exif_metadata(exif).expect("EXIF supported");
        encoder
            .write_image(image.as_raw(), 640, 320, image::ExtendedColorType::Rgb8)
            .expect("JPEG fixture");
        jpeg
    }

    #[test]
    fn thumbnail_honors_rotate90_exif_before_scaling() {
        let png = render(&oriented_jpeg(6)).expect("oriented thumbnail");
        let image = image::load_from_memory(&png).expect("PNG").to_rgb8();
        assert_eq!(image.dimensions(), (128, 256));
        assert!(
            image.get_pixel(64, 32)[0] > 200,
            "red half is upright at the top"
        );
        assert!(
            image.get_pixel(64, 224)[2] > 200,
            "blue half is upright at the bottom"
        );
    }

    #[test]
    fn thumbnail_honors_mirrored_exif() {
        let png = render(&oriented_jpeg(2)).expect("mirrored thumbnail");
        let image = image::load_from_memory(&png).expect("PNG").to_rgb8();
        assert_eq!(image.dimensions(), (256, 128));
        assert!(
            image.get_pixel(32, 64)[2] > 200,
            "blue half is mirrored to the left"
        );
        assert!(
            image.get_pixel(224, 64)[0] > 200,
            "red half is mirrored to the right"
        );
    }

    #[test]
    fn thumbnail_refuses_compressed_oversized_icc_metadata() {
        let png = include_bytes!("../../../../../dev/fixtures/agent-photo-oversized-icc.png");
        assert_eq!(render(png), Err(OpenError::NotHere));
    }

    #[test]
    fn thumbnail_dimension_read_uses_configured_limits() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(16_385, 1));
        let mut png = Cursor::new(Vec::new());
        image
            .write_to(&mut png, ImageFormat::Png)
            .expect("wide PNG");
        assert_eq!(render(png.get_ref()), Err(OpenError::NotHere));
    }
}
