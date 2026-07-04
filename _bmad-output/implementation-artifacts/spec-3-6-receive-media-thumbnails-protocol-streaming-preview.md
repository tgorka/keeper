---
title: 'Receive Media — Thumbnails, Protocol Streaming, Preview'
type: 'feature'
created: '2026-07-04'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: 'f4c04338cc73dfa76e1efc3868fc03efc140c5a6'
final_revision: '97da8e3e99b39ad0b25c30ebdb7b9685dac7ed3f'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** keeper's timeline (Stories 1.5/3.4/3.5) renders only text, replies, edits, and reactions. Every incoming media message (image, video, audio, file) — including in E2EE rooms — currently maps to `TimelineItemVm::Other` and renders as **nothing**. Users see blanks where photos, clips, voice clips, and files should be (FR-13 receive leg).

**Approach:** Map the media `MessageType`s (`Image`/`Video`/`Audio`/`File`) into an extended `Message` VM carrying only opaque `keeper-media://` URLs + display metadata (no bytes, no keys). Serve decrypted bytes exclusively through a new **Range-capable `keeper-media://` custom URI-scheme protocol** backed by the matrix-sdk media cache (AD-4/NFR-9) — never base64/JSON over IPC. Render thumbnails in the bubble (loading + retry states) and a Quick-Look-style preview overlay with inline video/audio playback.

## Boundaries & Constraints

**Always:**
- Decrypted media bytes cross to the webview **only** over `keeper-media://`, served from the SDK media cache via `client.media().get_media_content(&MediaRequestParameters { source, format }, true)` (which decrypts E2EE media). Never as base64/JSON/`Vec<u8>` over IPC.
- The timeline VM carries **only** opaque `keeper-media://` URL strings + display metadata (kind, filename, mimetype, size, dimensions, caption). `MediaSource`, `EncryptedFile`, `mxc://` URIs, decryption keys, and event IDs stay inside `keeper-core` — never on the VM.
- Model media as an **optional field on the existing `Message` variant** (`media: Option<MediaVm>`), reusing the existing sender/timestamp/send-state/reply/reactions/edit/grouping plumbing — do **not** add a parallel timeline variant.
- Register the scheme via `tauri::Builder::register_asynchronous_uri_scheme_protocol("keeper-media", …)`; the handler runs the async SDK fetch off-thread and **honors HTTP `Range`** (206 Partial Content) for video/audio seeking.
- Thumbnail renders before full download when a thumbnail source is available; full content loads on preview-open.
- Rust rules hold: no `unsafe`, no `.unwrap()`/bare `.expect()` in production paths, `tracing` logging, clippy `-D warnings` clean.
- No new crate dependency unless it passes the cargo-deny license firewall; prefer SDK/`http` primitives already present.

**Block If:**
- Serving Range-capable decrypted media cannot be achieved without either (a) routing bytes through IPC (violates AD-4) or (b) adding an AGPL/GPL-licensed dependency — HALT (`blocked`, "cannot satisfy AD-4 media transport within license firewall").
- The account/room/timeline lookup needed to resolve a media handle in the protocol handler cannot reach `keeper-core` state (e.g. no app-state accessor from the scheme handler) — HALT (`blocked`, "protocol handler cannot reach core state").

