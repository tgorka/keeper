//! Media resolution and the sole decrypted-bytes fetch gate (Story 3.6, FR-13,
//! AD-4, NFR-9).
//!
//! Decrypted media bytes cross to the webview **only** over the `keeper-media://`
//! custom URI-scheme protocol, served from the SDK media cache via the single
//! [`fetch_media`] gate — never as base64/JSON/`Vec<u8>` over IPC. This module
//! owns:
//!
//! - the pure, round-trippable `keeper-media://` URL builder/parser
//!   ([`media_url`] / [`parse_media_url`]) whose URL is composed **only** of data
//!   the frontend already legitimately holds — `account_id`, `room_id`, the opaque
//!   render `item_key` (`unique_id`), and a `thumb|full` variant. No `mxc`,
//!   `EncryptedFile`, or key material ever appears in the URL;
//! - the pure [`select_source`] that chooses the full vs thumbnail
//!   [`MediaSource`] + [`MediaFormat`] for a msgtype and variant;
//! - the single [`fetch_media`] async gate that resolves an item by `item_key` in
//!   a live timeline (mirroring `send::submit_edit`), selects the source, and
//!   calls the SDK `Media::get_media_content` method — the **sole** call site of
//!   that SDK method in the whole crate (a `#[cfg(test)]` scan asserts it).
//!
//! `tracing` logs carry the opaque room id only — never a URL, key, or `mxc`.

use std::sync::Arc;

use matrix_sdk::media::{MediaFormat, MediaRequestParameters, MediaThumbnailSettings};
use matrix_sdk::ruma::events::room::message::MessageType;
use matrix_sdk::ruma::events::room::MediaSource;
use matrix_sdk::Client;
use matrix_sdk_ui::timeline::Timeline;
use matrix_sdk_ui::timeline::{MsgLikeKind, TimelineItemContent, TimelineItemKind};
use url::Url;

use crate::error::MediaError;

/// The fixed host of every `keeper-media://` URL. A constant so the parser can
/// reject a foreign host.
const MEDIA_HOST: &str = "media";

/// The default thumbnail edge (px) requested from the SDK when a msgtype has no
/// dedicated `thumbnail_source` and we scale the full image down. Kept modest so
/// the bubble thumbnail is cheap; the preview overlay always loads the full asset.
const THUMBNAIL_EDGE: u16 = 400;

/// Which representation of a media attachment a `keeper-media://` URL addresses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaVariant {
    /// The thumbnail representation (bubble preview, before full download).
    Thumbnail,
    /// The full content (preview overlay, inline audio/video, file download).
    Full,
}

impl MediaVariant {
    /// The URL path token for this variant (`"thumb"` / `"full"`).
    fn as_str(self) -> &'static str {
        match self {
            MediaVariant::Thumbnail => "thumb",
            MediaVariant::Full => "full",
        }
    }

    /// Parse a URL path token back into a variant, or `None` for an unknown token.
    fn from_str(token: &str) -> Option<Self> {
        match token {
            "thumb" => Some(MediaVariant::Thumbnail),
            "full" => Some(MediaVariant::Full),
            _ => None,
        }
    }
}

/// A resolved `keeper-media://` handle: the opaque coordinates the protocol
/// handler needs to re-resolve a live [`MediaSource`] on demand. Carries no key
/// material — the source is re-derived from the account's open timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaHandle {
    /// The opaque keeper account id that owns the media's room.
    pub account_id: String,
    /// The Matrix room id the media message lives in.
    pub room_id: String,
    /// The opaque render key (`unique_id`) of the media timeline item.
    pub item_key: String,
    /// Which representation (thumbnail / full) is requested.
    pub variant: MediaVariant,
}

/// Decrypted media bytes plus their resolved MIME type, produced by the single
/// [`fetch_media`] gate and streamed out over the `keeper-media://` protocol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaBytes {
    /// The decrypted content bytes (never re-encoded, never base64).
    pub bytes: Vec<u8>,
    /// The `Content-Type` to serve, resolved from the message `info.mimetype`
    /// (falling back to `application/octet-stream`).
    pub mimetype: String,
}

