---
title: 'Story 61.12: an image you can paste, a path you can open'
type: 'feature'
created: '2026-09-02'
status: 'review'
baseline_revision: '9e06c2b'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-392, FR-393 → AD-160; AD-58 (bytes never cross IPC, consumed), AD-151 (a capability is read, never assumed, consumed), AD-158 (a path is opened only inside a grant, consumed), AD-27 (no affordance that lies, consumed)
depends on: 61.3 (the tri-state `vision` capability on `BotModelVm`) and 61.10 (`grant::decide`, the grant list the reveal check reads)

<intent-contract>

## Intent

**Problem:** The owner asked for *"image mode (with paste from clipboard), deliverable mode (look hermes)"*. The epic's finding: the plumbing exists, pointed the other way — pasted images already reach Rust as a percent-decoded raw IPC body, never base64 through JSON (AD-58, `notes_vault.rs`); and deliverable mode is a *gateway* feature: Hermes strips `MEDIA:` directives and absolute paths from the visible text and uploads the file, whereas over the api-server a client receives the path itself. The epic's rules: attach an image only for a model whose vision is `true` or `unknown`, refuse for `false` with the model's name in the sentence; state size and count caps; and handle deliverables from the receiving end — recognise an absolute path, offer *reveal* or *open* **only inside a grant**, never fetch or copy a path a model named outside one, and never strip text from a reply, because here the reply is the record.

**Approach:** Both halves in `keeper_core::bots::deliverable`. Outbound: `image_offer` (tri-state) and `accept_image` (the full gate), raw-body staging on disk (`stage_image`/`read_staged`/`discard_staged`), and `image_content_part` building a `ContentPart::Image { url: data:… }` that the existing `chat.rs` serializer emits per kind from `quirks::ImagePartShape` — no new per-provider branch. Inbound: `scan_paths` with a code mask, `resolve_deliverables` with root matching by path component and the grant check, returning byte offsets into the reply as received. The frontend extends the proven `chat/composer.tsx` + `sendAttachmentBytes` shape: `clipboardData → File → ArrayBuffer → tauriInvoke(cmd, arrayBuffer, { headers }) → InvokeBody::Raw`, on a bots-owned raw-body command rather than a Matrix one widened with two null arguments.

## Boundaries & Constraints