**Never:**
- No base64/JSON media bytes over IPC; no `mxc`/`EncryptedFile`/key material/event IDs crossing into JavaScript.
- No `matrix-js-sdk` or any Matrix/media/crypto JS library; no decryption in TS.
- No **sending/uploading** media (Story 3.7); no voice-note **recording** (post-MVP); no message-content logic in TS.
- No unbounded in-memory media registry and no persisting decrypted plaintext outside the SDK cache.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Incoming image (unencrypted) | `MessageType::Image` with plain `mxc` | Bubble shows thumbnail via `keeper-media://…/thumb` before full download; click/Enter opens preview overlay with full image via `…/full` | Fetch fails → bubble shows retry; retry re-requests |
| Incoming image (E2EE) | `MessageType::Image`, `MediaSource::Encrypted` | SDK decrypts in the protocol handler; bytes only over `keeper-media://`; `thumbnail_source` used if present, else full image scaled | Decrypt/fetch error → retry affordance, no crash/blank |
| Incoming video | `MessageType::Video` | Poster/thumbnail in bubble; preview overlay plays via `<video controls>` over the Range protocol (seek works, 206) | Load error → retry |
| Incoming audio | `MessageType::Audio` | Inline `<audio controls>` in the bubble plays via protocol URL | Load error → retry |
| Incoming file | `MessageType::File` | Bubble shows file icon + filename + human size; protocol URL available (no auto-download of bytes over IPC) | Unresolvable → retry |
| Range request | `<video>` seek issues `Range: bytes=…` | Handler returns 206 with the requested slice + `Content-Range`/`Accept-Ranges` | Malformed Range → 200 full body or 416 |
| Handle unresolvable | `keeper-media://` key not in any open timeline for the account | Handler returns 404 | `onError` → retry (succeeds once resolvable); no crash |
| Non-media message | text/notice/emote | `media: None`; renders text exactly as before (no regression) | n/a |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/vm.rs` -- Add `MediaKindVm` (enum: `Image`/`Video`/`Audio`/`File`, camelCase) and `MediaVm { kind, url: String, thumbnail_url: Option<String>, filename: String, mimetype: Option<String>, size: Option<u32> (`#[ts(type="number")]`), width: Option<u32>, height: Option<u32>, caption: Option<String> }` (serde camelCase + `#[ts(export)]`). Extend `TimelineItemVm::Message` with `media: Option<MediaVm>`. Update `sample_message`/round-trip tests; assert `MediaVm` carries **no** `mxc`/key/event-id material (only opaque `keeper-media://` url + display metadata).
- `src-tauri/crates/keeper-core/src/media.rs` -- **NEW** module. Pure `media_url(account_id, room_id, item_key, variant) -> String` builder and its inverse `parse_media_url(&str) -> Option<MediaHandle { account_id, room_id, item_key, variant }>` (fixed host, percent-encoded segments; round-trippable). `MediaVariant { Thumbnail, Full }`. Pure `select_source(msgtype, variant) -> Option<(MediaSource, MediaFormat)>` choosing full vs `thumbnail_source` (fallback to `MediaFormat::Thumbnail` for unencrypted, else full). `MediaBytes { bytes: Vec<u8>, mimetype: String }`. `async fetch_media(client, timeline_items, handle) -> Result<MediaBytes, CoreError>` = resolve item by `item_key` in the timeline items (mirror `send::submit_edit` resolution), `select_source`, `client.media().get_media_content(&MediaRequestParameters{source, format}, true)`; the **sole** `get_media_content` call site.
- `src-tauri/crates/keeper-core/src/timeline.rs` -- In `item_to_vm`, add a pure `media_vm(msgtype, account_id, room_id, item_key) -> Option<MediaVm>` mapping `MessageType::{Image,Video,Audio,File}` → `MediaVm` (kind, `url`/`thumbnail_url` via `media::media_url`, filename via `.filename()`/`body`, mimetype/size/dimensions from `info`). Text msgtypes → `None`. Populate `Message.media` (None for text). `body` remains the caption/text.
- `src-tauri/crates/keeper-core/src/account.rs` -- Add `async fetch_media(account_id, room_id, item_key, variant) -> Result<MediaBytes, CoreError>`: `client_for` + resolve the room's open timeline items, delegate to `media::fetch_media`. Log room id only (no key/url material).
- `src-tauri/crates/keeper/src/lib.rs` -- Register the `keeper-media` async URI-scheme protocol on the `Builder`: parse the request URI via `media::parse_media_url`, read the `Range` header, `tauri::async_runtime::spawn` a task that calls `state.accounts.fetch_media(...)`, then `responder.respond(...)` with `Content-Type`, `Accept-Ranges: bytes`, and a 200 (full) or 206 (sliced, with `Content-Range`) body. 404 on parse/resolve failure. No `.unwrap()`.
- `src-tauri/crates/keeper/src/ipc.rs` -- No new command (bytes never cross IPC). If a `CoreError` variant is missing for media-resolve failures, add one + map in `to_ipc_error` only as needed by `fetch_media`'s signature; keep the surface minimal.
- `src/lib/ipc/client.ts` -- Re-export regenerated `MediaVm`, `MediaKindVm`, updated `TimelineItemVm`. No new invoke wrapper.
- `src/components/chat/media-attachment.tsx` -- **NEW** presentational component: given a `MediaVm`, render image thumbnail / video poster / inline `<audio controls>` / file chip (icon+name+size). Loading skeleton until `onLoad`; `onError` → retry affordance that reloads the src with a cache-busting suffix. Image/video click or Enter → `onOpenPreview(key)`. Uses width/height to reserve layout.
- `src/components/chat/media-preview-overlay.tsx` -- **NEW** shadcn `Dialog`-based Quick-Look overlay: full-res `<img>` / `<video controls autoPlay>` / `<audio controls>` from the `…/full` URL; `Esc`/backdrop closes and returns focus to the timeline.
- `src/components/chat/message-bubble.tsx` -- When `item.media` present, render `<MediaAttachment>` (with optional caption from `body`) instead of / above the text `<p>`; thread `onOpenPreview`. New prop `onOpenPreview?`.
- `src/components/layout/conversation-pane.tsx` -- Hold preview overlay state (`previewKey`); `onOpenPreview(key)` opens it; render `<MediaPreviewOverlay>` with the resolved media VM; thread `onOpenPreview` to bubbles.
- `src/lib/stores/timeline.ts` (test fixture) & `src/**` colocated tests -- update fixtures with `media: null`; tests below.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/vm.rs` -- Add `MediaKindVm` + `MediaVm` (camelCase + `#[ts(export)]`); extend `Message` with `media: Option<MediaVm>`. -- Typed media metadata crossing IPC (opaque url + display fields only).
- [x] `src-tauri/crates/keeper-core/src/media.rs` -- New module: pure `media_url`/`parse_media_url` round-trip, `MediaVariant`, `select_source`, `MediaBytes`, `fetch_media` (sole `get_media_content` gate). -- Confine all media source/keys/fetch/decrypt to Rust.
- [x] `src-tauri/crates/keeper-core/src/timeline.rs` -- `media_vm` mapping for `Image/Video/Audio/File`; populate `Message.media`. -- Media messages stop rendering as `Other`.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- `fetch_media(account_id, room_id, item_key, variant)` resolving the open timeline + delegating to `media::fetch_media`. -- Per-account media access path.
- [x] `src-tauri/crates/keeper/src/lib.rs` -- Register the `keeper-media` async URI-scheme protocol (Range-aware, async off-thread, 200/206/404, content-type). -- The exclusive decrypted-bytes transport (AD-4).
- [x] `src-tauri/crates/keeper-core/src/{account.rs,error.rs}` (as needed) -- Any new `CoreError` arm for media-resolve failure. -- Honest error typing.
- [x] `src/lib/ipc/client.ts` -- Re-export `MediaVm`/`MediaKindVm` + updated `TimelineItemVm`. -- Typed frontend bindings.
- [x] `src/components/chat/media-attachment.tsx` -- New media renderer (thumbnail/poster/inline audio/file chip; loading + retry; open-preview). -- Bubble media rendering (AC1).
- [x] `src/components/chat/media-preview-overlay.tsx` -- New Dialog overlay (image/video/audio; Esc closes, focus returns). -- Quick-Look preview (AC2, AC3).
- [x] `src/components/chat/message-bubble.tsx` -- Render `<MediaAttachment>` when `item.media`; thread `onOpenPreview`. -- Wire media into the timeline bubble.
- [x] `src/components/layout/conversation-pane.tsx` -- Preview overlay state + `onOpenPreview` wiring; render overlay. -- End-to-end preview flow.
- [x] `src-tauri/crates/keeper-core/src/{vm.rs,media.rs,timeline.rs}` (tests) -- vm serde round-trip (`MediaVm`/`MediaKindVm`; Message with media; no key/mxc/event-id material); `media_url`↔`parse_media_url` round-trip incl. odd room ids; `select_source` thumb-vs-full + encrypted-fallback; `media_vm` per msgtype (kind/url/thumbnail_url/filename/mimetype/size/dims) + text→`None`; guard test: `get_media_content` appears exactly once (in `media.rs`) and VM has no bytes/base64 field. -- Lock the transport + mapping contract.
- [x] `src/**` (tests) -- media-attachment: image `<img>` thumbnail src, file chip name+size, inline `<audio>`, loading skeleton, `onError`→retry reloads src, image click→`onOpenPreview`; media-preview-overlay: opens with media, `<video>`/`<audio>` present, Esc closes; conversation-pane: media click opens overlay. -- Cover the I/O matrix + ACs.

