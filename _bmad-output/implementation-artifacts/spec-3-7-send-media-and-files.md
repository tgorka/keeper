---
title: 'Send Media and Files'
type: 'feature'
created: '2026-07-04'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
baseline_revision: 'bb6751b4cbf2c75374e44333bed97abc4531f72e'
final_revision: '1e154901469a2551c6b012d02ddf82d6331941c0'
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-3-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-3-6-receive-media-thumbnails-protocol-streaming-preview.md'
warnings: ['oversized']
---

<intent-contract>

## Intent

**Problem:** keeper can now *receive* media (Story 3.6) but the composer only sends text. Users cannot attach a file, paste an image, or drop a file into a Chat — the send leg of FR-13 is missing, so media is a one-way street.

**Approach:** Add a media **send** leg that reuses the SDK send-queue plumbing already behind text (local echo, send-state stream, retry-via-unwedge, offline queue) via `Timeline::send_attachment(...).use_send_queue()` — the SDK encrypts automatically in E2EE rooms and emits a local-echo timeline item that the 3.6 receive path already renders (`media_vm` + `keeper-media://`). Attachments enter as OS **file paths** (composer button + native drag-drop) or a **raw binary IPC body** (paste) — never base64/JSON media over IPC. The bubble shows an uploading state with a Cancel affordance during the sending state, and the existing "Failed — Retry" caption covers terminal failure.

## Boundaries & Constraints

**Always:**
- Media bytes reach `keeper-core` as either an **OS file path** (attach button + drag-drop → Rust `tokio::fs::read`) or a **raw binary IPC body** (`tauri::ipc::Request`, `InvokeBody::Raw`, metadata in headers) for paste. **Never** base64/JSON/`Vec<u8>`-in-JSON over IPC.
- Sending goes through **one new dispatch gate** `send::submit_attachment` calling `Timeline::send_attachment(...)` exactly once (extend the existing single-gate guard test that already covers `Timeline::send`/`send_reply`/`edit`). Use `.use_send_queue()` so local echo, send states, offline queueing, and retry reuse the text plumbing.
- Reuse the existing send-state stream, `SendStateCaption` ("Failed — Retry" / "Sending…" / "Queued…"), and `retrySend` (SDK `unwedge`) for media echoes unchanged — do **not** synthesize echoes in keeper.
- The local echo renders through the **unchanged** 3.6 receive path (`MediaVm`/`keeper-media://`); no new timeline VM variant and no new VM field.
- Cancel-during-upload = resolve the local echo's SDK send handle and call its **abort** (best-effort, symmetric with `retry`/`unwedge`); if the event already dispatched, cancel is a no-op and the message stays sent.
- E2EE rooms: rely on the SDK to encrypt the attachment; sent media must be decryptable by other Matrix clients (do not hand-roll encryption).
- Rust rules hold: no `unsafe`, no `.unwrap()`/bare `.expect()` in production paths, `tracing` logging (log room id / kind / size only — never bytes, paths of user files at info level, keys, or mxc), clippy `-D warnings` clean. TS: no `any`, `import type`, Biome-clean.
- Any new crate/JS dep must pass the cargo-deny license firewall (permissive only). Prefer `@tauri-apps/plugin-dialog` + `tauri-plugin-dialog` and `mime_guess` (all MIT/Apache).

**Block If:**
- `Timeline::send_attachment` + send-queue local echo + an abort handle for in-flight attachments cannot be reconciled in matrix-sdk-ui 0.18 without either routing media bytes through JSON/base64 IPC or adding an AGPL/GPL dependency — HALT (`blocked`, "cannot satisfy AD-4/license firewall for media send").

