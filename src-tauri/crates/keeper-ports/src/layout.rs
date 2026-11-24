//! Object-store key layout and user-metadata encoding for files (story 115.3).
//!
//! Shared between clients and servers via `syncular/object-layout.vectors.json`
//! and `syncular/README.md` § *Objects in parts*. The file key space
//! (`files/<db>/`) never overlaps the conversation blob key space
//! (`conv/<db>/blob/<partition>/sha256/<hex>`).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use serde_json::Value;

/// S3 user-metadata header carrying the encoded [`ObjectMeta`].
pub const METADATA_HEADER: &str = "x-amz-meta-keeper-file";

/// Prefix of a database's file key space (`files/<db>/`).
pub fn file_prefix(database: &str) -> String {
    format!("files/{database}/")
}

/// Content key of `content_id` in `database`'s file key space
/// (`files/<db>/sha256/<hex>`), or `None` unless `content_id` is
/// `sha256:<64 lowercase hex>`.
pub fn content_key(database: &str, content_id: &str) -> Option<String> {
    let hex = content_id.strip_prefix("sha256:")?;
    if hex.len() != 64 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    Some(format!("files/{database}/sha256/{hex}"))
}

/// Prefix of a database's multipart staging keys (`files/<db>/staging/`).
pub fn staging_prefix(database: &str) -> String {
    format!("files/{database}/staging/")
}

/// Staging key of an upload plan (`files/<db>/staging/<plan_id>`).
pub fn staging_key(database: &str, plan_id: &str) -> String {
    format!("files/{database}/staging/{plan_id}")
}

/// User metadata stored beside a file object under [`METADATA_HEADER`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectMeta {
    pub content_id: String,
    pub size: u64,
    pub media_type: Option<String>,
    pub uploaded_at_ms: i64,
}

/// Encode `meta` as unpadded base64url of the compact UTF-8 JSON object with
/// keys in fixed order: `contentId`, `size`, optional `mediaType`,
/// `uploadedAtMs`.
pub fn metadata_value(meta: &ObjectMeta) -> String {
    let content_id = serde_json::to_string(&meta.content_id).unwrap_or_default();
    let mut json = format!("{{\"contentId\":{content_id},\"size\":{}", meta.size);
    if let Some(media_type) = &meta.media_type {
        let escaped = serde_json::to_string(media_type).unwrap_or_default();
        json.push_str(",\"mediaType\":");
        json.push_str(&escaped);
    }
    json.push_str(",\"uploadedAtMs\":");
    json.push_str(&meta.uploaded_at_ms.to_string());
    json.push('}');
    URL_SAFE_NO_PAD.encode(json.as_bytes())
}

/// Decode a [`METADATA_HEADER`] value back into [`ObjectMeta`].
pub fn parse_metadata(value: &str) -> Option<ObjectMeta> {
    let bytes = URL_SAFE_NO_PAD.decode(value).ok()?;
    let Value::Object(map) = serde_json::from_slice(&bytes).ok()? else {
        return None;
    };
    let content_id = map.get("contentId")?.as_str()?.to_owned();
    let size = map.get("size")?.as_u64()?;
    let media_type = match map.get("mediaType") {
        None => None,
        Some(v) => Some(v.as_str()?.to_owned()),
    };
    let uploaded_at_ms = map.get("uploadedAtMs")?.as_i64()?;
    Some(ObjectMeta {
        content_id,
        size,
        media_type,
        uploaded_at_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EMPTY_HEX: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    #[test]
    fn prefixes_and_keys_follow_layout() {
        assert_eq!(file_prefix("general"), "files/general/");
        assert_eq!(staging_prefix("general"), "files/general/staging/");
        assert_eq!(
            staging_key("general", "plan-1"),
            "files/general/staging/plan-1"
        );
        assert_eq!(
            content_key("general", &format!("sha256:{EMPTY_HEX}")),
            Some(format!("files/general/sha256/{EMPTY_HEX}"))
        );
    }

    #[test]
    fn content_key_refuses_malformed_ids() {
        let upper = format!("sha256:{}", EMPTY_HEX.to_ascii_uppercase());
        let short = format!("sha256:{}", &EMPTY_HEX[..63]);
        let long = format!("sha256:{EMPTY_HEX}0");
        let sha1 = "sha1:da39a3ee5e6b4b0d3255bfef95601890afd80709";
        let non_hex = format!("sha256:{}g", &EMPTY_HEX[..63]);

        for bad in [
            upper.as_str(),
            short.as_str(),
            long.as_str(),
            sha1,
            non_hex.as_str(),
            EMPTY_HEX,
        ] {
            assert_eq!(
                content_key("general", bad),
                None,
                "expected refusal for {bad}"
            );
        }
    }

    #[test]
    fn metadata_round_trips_and_rejects_bad_input() {
        let meta = ObjectMeta {
            content_id: format!("sha256:{EMPTY_HEX}"),
            size: 12,
            media_type: Some("text/plain; charset=\"utf-8\"".to_owned()),
            uploaded_at_ms: 1_700_000_000_000,
        };
        let encoded = metadata_value(&meta);
        assert!(!encoded.contains('='));
        assert_eq!(parse_metadata(&encoded), Some(meta));

        assert_eq!(parse_metadata("!!!not-base64!!!"), None);
        assert_eq!(parse_metadata(&URL_SAFE_NO_PAD.encode(b"[]")), None);
        assert_eq!(
            parse_metadata(&URL_SAFE_NO_PAD.encode(
                br#"{"contentId":"sha256:00","size":0,"mediaType":null,"uploadedAtMs":1}"#
            )),
            None
        );
    }
}