**Acceptance Criteria:**
- Given an incoming media message (including E2EE), when it renders, then a thumbnail appears before full download, decrypted bytes are served exclusively via the Range-capable `keeper-media://` protocol from the SDK media cache (never base64/JSON over IPC), and a retry affordance appears on fetch failure (FR-13, AD-4, NFR-9).
- Given a media bubble, when the user clicks it or presses Enter, then a Quick-Look-style preview overlay opens (Esc closes, focus returns to the timeline) with video/audio playable via the protocol URL (FR-13).
- Given received audio, then it plays back inline via `<audio controls>` over the protocol.
- Given the media path, then all `MediaSource`s, `EncryptedFile`s, `mxc` URIs, keys, and event IDs stay in `keeper-core` (only opaque `keeper-media://` URLs + display metadata cross IPC), decrypted bytes flow only over the custom protocol, and `get_media_content` is the single fetch gate.
- Given `bun run check:all`, then Biome, tsc, vitest, rustfmt, clippy (`-D warnings`), cargo-nextest, and `cargo deny check` all pass and the ts-rs bindings (`MediaVm`, `MediaKindVm`, updated `TimelineItemVm`) regenerate without drift.

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 2, low 0)
- defer: 3: (high 0, medium 2, low 1)
- reject: 10: (high 0, medium 0, low 10)
- addressed_findings:
  - `[medium]` `[patch]` The Quick-Look preview overlay rendered a bare `<img>`/`<video>`/`<audio>` with no `onError` — a failed `…/full` fetch (the heavier variant, most likely to 404 after the bubble thumbnail already cached) left a silent broken/blank element with no recovery, violating the "no silent blanks" invariant. Added a per-`PreviewBody` error+retry state (cache-busting nonce, mirroring the bubble) plus a test asserting error → retry affordance → re-requested `src` with `retry=1`.
  - `[medium]` `[patch]` A posterless video (encrypted video without a `thumbnail_source`) mounted a `<video src=fullSrc preload="metadata">` in the bubble; because matrix-sdk's `get_media_content` is atomic, that request forced a full download+decrypt of the entire video just to draw a poster (defeating "thumbnail before full download" and OOM-amplifying on scroll). Replaced it with a static, non-fetching placeholder (film icon + immediate play affordance) that still opens the full video in the preview overlay; added a test asserting no `<video>` mounts in the bubble and the placeholder opens the preview.