/// Build the opaque `keeper-media://` URL for one media attachment variant
/// (Story 3.6, AD-4). Pure and round-trippable via [`parse_media_url`].
///
/// The URL is `keeper-media://media/<account>/<room>/<item>/<variant>` with each
/// segment percent-encoded by the `url` crate, so odd room ids (`!a/b:host`) and
/// keys survive intact. It carries **only** data the frontend already holds — no
/// `mxc`, `EncryptedFile`, or key material ever appears.
pub fn media_url(account_id: &str, room_id: &str, item_key: &str, variant: MediaVariant) -> String {
    // Start from a bare base with the fixed host, then push percent-encoded path
    // segments so no separator inside a segment is ever mis-parsed.
    let mut url = Url::parse(&format!("keeper-media://{MEDIA_HOST}"))
        .expect("static keeper-media base URL is always valid");
    {
        // `path_segments_mut` fails only for a cannot-be-a-base URL; ours always
        // has a host, so it never does.
        let mut segments = url
            .path_segments_mut()
            .expect("keeper-media URL always has a base");
        segments
            .push(account_id)
            .push(room_id)
            .push(item_key)
            .push(variant.as_str());
    }
    url.to_string()
}

/// Parse a `keeper-media://` URL back into a [`MediaHandle`] (Story 3.6, AD-4),
/// or `None` for any malformed / foreign-scheme / foreign-host / wrong-arity URL.
/// The inverse of [`media_url`] — percent-decoding each segment.
pub fn parse_media_url(raw: &str) -> Option<MediaHandle> {
    let url = Url::parse(raw).ok()?;
    if url.scheme() != "keeper-media" {
        return None;
    }
    if url.host_str() != Some(MEDIA_HOST) {
        return None;
    }
    let segments: Vec<String> = url
        .path_segments()?
        // Percent-decode each segment back to its original bytes/string.
        .map(percent_decode)
        .collect::<Option<Vec<String>>>()?;
    let [account_id, room_id, item_key, variant] = segments.as_slice() else {
        return None;
    };
    let variant = MediaVariant::from_str(variant)?;
    Some(MediaHandle {
        account_id: account_id.clone(),
        room_id: room_id.clone(),
        item_key: item_key.clone(),
        variant,
    })
}

/// Percent-decode a single path segment to a UTF-8 string, or `None` if the bytes
/// are not valid UTF-8.
fn percent_decode(segment: &str) -> Option<String> {
    percent_encoding::percent_decode_str(segment)
        .decode_utf8()
        .ok()
        .map(|cow| cow.into_owned())
}

/// Choose the [`MediaSource`] + [`MediaFormat`] to fetch for a media `msgtype` and
/// the requested [`MediaVariant`] (Story 3.6). Pure — the sole selection logic.
///
/// - `Full` → the message's own `source`, `MediaFormat::File`.
/// - `Thumbnail` → the msgtype's `info.thumbnail_source` when present (served as a
///   `File` — it is already a distinct thumbnail asset); otherwise, for an
///   **unencrypted** full source, the same source with `MediaFormat::Thumbnail`
///   (the homeserver scales it); for an **encrypted** full source with no
///   dedicated thumbnail, fall back to the full source as `File` (the SDK can't
///   server-thumbnail an encrypted asset — the frontend scales it in layout).
///
/// Returns `None` for a non-media msgtype (text/notice/emote/…).
pub fn select_source(
    msgtype: &MessageType,
    variant: MediaVariant,
) -> Option<(MediaSource, MediaFormat)> {
    let (source, thumbnail_source) = media_sources(msgtype)?;
    match variant {
        MediaVariant::Full => Some((source, MediaFormat::File)),
        MediaVariant::Thumbnail => {
            if let Some(thumb) = thumbnail_source {
                // A dedicated thumbnail asset — fetch it whole.
                Some((thumb, MediaFormat::File))
            } else if matches!(source, MediaSource::Plain(_)) {
                // Unencrypted: let the homeserver scale a thumbnail from the full.
                let settings =
                    MediaThumbnailSettings::new(THUMBNAIL_EDGE.into(), THUMBNAIL_EDGE.into());
                Some((source, MediaFormat::Thumbnail(settings)))
            } else {
                // Encrypted with no dedicated thumbnail: the SDK can't server-side
                // thumbnail an encrypted asset, so serve the full and scale in CSS.
                Some((source, MediaFormat::File))
            }
        }
    }
}

