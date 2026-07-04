# Epic 3 Context: Trusted, Full-Fidelity Conversations — E2EE & Rich Messages

<!-- Generated from planning artifacts. Regenerate with compile-epic-context if planning docs change. -->

## Goal

Bring keeper's messaging to table stakes: transparent end-to-end encryption that "just works" when healthy and states its limits plainly when not, plus the rich-message features users expect across Matrix-native and bridged Chats. This epic delivers E2EE encrypt/decrypt with device verification and key backup/restore, replies and edits, reactions, media and files (received and sent), delete-for-everyone via redaction with honest cross-network framing, read receipts/typing indicators, and smooth deep-history pagination. It matters because encryption and message fidelity are the baseline for keeper being usable as a primary messenger, and because keeper's differentiator is honesty rendered as UI — undecryptable events, best-effort remote deletions, and history boundaries are all surfaced explicitly rather than hidden.

## Stories

- Story 3.1: Encrypted Rooms — Decrypt, Encrypt, and Honest UTD States
- Story 3.2: Device Verification — Emoji/SAS and QR
- Story 3.3: Key Backup — Enable and Restore
- Story 3.4: Replies and Edits
- Story 3.5: Reactions
- Story 3.6: Receive Media — Thumbnails, Protocol Streaming, Preview
- Story 3.7: Send Media and Files
- Story 3.8: Delete for Everyone — Redaction
- Story 3.9: Receipts, Typing, and History Pagination

## Requirements & Constraints

- **E2EE transparency and honesty:** Encrypt outgoing and decrypt incoming messages transparently; messages sent from keeper into an encrypted Room must be decryptable by other Matrix clients. Events that cannot be decrypted must render an explicit stub with a recovery hint and an inline path to verification — never a blank.
- **Device verification:** Support interactive verification against an existing session (e.g. Element) via emoji/SAS comparison and QR scan/display, bidirectionally; after verification the keeper device shows trusted on both ends and previously-undecryptable events re-render where keys arrive.
- **Key backup:** Enable server-side key backup and restore historical encrypted messages with a recovery key. The recovery key is shown exactly once; invalid recovery keys produce named inline errors, not generic failures.
- **Replies/edits/reactions:** Replies render the quoted original inline and round-trip on remote Networks where the Bridge supports it; edits update in place with an "Edited" caption; reactions aggregate per emoji with the user's own highlighted and round-trip in native and bridged Chats. Incremental reaction/edit events must render via the diff stream without full timeline re-render.
- **Media and files:** Send and receive images, video, audio, and arbitrary files with thumbnails, upload/download progress, cancelable uploads, and inline preview. Encrypted media must be encrypted on send and decryptable by other clients. Test bar: a 25 MB video sends with progress and produces a playable message. MVP plays received audio inline; voice-note recording is out of scope.
- **Redaction:** Delete-for-everyone issues a Matrix redaction rendering a stub for all Matrix clients; in bridged Chats the confirmation must name the Network and state that remote removal is best-effort.
- **Receipts/typing/pagination:** Emit public read receipts and typing notifications, and render others' within ~2 s. Back-pagination through ≥ 10k events must not freeze the UI; an inline boundary row indicates homeserver-sourced history with a spinner, and offline it says so and stops rather than spinning.
- **No silent loss:** Every outgoing message (text or media) reaches a terminal user-visible state — sent, or a persistent "Failed — Retry".
- **Performance:** Interaction latency bar governs pagination and diff rendering (cached timeline switch < 150 ms; input < 16 ms/frame).
- **Accessibility:** All flows keyboard-operable with labeled controls (verification flow fully keyboard-operable).

## Technical Decisions