## Design Notes

**SDK API (matrix-sdk 0.18, verified on disk).** Fetch+decrypt: `client.media().get_media_content(request: &MediaRequestParameters, use_cache: bool) -> Result<Vec<u8>>` — for `MediaSource::Encrypted` and the `e2e-encryption` feature it downloads and **decrypts** the attachment; `use_cache: true` reads/writes the SDK's SQLite media store (this is "the Rust media cache" of AD-4). `MediaRequestParameters { source: MediaSource, format: MediaFormat }`; `MediaFormat::{File, Thumbnail(MediaThumbnailSettings{method,width,height,animated})}` (from `matrix_sdk::media`). `MediaSource::{Plain(OwnedMxcUri), Encrypted(Box<EncryptedFile>)}` (ruma). Message content: `MessageType::{Image,Video,Audio,File}(content)` where each `content` has `.source: MediaSource`, `.filename()`/`.body`, and `.info: Option<Box<…Info>>` (`mimetype`, `size`, image/video dimensions, and `thumbnail_source: Option<MediaSource>`).

**Range without streaming progress.** `get_media_content` is atomic (whole `Vec<u8>`, no byte-progress callback in 0.18). So the protocol handler fetches the full (SDK-cached, decrypted) bytes and **slices per `Range`** in memory — the protocol is Range-capable *from the cache*, which is exactly what the AC requires ("served … from the Rust media cache"). Consequently the bubble's "download progress" is an **indeterminate loading state** (skeleton/spinner until the media element's `onLoad`), not a byte percentage — byte-granular progress isn't exposed by the SDK. Retry is pure-frontend: re-set the element `src` with a cache-busting suffix so the webview re-requests (handler re-fetches on cache miss).

**Opaque handle, zero key leakage.** The `keeper-media://` URL is composed **only** of data the frontend already legitimately holds — `account_id`, `room_id`, the opaque render `key` (item `unique_id`, already the VM `key`), and a `thumb|full` variant — percent-encoded into a fixed-host URL by the pure `media_url` builder. No `mxc`/`EncryptedFile`/keys ever appear in it. The handler resolves the handle back to a live `MediaSource` on demand by scanning the account's **open** timeline items for `unique_id` (mirroring how `send::submit_edit`/`toggle_reaction` resolve keys) — so there's **no persistent media registry** to leak or grow unbounded; an unresolvable handle → 404 → retry.

