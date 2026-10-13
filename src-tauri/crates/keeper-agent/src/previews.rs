//! The screenshots an approval was decided on (S-18, AD-408, AD-409): each
//! kept under the sessions zone's `.keeper/previews/<sha256>.<ext>` — never
//! in a session folder, never committed (`.keeper/` never syncs) — named by
//! its SHA-256, which the record keeps, and sent to a person only as the
//! request's encrypted attachment.

use std::io::Write;
use std::path::{Path, PathBuf};

use keeper_core::agents::approval::sha256_hex;
use keeper_core::agents::screen::preview_path;

/// Where the preview `sha256` lives in `sessions_zone`; `None` unless
/// `sha256` is 64 lowercase hex digits and `ext` one keeper writes
/// ([`keeper_core::agents::screen::PREVIEW_EXTS`]): a record edited in the
/// synced zone names no other file (R316).
pub fn path(sessions_zone: &Path, sha256: &str, ext: &str) -> Option<PathBuf> {
    preview_path(sha256, ext).map(|rel| sessions_zone.join(rel))
}

fn named(sessions_zone: &Path, sha256: &str, ext: &str) -> std::io::Result<PathBuf> {
    path(sessions_zone, sha256, ext).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "a preview is named by its SHA-256 and a png extension",
        )
    })
}

/// Keep `bytes` as a preview: written whole beside its place and renamed
/// into it, once — the same bytes again find it there. Its SHA-256.
pub fn write(sessions_zone: &Path, bytes: &[u8], ext: &str) -> std::io::Result<String> {
    let sha256 = sha256_hex(bytes);
    let at = named(sessions_zone, &sha256, ext)?;
    if at.is_file() {
        return Ok(sha256);
    }
    let dir = at.parent().unwrap_or(sessions_zone);
    std::fs::create_dir_all(dir)?;
    let temp = dir.join(format!(".{sha256}.{}.part", std::process::id()));
    let written = std::fs::File::create(&temp).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    match written.and_then(|()| std::fs::rename(&temp, &at)) {
        Ok(()) => Ok(sha256),
        Err(error) => {
            let _ = std::fs::remove_file(&temp);
            Err(error)
        }
    }
}

/// The preview `sha256`, when this host has it and its bytes are still
/// those.
pub fn read(sessions_zone: &Path, sha256: &str, ext: &str) -> std::io::Result<Vec<u8>> {
    let bytes = std::fs::read(named(sessions_zone, sha256, ext)?)?;
    if sha256_hex(&bytes) != sha256 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the preview is not the one its name says",
        ));
    }
    Ok(bytes)
}

/// A screenshot as grey bytes, one a pixel, row-major.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grey {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// `image`, decoded and made grey; `Err` is why it does not decode.
pub fn grey(image: &[u8]) -> Result<Grey, String> {
    let decoded = image::load_from_memory(image)
        .map_err(|_| "the screenshot is not an image keeper reads".to_owned())?
        .to_luma8();
    let (width, height) = decoded.dimensions();
    Ok(Grey {
        pixels: decoded.into_raw(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_is_kept_once_by_its_hash_under_the_zones_keeper() {
        let zone = tempfile::tempdir().expect("zone");
        let sha = write(zone.path(), b"frame", "png").expect("kept");
        assert_eq!(sha, sha256_hex(b"frame"));
        let at = zone
            .path()
            .join(".keeper/previews")
            .join(format!("{sha}.png"));
        assert_eq!(std::fs::read(&at).expect("there"), b"frame");
        assert_eq!(write(zone.path(), b"frame", "png").expect("again"), sha);
        assert_eq!(read(zone.path(), &sha, "png").expect("read"), b"frame");
        std::fs::write(&at, b"other").expect("tampered");
        assert!(read(zone.path(), &sha, "png").is_err());
    }

    /// R316: a record's preview names nothing but a kept preview — a file
    /// elsewhere whose hash its writer knows is never read for upload, even
    /// where the path it spells resolves to it.
    #[test]
    fn a_preview_names_nothing_outside_the_previews() {
        let zone = tempfile::tempdir().expect("zone");
        std::fs::write(zone.path().join("secret.txt"), b"secret").expect("a file");
        let sha = sha256_hex(b"secret");
        let upper = sha.to_uppercase();
        std::fs::create_dir_all(zone.path().join(format!(".keeper/previews/{sha}.png")))
            .expect("a folder the path walks through");
        for (sha256, ext) in [
            (sha.as_str(), "png/../../../secret.txt"),
            (upper.as_str(), "png"),
            (sha.as_str(), "txt"),
        ] {
            assert!(path(zone.path(), sha256, ext).is_none(), "{sha256} {ext}");
            assert!(read(zone.path(), sha256, ext).is_err(), "{sha256} {ext}");
        }
        assert!(write(zone.path(), b"frame", "txt").is_err());
    }
}