**Never:**
- No base64/JSON media bytes over IPC; no `mxc`/`EncryptedFile`/keys/plaintext crossing into JavaScript.
- No `matrix-js-sdk` or any Matrix/media/crypto JS; no encryption/upload logic in TS.
- No voice-note **recording** (post-MVP); no client-generated video transcoding.
- No artificial size cap that would break the 25 MB video bar (unbounded-RAM hardening is a shared 3.6 deferred item — do not regress the bar to "fix" it here).
- No client-side thumbnail generation required for MVP (send with minimal `AttachmentInfo`; receivers fall back to full per 3.6). Byte-granular upload % bar is out of scope (SDK send-queue path does not surface transmitted/total bytes in 0.18) — MVP shows an indeterminate uploading indicator.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Attach via button | User clicks attach, dialog returns a file path | Pending-attachment chip appears in composer; Send dispatches via path IPC; local echo shows in timeline with uploading state | Dialog cancelled → no-op |
| Paste image | Clipboard image pasted into composer | Pending chip appears; Send dispatches pasted bytes via raw binary IPC body (not base64); local echo uploads | Non-image paste → falls through to text paste unchanged |
| Drag-drop file | File(s) dropped on conversation pane (Tauri drag-drop event → path) | Pending chip(s) appear; Send dispatches via path IPC | Drop of unsupported/dir → ignored with a tracing warn |
| Upload in flight | Local echo `sendState: "sending"`, media present | Bubble shows uploading indicator + Cancel affordance | n/a |
| Cancel during upload | User clicks Cancel on an in-flight media echo | `cancel_send` aborts the queued send; echo removed via SDK diff | Already dispatched → abort returns false → no-op, stays sent |
| Terminal upload failure | Send fails terminally (network/server) | Existing "Failed — Retry" caption on the bubble; `retrySend` re-drives (unwedge) | Retry loops through the same gate |
| E2EE room | Room encrypted | SDK encrypts attachment; message decryptable by other clients | Encrypt/upload error → Failed — Retry |
| 25 MB video | Large file attached | Uploads (progress indicator), produces a playable message on the receiving side | Failure → Failed — Retry |
| Caption present | Composer text + single pending attachment | Text rides as the attachment `caption`; bubble shows media + caption (3.6 caption rendering) | Empty caption → no caption field |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/send.rs` -- Add `pub async fn submit_attachment(timeline: &Timeline, bytes: Vec<u8>, filename: &str, mime: Mime, caption: Option<&str>) -> Result<(), SendError>`: build `AttachmentConfig` (optional `caption` via `TextMessageEventContent`), call `timeline.send_attachment(bytes, mime, config).use_send_queue().await` — the **sole** `send_attachment` gate. Add `pub async fn cancel(timeline: &Timeline, item_key: &str) -> Result<(), SendError>`: resolve the item by `unique_id` (mirror `retry`), obtain its send handle, call `.abort()`. New `SendError::Upload(String)` (or reuse `Dispatch`) mapped retriable. Extend the compile-time single-gate guard test to include `send_attachment`.
- `src-tauri/crates/keeper-core/src/account.rs` -- `pub async fn send_attachment_path(account_id, room_id, path: &Path, caption: Option<&str>)`: `open_timeline_for`, `tokio::fs::read(path)`, derive filename + mime (`mime_guess::from_path`, fallback `application/octet-stream`), delegate to `send::submit_attachment`. `pub async fn send_attachment_bytes(account_id, room_id, bytes, filename, mime_str, caption)`: parse mime, delegate. `pub async fn cancel_send(account_id, room_id, item_key)` → `send::cancel`. Log room id + kind + size only.
- `src-tauri/crates/keeper-core/Cargo.toml` -- Add `mime_guess` (MIT). Must pass `cargo deny check`.
- `src-tauri/crates/keeper/src/ipc.rs` -- Commands: `send_attachment_path(state, account_id, room_id, path, caption)`; `send_attachment_bytes(state, request: tauri::ipc::Request<'_>)` reading `request.body()` raw bytes + `account_id`/`room_id`/`filename`/`mime`/`caption` from `request.headers()`; `cancel_send(state, account_id, room_id, item_key)`. Map new `SendError` arm(s) in `to_ipc_error` (retriable). No media bytes returned.
- `src-tauri/crates/keeper/src/lib.rs` -- Register the three new commands in `invoke_handler`; init `tauri_plugin_dialog::init()`.
- `src-tauri/crates/keeper/Cargo.toml` + `capabilities/*.json` -- Add `tauri-plugin-dialog`; grant the dialog `open` capability.
- `package.json` -- Add `@tauri-apps/plugin-dialog`.
- `src/lib/ipc/client.ts` -- Wrappers `sendAttachmentPath(accountId, roomId, path, caption?)`, `sendAttachmentBytes(accountId, roomId, bytes: ArrayBuffer, filename, mime, caption?)` (invoke with raw body + headers), `cancelSend(accountId, roomId, itemKey)`.
- `src/components/chat/composer.tsx` -- Attach button (opens dialog via plugin → paths) + `onPaste` handler (image → bytes) building a **pending-attachment tray** (removable chips above the textarea; filename + size). `send()` dispatches each pending attachment (path or bytes), with trimmed composer text as `caption` when exactly one attachment is pending; clears the tray. Removing a chip = pre-upload cancel.
- `src/components/layout/conversation-pane.tsx` -- Subscribe to Tauri drag-drop (`getCurrentWebview().onDragDropEvent`) while a room is open → push dropped paths into the composer pending tray; thread `onCancelSend` to bubbles.
- `src/components/chat/media-attachment.tsx` + `src/components/chat/message-bubble.tsx` -- When the owning message is an outgoing echo with `sendState === "sending"` and media present, overlay an **uploading indicator + Cancel** button (calls `onCancelSend(key)`); reuse existing `SendStateCaption` for failed/queued/sent.
- `src/components/chat/*.test.tsx`, `src/lib/**` tests + Rust `#[cfg(test)]` -- see test task.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-core/src/send.rs` -- Add `submit_attachment` (sole `Timeline::send_attachment` gate, `.use_send_queue()`) + `cancel` (abort via send handle); extend the single-gate guard test. -- Media dispatch reuses text send-state/retry plumbing.
- [x] `src-tauri/crates/keeper-core/src/account.rs` -- `send_attachment_path` (read file + mime-guess), `send_attachment_bytes`, `cancel_send`. -- Per-account send + cancel path.
- [x] `src-tauri/crates/keeper-core/Cargo.toml` -- Add `mime_guess`; verify `cargo deny check`. -- Mime detection from path/extension.
- [x] `src-tauri/crates/keeper/src/ipc.rs` -- `send_attachment_path`, `send_attachment_bytes` (raw body + headers), `cancel_send` commands; map new `SendError` arm(s). -- Path + raw-bytes ingestion, no media over JSON.
- [x] `src-tauri/crates/keeper/src/lib.rs` + `Cargo.toml` + capabilities -- Register commands; add + init `tauri-plugin-dialog`; grant `dialog:allow-open`. -- File-picker + command wiring.
- [x] `package.json` -- Add `@tauri-apps/plugin-dialog`. -- Frontend dialog access.
- [x] `src/lib/ipc/client.ts` -- `sendAttachmentPath`, `sendAttachmentBytes` (raw-body invoke), `cancelSend`. -- Typed send wrappers.
- [x] `src/components/chat/composer.tsx` -- Attach button + paste handler + pending-attachment tray; Send dispatches attachments + caption. -- Attach/paste ingestion + review UX (AC1).
- [x] `src/components/layout/conversation-pane.tsx` -- Tauri drag-drop listener → pending tray; thread `onCancelSend`. -- Drag-drop ingestion + cancel wiring (AC1).
- [x] `src/components/chat/media-attachment.tsx` + `message-bubble.tsx` -- Uploading indicator + Cancel affordance on in-flight own media echoes. -- Visible upload state + cancel (AC1).
- [x] `src-tauri/**` tests -- `submit_attachment` gate (guard test sees exactly one `send_attachment`); `cancel` resolves + aborts; mime-guess mapping (extensions → image/video/audio/file + octet-stream fallback); `to_ipc_error` maps new arm; raw-body command parses headers. -- Lock the send/cancel contract.
- [x] `src/**` tests -- composer: attach button adds chip, paste image adds chip, remove chip drops it, Send calls `sendAttachmentPath`/`sendAttachmentBytes` with caption; bubble: `sending`+media renders uploading indicator + Cancel → `cancelSend`; failed media still shows "Failed — Retry"; drag-drop pushes a chip. -- Cover the I/O matrix + ACs.

**Acceptance Criteria:**
- Given an open Chat, when the user attaches via the composer button, pastes an image, or drops a file, then a local echo appears with a visible uploading state and a Cancel affordance, and on success a playable/openable message is produced on the receiving side — verified with a 25 MB video (FR-13).
- Given media bytes, then they reach `keeper-core` only as an OS file path or a raw binary IPC body — never base64/JSON — and no `mxc`/`EncryptedFile`/keys/plaintext cross into JavaScript; `Timeline::send_attachment` is the single dispatch gate.
- Given an in-flight upload, when the user clicks Cancel, then the queued send is aborted (best-effort) and the echo disappears; if already dispatched, it stays sent.
- Given a terminal upload failure, then the bubble shows the persistent "Failed — Retry" state like text sends and retry re-drives the send (NFR-5).
- Given an E2EE room, then sent media is encrypted by the SDK and decryptable by other clients in the Room.
- Given `bun run check:all`, then Biome, tsc, vitest, rustfmt, clippy (`-D warnings`), cargo-nextest, and `cargo deny check` all pass; ts-rs bindings regenerate without drift (no VM change expected).

## Spec Change Log

## Review Triage Log

### 2026-07-04 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 2: (high 0, medium 1, low 1)
- defer: 2: (high 0, medium 1, low 1)
- reject: 16: (high 0, medium 0, low 16)
- addressed_findings:
  - `[medium]` `[patch]` `onSendAttachments` looped over the pending attachments awaiting each enqueue but never removed a succeeded item from the tray. On a multi-attachment partial failure (a middle enqueue rejects — e.g. a dropped directory) or a failed trailing text send, the composer's `catch` kept the entire tray, so a user retry re-dispatched already-enqueued files → duplicate media messages. Fixed by removing each attachment from the store the moment it enqueues, so a retry re-sends only the un-enqueued remainder. Added a regression test (`conversation-pane.test.tsx`) asserting the succeeded attachment is dropped and only the failed one remains.
  - `[low]` `[patch]` `send_attachment_bytes` used the frontend-supplied paste filename verbatim as the sent event's filename, letting a compromised webview inject directory separators / odd path components into the `m.room.message` filename. Reduced it to its final path component via `Path::file_name` (defense-in-depth; the path route was already safe because it derives the name from the OS path). The `x-mime` header is already CTL-safe (rejected by `HeaderValue`) and decoded defensively.

## Design Notes

**Send-queue over direct upload.** `Timeline::send_attachment(...).use_send_queue()` enqueues and returns fast, letting the SDK drive the upload in its background send-queue task. This is the architecturally consistent choice: it produces a **local-echo timeline item** (same envelope as text), reuses the send-state diff stream (`sending → sent/failed`), inherits the offline-queue behavior from Story 1.7, and makes `retrySend` (`unwedge`) work for media echoes with no change. The trade-off is that the send-queue path does **not** surface transmitted/total bytes in matrix-sdk 0.18, so MVP shows an **indeterminate uploading indicator** (consistent with 3.6's indeterminate download precedent) rather than a byte-% bar; a determinate bar (direct `send_attachment().with_send_progress_observable()` path, which sacrifices the queue/echo/retry consistency) is logged as deferred.

**Local echo renders for free.** A media local echo is just a `MsgLikeKind::Message` timeline item with a media `msgtype` and a (locally-cached) `MediaSource`. The 3.6 receive path (`media_vm` → `keeper-media://` → `fetch_media`/`get_media_content`) already maps and renders it — so there is **no new VM field and no new timeline variant**. The uploading + Cancel affordance is derived purely from the existing `sendState` on the item (`"sending"` + `media` present), not from a new VM. If the echo's bytes aren't yet in the SDK media cache, the bubble thumbnail 404s and the existing retry affordance loads it once available — no crash/blank.

**Bytes-off-IPC ingestion.** Anti-pattern: no media through IPC as base64/JSON. So the two large-file paths (composer attach button via `@tauri-apps/plugin-dialog` `open()`, and native Tauri drag-drop) yield **OS file paths**; Rust reads the file itself. Only **paste** (clipboard image, which has no path) crosses bytes, and it does so as a **raw binary IPC body** (`tauri::ipc::Request` / `InvokeBody::Raw`, ~1× size, not base64), with `account_id`/`room_id`/`filename`/`mime`/`caption` in request headers — the sanctioned exception for path-less pasted images.

**Cancel = abort, symmetric with retry.** `send.rs::retry` already resolves a timeline item and calls the SDK send handle's `unwedge` for wedged echoes. `cancel` mirrors it: resolve the item by `unique_id`, take the send handle, call `.abort()`. Best-effort — `Ok(false)` (already dispatched) is a no-op that leaves the message sent. This keeps "cancelable during upload" honest without inventing a new cancelation channel.

## Verification

**Commands:**
- `bun run check` -- Biome + tsc + vitest green (incl. new composer/bubble/pane tests).
- `bun run check:rust` -- rustfmt clean + clippy `-D warnings` clean.
- `bun run test:rust` -- cargo-nextest green; ts-rs bindings regenerate with no git drift.
- `cargo deny check` (from `src-tauri/`) -- green; `mime_guess` + `tauri-plugin-dialog` pass the license firewall (advisories-only pre-existing gtk-rs failures excepted, as in 3.6).

**Manual checks (real second session, test credentials in 1Password):**
- In an encrypted room, attach a file via the button, paste a screenshot, and drag a file onto the pane — each shows a local echo with an uploading indicator; confirm the receiving Element client renders a playable/openable message.
- Attach a 25 MB video; confirm it uploads and plays on the receiving side; click Cancel mid-upload on a fresh large send and confirm the echo disappears.
- Force a terminal failure (kill network mid-send); confirm the bubble shows "Failed — Retry" and retry succeeds after reconnect.
- Via dev tools, confirm no media bytes traverse the JSON IPC channel — attach/drop use paths; paste uses a raw binary body.

## Auto Run Result

Status: done

**Summary:** Implemented the media/file **send** leg of FR-13 — attach via a native file picker, paste an image, or drag-drop a file into a Chat — reusing the SDK send-queue plumbing behind text so local echo, send-state stream, offline queueing, and retry all come for free. Sending goes through one new dispatch gate `send::submit_attachment` calling matrix-sdk-ui `Timeline::send_attachment(...).use_send_queue()` (the SDK encrypts automatically in E2EE rooms and decrypts on other clients), plus a symmetric `send::cancel` (`SendHandle::abort`) for cancel-during-upload. Media bytes never cross IPC as base64/JSON: the attach button + drag-drop deliver **OS file paths** (Rust reads the file), and paste delivers a **raw binary IPC body** (`tauri::ipc::Request` / `InvokeBody::Raw`, metadata percent-encoded in headers). The local echo renders through the unchanged 3.6 receive path (`MediaVm` + `keeper-media://`) — no new VM. The composer gained a pending-attachment tray (removable chips), the conversation pane a window drag-drop listener, and in-flight own media echoes an uploading indicator + Cancel overlay; terminal failures reuse the existing "Failed — Retry" caption.

**Files changed:**
- `src-tauri/crates/keeper-core/src/send.rs` — `submit_attachment` (sole `Timeline::send_attachment` gate, `.use_send_queue()`) + `cancel` (`SendHandle::abort`); single-dispatch-gate guard test extended.
- `src-tauri/crates/keeper-core/src/account.rs` — `send_attachment_path` (reads file, `mime_guess`), `send_attachment_bytes` (paste bytes; filename now reduced to its basename), `cancel_send`; mime-guess tests.
- `src-tauri/crates/keeper-core/src/error.rs` — `SendError::Upload` (retriable).
- `src-tauri/crates/keeper-core/Cargo.toml` + `src-tauri/Cargo.toml` — `mime`/`mime_guess`, `tauri-plugin-dialog` (workspace).
- `src-tauri/crates/keeper/src/ipc.rs` — `send_attachment_path`, `send_attachment_bytes` (raw body + percent-decoded headers), `cancel_send`; `SendError::Upload` mapped; tests.
- `src-tauri/crates/keeper/src/lib.rs` + `Cargo.toml` + `capabilities/default.json` — command registration; `tauri_plugin_dialog::init()`; `dialog:allow-open`.
- `package.json` — `@tauri-apps/plugin-dialog`.
- `src/lib/ipc/client.ts` — `sendAttachmentPath`, `sendAttachmentBytes` (raw-body invoke), `cancelSend`.
- `src/lib/stores/attachments.ts` (new) — pending-attachment tray store.
- `src/components/chat/composer.tsx` — attach button, image paste, removable chip tray, dispatch + caption logic.
- `src/components/layout/conversation-pane.tsx` — drag-drop listener, `onSendAttachments` (now drops each item from the tray as it enqueues), `onCancelSend` wiring.
- `src/components/chat/message-bubble.tsx` + `media-attachment.tsx` — uploading indicator + Cancel overlay on in-flight own media echoes.
- Tests: `composer.test.tsx`, `message-bubble.test.tsx`, `conversation-pane.test.tsx` (incl. the partial-failure no-duplicate regression test).

**Review findings breakdown:** intent_gap 0, bad_spec 0, patch 2 (medium 1, low 1 — both applied + tested), defer 2 (medium 1, low 1), reject 16 (all low).
- **Patches applied:** (1) [medium] `onSendAttachments` didn't remove enqueued items from the tray, so a multi-attachment partial failure (or failed trailing text send) re-dispatched already-enqueued files on retry → duplicate sends; now each item is removed the moment it enqueues, with a regression test. (2) [low] paste filename used verbatim as the sent event filename; reduced to its basename (defense-in-depth; the path route was already safe).
- **Deferred (2):** no upload size ceiling (whole file/body read into memory — spec-acknowledged, shared with 3.6's receive-side entry); media-as-reply drops the reply relation (out of 3.7 ACs). Both logged in `deferred-work.md`.
- **Rejected (16, all low):** forgeable-but-safe opaque cancel key (→`EchoNotFound`), `x-mime` trust (CTL-safe, octet-stream fallback), cross-platform basename label cosmetics, window-global drop scope (chip is visible + removable, not auto-sent), Cancel-overlay wording during `sending`, narrow drag-drop re-subscribe race (double-adds a removable chip), monotonic attachment id, empty `file.type` on paste (octet-stream fallback), multi-image paste taking only the first, `arrayBuffer()` rejection (effectively never for a clipboard File), degenerate zero-byte / empty-paste, directory-drop residual (mitigated by patch 1), remove-chip-mid-send snapshot divergence.

**Verification performed (independently re-run after patches):**
- `bun run check` — Biome clean, tsc clean, **387** vitest tests pass (41 files; +1 regression test).
- `bun run check:rust` — `cargo fmt --check` clean, clippy `-D warnings` clean.
- `bun run test:rust` — cargo-nextest **263** tests pass; ts-rs bindings regenerated with **no drift** (no VM change).
- `cargo deny check licenses bans sources` — ok (`mime`/`mime_guess`/`tauri-plugin-dialog` firewall-clean). The pre-existing `advisories` failure is the gtk-rs/GTK3 `unmaintained` RUSTSEC set from Tauri's Linux backend (also reached via `tauri-plugin-dialog`→`rfd`→GTK3) — present on the baseline, no new advisory introduced.

**Residual risks:** Live E2EE send against a real second Matrix session (Element decryptability, the 25 MB video playing on the receiving side, real drag-drop/paste/cancel in the running app) was not exercised in this environment — see the spec's Manual checks. The two deferred items (upload size ceiling, media-as-reply) remain open.