**Async scheme handler (Tauri 2.11).** `Builder::register_asynchronous_uri_scheme_protocol("keeper-media", |ctx, req, responder| { … })` — the handler gets `UriSchemeContext` (→ `app_handle()` → `state::<AppState>()`), the `http::Request<Vec<u8>>` (URI + `Range` header), and a `UriSchemeResponder` that is `Send` and moved into `tauri::async_runtime::spawn` so the async SDK fetch never blocks the webview thread. Respond with `http::Response` (status 200/206/404, `Content-Type`, `Accept-Ranges: bytes`, `Content-Range` on 206). CSP is `null` in `tauri.conf.json`, so no CSP/img-src change is needed now; if CSP is later enabled it must allow `keeper-media:` in `img-src`/`media-src` (forward note only).

**VM modeling.** Media messages are still `MsgLikeKind::Message` — same envelope as text, just a media `msgtype`. Modeling media as `Message.media: Option<MediaVm>` (not a new variant) reuses reply/reactions/edit/send-state/grouping already wired through the bubble, and text messages simply carry `media: None`. `MediaVm` intentionally excludes reactor/senders/event-ids — only render-facing fields.

## Verification

**Commands:**
- `bun run check` -- Biome + tsc + vitest green (incl. new media-attachment/preview-overlay/bubble/pane tests).
- `bun run check:rust` -- rustfmt clean + clippy `-D warnings` clean.
- `bun run test:rust` -- cargo-nextest green; ts-rs regenerates `MediaVm`, `MediaKindVm`, updated `TimelineItemVm` with no git drift.
- `cargo deny check` (from `src-tauri/`) -- green; no new crate deps (or only firewall-clean ones).

**Manual checks (real second session, test credentials in 1Password):**
- From Element, send an image, a short video, an audio clip, and a file into an encrypted room; confirm keeper shows the image/video thumbnail before full load, the file chip with name/size, and inline audio playback.
- Click the image → preview overlay opens full-res; Esc returns focus to the timeline. Open the video in the overlay and seek — playback resumes at the sought position (Range 206 served).
- Kill the network mid-load and confirm the bubble shows a retry affordance; restore network, retry, and confirm it loads. Confirm (dev tools / network) that no media bytes traverse the IPC channel — only `keeper-media://` requests.

## Auto Run Result

Status: done

**Summary:** Implemented the receive leg of media (FR-13) — image/video/audio/file, including in E2EE rooms — with decrypted bytes served **exclusively** over a new Range-capable `keeper-media://` custom URI-scheme protocol backed by the matrix-sdk SQLite media cache (AD-4/NFR-9), never as base64/JSON over IPC. Media messages (still `MsgLikeKind::Message`) now map to `TimelineItemVm::Message` with an added `media: Option<Box<MediaVm>>` carrying only opaque `keeper-media://` URLs + display metadata (`kind`, `filename`, `mimetype`, `size`, `width`, `height`, `caption`) — no `mxc`/`EncryptedFile`/keys/event-IDs ever cross IPC. A new `keeper-core::media` module owns a pure, round-trippable `media_url`/`parse_media_url`, `select_source` (full vs `thumbnail_source`, encrypted-fallback), and `fetch_media` — the **sole** `get_media_content` call site, resolving the opaque render key to the live SDK item by scanning the open timeline (mirroring `send::submit_edit`). The Tauri `keeper-media` async scheme handler (`keeper::media_protocol`) parses the URL, honors HTTP `Range` (200/206/416), spawns the async decrypt off-thread, and 404s unresolvable handles. The frontend renders image thumbnails / video posters / inline `<audio controls>` / file chips with loading + retry states, and a shadcn `Dialog` Quick-Look overlay (Esc closes, focus returns; video/audio play via the Range protocol).