/// Extract `(full_source, thumbnail_source)` from a media `msgtype`, or `None` for
/// a non-media msgtype. Pure.
fn media_sources(msgtype: &MessageType) -> Option<(MediaSource, Option<MediaSource>)> {
    match msgtype {
        MessageType::Image(c) => Some((
            c.source.clone(),
            c.info.as_ref().and_then(|i| i.thumbnail_source.clone()),
        )),
        MessageType::Video(c) => Some((
            c.source.clone(),
            c.info.as_ref().and_then(|i| i.thumbnail_source.clone()),
        )),
        MessageType::File(c) => Some((
            c.source.clone(),
            c.info.as_ref().and_then(|i| i.thumbnail_source.clone()),
        )),
        // Audio has no thumbnail source in the spec.
        MessageType::Audio(c) => Some((c.source.clone(), None)),
        _ => None,
    }
}

/// Resolve the `info.mimetype` of a media `msgtype` (for the served
/// `Content-Type`), or `None` when the sender omitted it. Pure.
fn source_mimetype(msgtype: &MessageType) -> Option<String> {
    match msgtype {
        MessageType::Image(c) => c.info.as_ref().and_then(|i| i.mimetype.clone()),
        MessageType::Video(c) => c.info.as_ref().and_then(|i| i.mimetype.clone()),
        MessageType::File(c) => c.info.as_ref().and_then(|i| i.mimetype.clone()),
        MessageType::Audio(c) => c.info.as_ref().and_then(|i| i.mimetype.clone()),
        _ => None,
    }
}

/// Resolve a media [`MediaHandle`] to its decrypted bytes through the SDK media
/// cache — the **sole** `get_media_content` call site in the crate (Story 3.6,
/// AD-4, NFR-9).
///
/// Resolves the `item_key` to a live `MessageType` by scanning `timeline.items()`
/// (mirroring `send::submit_edit`), selects the `(source, format)` via
/// [`select_source`], and calls the SDK `Media::get_media_content` method with a
/// `MediaRequestParameters { source, format }` and `use_cache = true` — which
/// downloads and, for `MediaSource::Encrypted`, decrypts the attachment,
/// reading/writing the SDK's SQLite media store (the "Rust media cache" of AD-4).
/// Returns the decrypted bytes + resolved MIME type.
///
/// Errors: an unresolvable key / a non-media item → [`MediaError::NotFound`]; an
/// SDK fetch/decrypt failure → [`MediaError::Fetch`].
pub async fn fetch_media(
    client: &Client,
    timeline: &Timeline,
    handle: &MediaHandle,
) -> Result<MediaBytes, MediaError> {
    let msgtype = resolve_msgtype(timeline, &handle.item_key)
        .await
        .ok_or(MediaError::NotFound)?;
    let (source, format) = select_source(&msgtype, handle.variant).ok_or(MediaError::NotFound)?;
    let mimetype =
        source_mimetype(&msgtype).unwrap_or_else(|| "application/octet-stream".to_owned());
    let request = MediaRequestParameters { source, format };
    // SOLE-MEDIA-GATE: the one and only `get_media_content` call site (AD-4). It
    // downloads + decrypts (E2EE) via the SDK media cache; bytes never touch IPC.
    let bytes = client
        .media()
        .get_media_content(&request, true)
        .await
        .map_err(|e| MediaError::Fetch(e.to_string()))?;
    Ok(MediaBytes { bytes, mimetype })
}

/// Scan a live [`Timeline`]'s items for the item whose `unique_id` matches
/// `item_key` and return a clone of its media `MessageType`, or `None` when the
/// key is unresolvable or the item is not a media message. Mirrors the
/// `send::submit_edit` resolution.
async fn resolve_msgtype(timeline: &Timeline, item_key: &str) -> Option<MessageType> {
    let items = timeline.items().await;
    let item = items.iter().find(|item| item.unique_id().0 == item_key)?;
    msgtype_of(item)
}