**Always:**
- **The caps, borrowed not invented.** `MAX_IMAGE_BYTES` = 8 MiB — `note_protocol::MAX_RANGE_CHUNK`, the ceiling the tree already puts on one read of one asset into the webview; a screenshot from any current display is an order of magnitude under it, and its ~10.7 MB base64 expansion is the largest JSON body keeper ever hands `reqwest`, a bound worth having when the far side is a local process with a multi-GB model resident. `MAX_IMAGES_PER_MESSAGE` = 4 — the context window, not the byte count: Ollama's `/v1` layer cannot set `num_ctx` (the quirk table's `context_window_over_v1: Support::No`, DW-210), each image costs on the order of a thousand tokens tiled, and a local model at its 4k default still has room for the question at four; a fifth is refused because keeper cannot tell the model to make room for it. `IMAGE_MIMES` = `image/png`, `image/jpeg`, `image/webp`, `image/gif` — the intersection of the vision guide's documented types and what the back ends take; SVG is excluded because Ollama has errored on SVG input since v0.4.6 and an SVG is a document that can carry script (the same reason `note_protocol.rs` serves it only behind `nosniff`). The TypeScript twins (`BOT_IMAGE_MAX_BYTES`, `BOT_IMAGE_MAX_COUNT`, `BOT_IMAGE_MIMES`) are pinned to the Rust source by a parity test, refusal sentences included.
- **The tri-state vision gate, in this order:** `Some(true)` ⇒ offer; `None` ⇒ offer with a warning naming the model (keeper could not read it, which is its own sentence and never `false`); `Some(false)` ⇒ refuse with the model's name. The capability is checked **before** the size cap: a person pasting into a bot that cannot see must hear that, not that their screenshot is large — a true sentence about the wrong problem.
- **Image bytes never cross IPC as base64 JSON.** They travel as an `InvokeBody::Raw` body under headers `x-mime`, `x-filename` (percent-encoded), `x-bot-id`, `x-model` (percent-encoded), `x-attached`; no bytes in any header and none in any JSON. Base64 exists only inside the outbound HTTP body — never in an IPC payload, a row, a DOM `src` (the tray shows a `blob:` URL) or a log line. Staged at `<data_dir>/bots/attachments/<ulid>.<ext>`; read back at send time by id matched against the folder's own listing, never joined from caller input; turned into a data URI; discarded. `detail` is left unset: the guide's own warning is that `low` does not always cost fewer tokens than `high`.
- **The path scanner.** Code is masked before any matching: fenced blocks (a run of 3+ backticks or tildes at line start, closed by a run of the same character at least as long; an unterminated fence masks to the end of the reply, which is the right answer mid-stream) and inline code spans (a backtick run closed by an equal run on the same line). A mention starts at `/` (not `//`) or `~/`, only where the preceding character is not alphanumeric and not one of `/ ~ . - _ : %` — which is what makes `https://example.com/x`, `a/b/c` and `3/4` yield nothing without a scheme list. The run ends at whitespace or `" ' < > | \` NUL; trailing `. , ; : ! ? ) ] } /` are trimmed repeatedly; what survives must have a non-empty segment after its root. Windows spellings are not matched — keeper's drive paths are POSIX on both shipped platforms, and a pattern loose enough for `C:\Users` catches every `12:30`. Offsets are byte offsets into the reply as received: `resolve_deliverables` returns coordinates for a decoration, never an edited string.
- **The grant check on reveal.** `~/` expands against HOME (with no HOME it stays a `~` path and gets the outside-the-drive sentence). Root matching is by path component (`strip_prefix`), deepest root wins — `/home/ada/Drive-old` is not inside `/home/ada/Drive`. The verdict is `grant::decide(grants, target, Effect::Read)`; anything but `Allow` yields no control. `exists()` is consulted **only after** the grant said yes, so a reply author cannot use reveal-or-not as an existence oracle over the drive.
- `BotReplyPaths` asks nothing while an answer is still streaming, and renders no control when the resolution failed.

**Block If:** nothing; every cap has a source in the tree and every rule is in the epic.

**Never:** base64 in JSON; a per-provider image branch outside `chat.rs`; a scheme list; an edited reply; a stat before a grant; an `/image` slash command (61.9 declined it, and the paste is the gesture).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Text paste; no bot | `text/plain`; `context === null` | passthrough — the browser's own paste | No error expected |
| Vision true / unknown / false | `image/png`, 4096 bytes | attach; attach with the warning naming `mystery:7b`; refuse naming `llama4:8b` | The sentence IS the handling |
| Capability before size | vision `false` and an oversize file | the vision refusal, not the size one | The sentence IS the handling |
| Oversize; at the cap | `MAX_IMAGE_BYTES + 1`; exactly `MAX_IMAGE_BYTES` | refused with both numbers in the sentence; accepted — the boundary is inclusive so the refusal never fires on a size equal to the number it quotes | The sentence IS the handling |
| Fifth image | `attached = 4` | refused with the count in the sentence | The sentence IS the handling |
| SVG | `image/svg+xml` | refused; `BOT_IMAGE_MIMES` does not contain it | The sentence IS the handling |
| Staging | raw body over IPC | `<data_dir>/bots/attachments/<ulid>.png` round-trips; an oversize body is refused before it writes | `internal` |
| Content part | png bytes `00 01 02 03` | Hermes `{"type":"image_url","image_url":{"url":"data:image/png;base64,AAECAw=="}}`; Ollama `{"type":"image_url","image_url":"data:image/png;base64,AAECAw=="}` | No error expected |
| Path in prose; two on one line | `… to /home/ada/Drive/notes/summary.md for you.`; `Compare /a.md and /b.md.` | one mention; two mentions, trailing `.` trimmed | No error expected |
| Path in a fence; in a code span; prose around a fence | a `sh` fence holding `cat /…/secret.md`; an inline span holding `/…/conf.toml`; a fence followed by `/…/outside.md` in prose | nothing; nothing; only `outside.md` | No error expected |
| URL, ratio, bare slash | `https://example.com/notes/a.md`, `3/4`, `/` | nothing | No error expected |
| Inside a read grant / outside every grant / grant `none` / outside every synced folder | the same path under each grant set | reveal offered; text with the reason and no control; nothing; nothing (`/etc/hosts`) | The reason IS the handling |
| Sibling root; `~/` with and without HOME; granted but absent; no grant and absent | `/home/ada/Drive-old/…`; `~/Drive/…`; a granted path not on disk | not inside; expanded / outside-the-drive; nothing; nothing **and no stat was issued** | No error expected |