**Files changed:**
- `src-tauri/crates/keeper-core/src/vm.rs` — `MediaKindVm` + `MediaVm`; `Message.media: Option<Box<MediaVm>>`; serde round-trip + no-key-material tests.
- `src-tauri/crates/keeper-core/src/media.rs` (NEW) — `media_url`/`parse_media_url`, `MediaVariant`, `select_source`, `MediaBytes`, `fetch_media` (sole `get_media_content` gate); guard + round-trip + fallback tests.
- `src-tauri/crates/keeper-core/src/timeline.rs` — `media_vm` mapping per msgtype; `account_id` threaded through `open_timeline`/`OpenTimeline`/`forward_timeline`; caption-as-body.
- `src-tauri/crates/keeper-core/src/{account.rs,error.rs,lib.rs,Cargo.toml}` — `fetch_media` account method; `MediaError`; `percent-encoding` dep (already transitive).
- `src-tauri/crates/keeper/src/media_protocol.rs` (NEW) + `lib.rs` — Range-aware async `keeper-media` scheme handler + registration; `parse_range` + response tests.
- `src-tauri/crates/keeper/src/ipc.rs` — `CoreError::Media` mapped in `to_ipc_error` (exhaustiveness; media errors never actually flow through IPC).
- `src/lib/ipc/client.ts` (+ `gen/MediaVm.ts`, `gen/MediaKindVm.ts`, `gen/TimelineItemVm.ts`) — re-exports + regenerated bindings.
- `src/components/chat/media-attachment.tsx` (+ `.test.tsx`) — NEW thumbnail/poster/inline-audio/file-chip renderer with loading + retry.
- `src/components/chat/media-preview-overlay.tsx` (+ `.test.tsx`) — NEW Quick-Look Dialog overlay with error/retry.
- `src/components/chat/message-bubble.tsx` (+ test), `src/components/layout/conversation-pane.tsx` (+ test), `src/lib/stores/timeline.test.ts` — wire media + `onOpenPreview` + preview state; fixtures gain `media: null`.

**Review findings breakdown:** intent_gap 0, bad_spec 0, patch 2 (both medium, applied), defer 3 (medium 2, low 1), reject 10 (all low).
- **Patches applied:** (1) preview overlay had no `onError` → a failed `…/full` fetch left a silent blank; added error+retry (cache-bust nonce) mirroring the bubble. (2) posterless video mounted a `<video preload=metadata>` that forced a full atomic download+decrypt just for a poster; replaced with a static non-fetching placeholder (film icon + play affordance) that loads the full video only in the overlay. Both covered by new tests.
- **Deferred (3):** unsupported-codec audio (e.g. Ogg/Opus in WKWebView) stalls with no fallback (platform limitation, needs stall-detect + download fallback UX); no media size ceiling (atomic SDK fetch loads whole file into RAM — needs a product limit that won't break the 25 MB video bar); `keeper-media://` handle is forgeable by a compromised webview (defense-in-depth — sign/HMAC the handle). All logged in `deferred-work.md`.
- **Rejected (10, all low):** formatSize MiB-vs-MB labeling (within common convention); u32 size/dimension saturation for >4 GiB (rare, saturating is honest); crafted `/thumb` URL on audio/file (→404, unreachable from the VM); whitespace-only caption blank line; 0-byte-attachment degenerate case; malformed-Range→200 (spec-permitted); stale `previewKey` after item flips to UTD (overlay just closes); file chip has no retry (metadata-only by design, no fetch); manual-only unbounded retry (no auto-loop, no runaway); play-badge over a still-loading poster (cosmetic).

**Verification performed (independently re-run):**
- `bun run check` — Biome clean, tsc clean, **373** vitest tests pass (41 files; +2 from the patch tests).
- `bun run check:rust` — `cargo fmt --check` clean, clippy `-D warnings` clean.
- `bun run test:rust` — cargo-nextest **256** tests pass; ts-rs bindings regenerated with drift limited to `TimelineItemVm.ts` (updated) + `MediaVm.ts`/`MediaKindVm.ts` (new).
- `cargo deny check licenses bans sources` — ok (no new crate added to the tree; `percent-encoding` was already transitive). The full `cargo deny check` `advisories` failure is pre-existing gtk-rs/GTK3 `unmaintained` RUSTSEC advisories from Tauri's Linux backend, present on the baseline and unrelated to this story.

**Residual risks:** Live E2EE verification against a real second session (Element → encrypted room: image/video/audio/file, seek, mid-load network kill, IPC-has-no-bytes check) was not performed in this environment — see the spec's Manual checks. The three deferred items (audio codec fallback, media size cap, handle signing) remain open hardening/UX work.