/// Extract the media `MessageType` from a timeline item, or `None` for a virtual /
/// non-message / non-media item. Split out so the resolution is unit-reasoned.
fn msgtype_of(item: &Arc<matrix_sdk_ui::timeline::TimelineItem>) -> Option<MessageType> {
    let TimelineItemKind::Event(ev) = item.kind() else {
        return None;
    };
    let TimelineItemContent::MsgLike(msg_like) = ev.content() else {
        return None;
    };
    let MsgLikeKind::Message(message) = &msg_like.kind else {
        return None;
    };
    Some(message.msgtype().clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_url_round_trips_via_parse() {
        let url = media_url(
            "acct-1",
            "!room:example.org",
            "unique-42",
            MediaVariant::Full,
        );
        let handle = parse_media_url(&url).expect("round-trip parse");
        assert_eq!(handle.account_id, "acct-1");
        assert_eq!(handle.room_id, "!room:example.org");
        assert_eq!(handle.item_key, "unique-42");
        assert_eq!(handle.variant, MediaVariant::Full);
    }

    #[test]
    fn media_url_round_trips_thumbnail_variant() {
        let url = media_url("a", "!r:h", "k", MediaVariant::Thumbnail);
        let handle = parse_media_url(&url).expect("round-trip parse");
        assert_eq!(handle.variant, MediaVariant::Thumbnail);
    }

    #[test]
    fn media_url_round_trips_odd_room_ids_and_keys() {
        // Room ids and keys with URL-significant characters must survive intact.
        let account = "acct/with?odd&chars";
        let room = "!weird/id with spaces:example.org";
        let key = "unique#42/variant?x=y";
        for variant in [MediaVariant::Full, MediaVariant::Thumbnail] {
            let url = media_url(account, room, key, variant);
            let handle = parse_media_url(&url).expect("round-trip parse of odd ids");
            assert_eq!(handle.account_id, account);
            assert_eq!(handle.room_id, room);
            assert_eq!(handle.item_key, key);
            assert_eq!(handle.variant, variant);
        }
    }

    #[test]
    fn media_url_uses_fixed_scheme_and_host() {
        let url = media_url("a", "r", "k", MediaVariant::Full);
        assert!(
            url.starts_with("keeper-media://media/"),
            "unexpected url: {url}"
        );
        // The URL carries no mxc / key material — only the opaque coordinates.
        assert!(!url.contains("mxc"), "url leaked mxc material: {url}");
    }

    #[test]
    fn parse_media_url_rejects_foreign_scheme() {
        assert_eq!(parse_media_url("https://media/a/r/k/full"), None);
        assert_eq!(parse_media_url("keeper://media/a/r/k/full"), None);
    }

    #[test]
    fn parse_media_url_rejects_foreign_host() {
        assert_eq!(parse_media_url("keeper-media://evil/a/r/k/full"), None);
    }

    #[test]
    fn parse_media_url_rejects_wrong_arity() {
        // Too few segments.
        assert_eq!(parse_media_url("keeper-media://media/a/r/full"), None);
        // Too many segments.
        assert_eq!(
            parse_media_url("keeper-media://media/a/r/k/full/extra"),
            None
        );
    }

    #[test]
    fn parse_media_url_rejects_unknown_variant() {
        assert_eq!(parse_media_url("keeper-media://media/a/r/k/bogus"), None);
    }

    #[test]
    fn parse_media_url_rejects_garbage() {
        assert_eq!(parse_media_url("not a url at all"), None);
        assert_eq!(parse_media_url(""), None);
    }

    fn image_msgtype_plain() -> MessageType {
        // Unencrypted image with a plain mxc url and no dedicated thumbnail.
        MessageType::new(
            "m.image",
            "photo.png".to_owned(),
            serde_json::json!({ "url": "mxc://example.org/abc" })
                .as_object()
                .expect("object")
                .clone(),
        )
        .expect("construct image msgtype")
    }

    fn image_msgtype_with_thumbnail() -> MessageType {
        MessageType::new(
            "m.image",
            "photo.png".to_owned(),
            serde_json::json!({
                "url": "mxc://example.org/full",
                "info": { "thumbnail_url": "mxc://example.org/thumb" }
            })
            .as_object()
            .expect("object")
            .clone(),
        )
        .expect("construct image msgtype with thumbnail")
    }

    fn image_msgtype_encrypted() -> MessageType {
        // Encrypted image (a `file` block instead of a plain `url`), no thumbnail.
        MessageType::new(
            "m.image",
            "photo.png".to_owned(),
            serde_json::json!({
                "file": {
                    "url": "mxc://example.org/enc",
                    "key": {
                        "kty": "oct",
                        "key_ops": ["encrypt", "decrypt"],
                        "alg": "A256CTR",
                        "k": "MDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDA",
                        "ext": true
                    },
                    "iv": "MDAwMDAwMDAwMDAwMDAwMA",
                    "hashes": { "sha256": "MDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDA" },
                    "v": "v2"
                }
            })
            .as_object()
            .expect("object")
            .clone(),
        )
        .expect("construct encrypted image msgtype")
    }

    #[test]
    fn select_source_full_uses_file_format() {
        let mt = image_msgtype_plain();
        let (source, format) =
            select_source(&mt, MediaVariant::Full).expect("full source for image");
        assert!(matches!(source, MediaSource::Plain(_)));
        assert!(matches!(format, MediaFormat::File));
    }

    #[test]
    fn select_source_thumbnail_prefers_dedicated_thumbnail_source() {
        let mt = image_msgtype_with_thumbnail();
        let (source, format) = select_source(&mt, MediaVariant::Thumbnail).expect("thumb source");
        // A dedicated thumbnail asset is fetched whole (File format), from the
        // thumbnail's own (plain) source — not the full url.
        assert!(matches!(source, MediaSource::Plain(_)));
        assert!(matches!(format, MediaFormat::File));
    }

    #[test]
    fn select_source_thumbnail_scales_unencrypted_without_dedicated_thumb() {
        let mt = image_msgtype_plain();
        let (source, format) = select_source(&mt, MediaVariant::Thumbnail).expect("thumb source");
        // No dedicated thumbnail + unencrypted → ask the server to scale it.
        assert!(matches!(source, MediaSource::Plain(_)));
        assert!(matches!(format, MediaFormat::Thumbnail(_)));
    }

    #[test]
    fn select_source_thumbnail_encrypted_falls_back_to_full_file() {
        let mt = image_msgtype_encrypted();
        let (source, format) = select_source(&mt, MediaVariant::Thumbnail).expect("thumb source");
        // Encrypted + no dedicated thumbnail → serve the full encrypted asset as a
        // File (the SDK decrypts; the frontend scales in layout).
        assert!(matches!(source, MediaSource::Encrypted(_)));
        assert!(matches!(format, MediaFormat::File));
    }

    #[test]
    fn select_source_returns_none_for_text() {
        use matrix_sdk::ruma::events::room::message::TextMessageEventContent;
        let mt = MessageType::Text(TextMessageEventContent::plain("hi"));
        assert!(select_source(&mt, MediaVariant::Full).is_none());
        assert!(select_source(&mt, MediaVariant::Thumbnail).is_none());
    }

    /// AD-4 single-fetch-gate guard: the SDK `get_media_content` call appears
    /// exactly once in the whole crate, and that one call site is in this module
    /// (the sole media-bytes gate). The production source is isolated from this
    /// `#[cfg(test)]` module (whose text mentions the method) before scanning.
    #[test]
    fn get_media_content_is_the_sole_fetch_gate() {
        let full = include_str!("media.rs");
        let source = full
            .split("#[cfg(test)]\nmod tests")
            .next()
            .expect("production source precedes the test module");
        let call_sites: Vec<usize> = source
            .match_indices(".get_media_content(")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(
            call_sites.len(),
            1,
            "expected exactly one `.get_media_content(` call site (the sole media gate); found {}",
            call_sites.len()
        );
        let gate_start = source
            .find("pub async fn fetch_media")
            .expect("fetch_media fn must exist");
        let gate_end = source
            .find("async fn resolve_msgtype")
            .expect("resolve_msgtype fn must follow fetch_media");
        let call = call_sites[0];
        assert!(
            call > gate_start && call < gate_end,
            "the sole `.get_media_content(` call must be inside `fetch_media`"
        );
    }
}