- **Rust-core confinement (AD-1 / NFR-9):** All E2EE key material, message plaintext, crypto, protocol state, and persistence live exclusively in `keeper-core`. The webview receives only rendered view models — no crypto, no message DB, no key material, and no plaintext ever cross into JavaScript. E2EE work lives behind `keeper-core`'s e2e-encryption feature.
- **Media over the `keeper-media://` protocol (AD-4 / NFR-9):** Decrypted media bytes travel exclusively over the Range-capable `keeper-media://` custom protocol served from the Rust media cache — never as base64/JSON over IPC. Thumbnails render before full download; download progress and retry surface on the bubble.
- **IPC shape (AD-4):** One-shot actions (verify, enable backup, restore, react, redact) are Tauri commands; ordered high-frequency timeline updates stream as `VectorDiff`-style batches over `Channel<T>`; low-frequency broadcasts are Tauri events.
- **Typed VM boundary (AD-7):** Every type crossing IPC lives in `keeper-core::vm` (suffix `Vm`), derives serde (camelCase) + ts-rs `#[ts(export)]`, and is emitted to the generated TS bindings; CI fails on drift.
- **`signals` module as sole signal emitter (AD-14):** A new `keeper-core::signals` module is established here as the only place allowed to call SDK receipt/typing/presence APIs. It emits public `m.read` receipts and typing notices for normal operation. Enforce by convention test or lint that no other module calls those SDK APIs. Full Incognito policy resolution (private receipts, dropped typing, per-scope precedence) lands in Epic 8 — only the module seam is created now.
- **Verification/backup UX vocabulary:** Render the SDK's native verification and key-backup flows (emoji/SAS, QR, recovery key) using Element-X-style patterns — do not invent novel crypto UX. Element X is a patterns-only reference; its AGPL code is study-only (licensing firewall).
- **Recovery keys / IDs typography:** Recovery keys, Matrix IDs, and verification codes render in `mono` type.

## UX & Interaction Patterns

- **Undecryptable stub:** Explicit timeline stub — "Can't decrypt yet — verify this device or restore key backup" — with an inline action into the verification flow. Never blank.
- **Unverified-device banner:** Post-login global banner "Verify this device to read encrypted history" links to the verification flow; dismissing collapses it to a persistent badge on Settings (not gone).
- **Verification flow states:** Waiting, comparing, confirmed, cancelled, and failed each render distinctly using the SDK flow vocabulary; fully keyboard-operable.
- **Timeline action bar:** Hover/focus reveals React (emoji Popover), Reply, Edit (own), Delete ▸, Copy, and Jump-to-original on reply quotes. Received edits show latest content + "Edited" caption; reactions render as a pill row under the bubble with counts and own-reaction highlight.
- **Keyboard affordances:** `↑`/`↓` select messages; `r` reply; `e` edit own; `↑` in an empty composer edits last own message; `⌫` opens delete (AlertDialog); `Esc` cancels a pending edit/reply without losing composer text.
- **Media preview:** Click or Enter opens a Quick-Look-style overlay; `Esc` closes and returns focus to the timeline; video/audio play via the protocol URL. Failed media shows retry.
- **Redaction framing:** Delete confirmation in a bridged Chat names the Network and states removal there is best-effort (e.g. "Deletes your copy on this Mac. … Removal on Telegram is best-effort.").
- **History boundary row:** Inline "Older history loads from your homeserver" with a spinner while paginating; offline, it says so and stops.
- **Read state rendering:** Ticks on own messages; others' read receipts as micro-avatars at their read position.

## Cross-Story Dependencies

- Stories 3.2–3.9 depend on 3.1 (encrypted rooms) being in place; 3.2 (verification) precedes 3.3 (key backup); 3.7 (send media) builds on 3.6 (receive media).
- The whole epic depends on Epic 2 and works with ≥ 1 account from Epic 1 onward.
- 3.4 replies/edits deliver the timeline leg only; local-archive retention of pre-edit content and redaction history is governed by Epic 5 (Story 5.2).
- 3.9 seeds the `signals` module; its Incognito policy logic lands in Epic 8.