</intent-contract>

## Code Map

| Path | Change |
|---|---|
| `src-tauri/crates/keeper-core/src/bots/deliverable.rs` | new, 776 lines: both halves — caps, `IMAGE_MIMES`, the five refusal-sentence builders, `human_bytes`, `ImageOffer` + `image_offer` + `accept_image`, `stage_image`/`read_staged`/`staged_mime`/`discard_staged`, `image_data_uri` + `image_content_part`; `PathMention`, `scan_paths`, `code_mask` + `fence_run` + `mask_inline_code`, `DeliverableRoot`/`DeliverableControl`/`Deliverable`, `resolve_deliverables`, `expand_home`, `classify`, `inside_a_root`, the three `NO_CONTROL_*` sentences |
| `src-tauri/crates/keeper-core/tests/bots_deliverable.rs` | new, 501 lines, 27 tests, every name prefixed `deliverable_` |
| `src-tauri/crates/keeper-core/src/bots/mod.rs`, `vm.rs`, `Cargo.toml` | `pub mod deliverable;`; `BotAttachmentVm`, `BotDeliverableVm` (+ compose), and one field on the existing `BotChatSendReq`: `#[serde(default)] pub attachment_ids: Vec<String>`; `base64 = { workspace = true }` with the house justification comment — not a new dependency (already the LFS Basic-auth header's, already in `Cargo.lock`) |
| `src/lib/ipc/gen/BotAttachmentVm.ts`, `BotDeliverableVm.ts` | generated |
| `src-tauri/crates/keeper/src/bots_ipc.rs`, `lib.rs` | `bots_image_paste` (raw body), `bots_image_discard`, `bots_deliverable_paths`, plus `vision_of`, `attach_staged_images`, `deliverable_roots`, the two header readers; two lines inside `bots_chat_send` (`let messages = attach_staged_images(…)`); 3 registrations — **not compiled on this host** |
| `src/components/bots/bot-paste.ts` | rewritten, 205 lines (replaces 61.4's stub): `BotPasteContext`, the three-arm `BotPasteDecision`, `botPasteDecision(data, context)`, the caps, the copy, `botHumanBytes` |
| `src/components/bots/bot-attachment.tsx` | new, 421 lines: `BotAttachmentStrip` (composer tray), `BotDeliverablePaths` (presentational), `BotReplyPaths` (self-fetching wrapper mounted by `bot-message`), `useBotImagePaste` (owns the bytes path, caps, object-URL lifetime and the staging round trip), `botAttachmentCaption` |
| `src/components/bots/bot-paste.test.ts` (17), `bot-attachment.test.tsx` (14) | new |
| `src/lib/ipc/client.ts`, `dev/mock-shell.ts` | `botsImagePaste` (raw-body `tauriInvoke`), `botsImageDiscard`, `botsDeliverablePaths`; three mock handlers |
| `src/components/bots/bots-pane.tsx` | coordinator-owned wiring, 5 lines: `useBotImagePaste(selectedBotId, selectedModel, pickedModel?.vision ?? null)`, `attachmentIds: imagePaste.take()` in the send request, `<BotAttachmentStrip …/>` before the composer, `pasteContext`/`onPaste` on it |
| `src/components/bots/bot-message.tsx` | 2 lines: `<BotReplyPaths sessionId body streaming />` after `<BotContextNote>` for non-own rows |
| `src/components/bots/bot-composer.tsx` | not touched — 61.9 added the two agreed optional props to the contract sent over hub |

## Tasks & Acceptance

**Execution:**
- [x] `deliverable.rs` (+ 27 tests) — the gate, the staging, the content part, the scanner, the resolver.
- [x] `bots_ipc.rs`, `lib.rs`, `client.ts`, `mock-shell.ts`, `vm.rs` — the raw-body command and its two siblings.
- [x] `bot-paste.ts`, `bot-attachment.tsx` (+ tests) and the seven wiring lines.

**Acceptance Criteria:**
- Given a model whose vision is `false`, when an image is pasted, then the refusal names the model and nothing is staged. **Met** — `deliverable_vision_false_refuses_and_names_the_model`; mutations (a) and (a, TS).
- Given a path inside a fenced block or a code span, when the reply is scanned, then it is not a mention. **Met** — `deliverable_ignores_a_path_inside_a_fenced_block`, `…_inside_an_inline_code_span`; mutation (b).
- Given a path no grant covers, when the reply is resolved, then it is text with a reason, no control, and no `stat` was issued. **Met** — `deliverable_outside_every_grant_renders_as_text_with_a_reason`, `deliverable_never_stats_a_path_no_grant_covers`; mutation (c).
- Given the bytes, when they leave the webview, then no base64 exists anywhere but the outbound HTTP body. **Met** — `deliverable_data_uri_exists_only_in_the_outbound_request_body`, *never puts the bytes anywhere but the File it was handed*, *shows the thumbnail from a blob URL and never a data URI*.

## Design Notes

**Which paste route was extended, and which was left alone.** `notes_attachment_paste` still refuses with `Unsupported`, and that refusal is untouched and still correct: reading the system clipboard from Rust needs a clipboard backend this build does not link. What this story extends is the webview-paste route the Matrix composer already proves. **Why `~/` and HOME.** A model on a Mac writes `~/Drive/…` as readily as the absolute form; expanding it against HOME and then applying the same component-wise root match keeps one rule for both spellings, and a missing HOME degrades to the honest sentence rather than a guess.

## Deferred

- The native `/api/chat` dialect, and with it `num_ctx` — which is the reason the count cap is four and not a computed number. **DW-210.**

## Verification

- `cargo nextest run -p keeper-core -E 'binary(bots_deliverable)'` → 27 passed; with the export-bindings test → 28 passed. `cargo clippy -p keeper-core --all-targets -- -D warnings` → clean.
- `bunx vitest run bot-paste.test.ts bot-attachment.test.tsx` → 2 files / 31 passed (17 + 14). `bunx tsc --noEmit` → clean. `bunx biome check` on the story's 8 files → clean.

| Mutation | Tests that failed | Observed |
|---|---|---|
| (a) `image_offer`: `Some(false) => ImageOffer::Offer` | `deliverable_vision_false_refuses_and_names_the_model`, `deliverable_vision_is_checked_before_the_size_cap` | 25 passed, 2 failed of 27 |
| (b) `scan_paths`: `code_mask(reply)` → `vec![false; reply.len()]` | `deliverable_ignores_a_path_inside_a_fenced_block`, `…_inside_an_inline_code_span`, `deliverable_never_rewrites_the_reply` | 3 failed |
| (c) the `decide(…) == Allow` guard in `resolve_deliverables` → `if false` | `deliverable_a_grant_set_to_no_access_offers_nothing`, `deliverable_outside_every_grant_renders_as_text_with_a_reason`, `deliverable_never_stats_a_path_no_grant_covers` | 24 passed, 3 failed (`--no-fail-fast`) |
| (d) `accept_image`: `byte_len > MAX_IMAGE_BYTES` → `> usize::MAX` | `deliverable_oversize_refusal_carries_both_numbers` | 26 passed, 1 failed |
| (a, TS) `context.vision === false` → `=== undefined` | *refuses an image for a model that cannot see, naming the model*, *refuses on the capability before it refuses on the size* | 2 failed of 17 |
| (d, TS) `file.size > BOT_IMAGE_MAX_BYTES` → `* 2` | *refuses an oversize image with both numbers in the sentence* | 1 failed / 16 passed |

Every site restored and re-read from source (`git diff --stat` on `bot-paste.ts` empty; the four Rust sites printed and checked); 27/27 and 31/31 green again. The coordinator's full gate afterwards: 4028 Rust tests, 5466 frontend tests, lint/typecheck/fmt/clippy clean.

## Auto Run Result

Status: implemented and gated; uncommitted; not reviewed.

**Not verified here, and why.** The three commands, the helpers around them and the two lines inside `bots_chat_send` were never compiled — the `keeper` shell crate does not build on Linux. No image has crossed a real `InvokeBody::Raw` boundary here: the raw-body shape is the Matrix composer's, proven in production, but this command's header readers are unexercised. No pixels: the tray thumbnail, the caption `1280×720, 180 kB` and the reveal control are asserted in jsdom. The deliverable resolver was exercised against temp roots, not a mounted sync profile; `deliverable_roots` reads `engine.list_profiles()` in the uncompiled shell.
