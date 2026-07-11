---
title: 'Media Protocol on WKURLSchemeHandler with Capped Buffers'
type: 'feature'
created: '2026-07-11'
status: 'done'
review_loop_iteration: 0
followup_review_recommended: false
context: []
warnings: [oversized]
baseline_revision: fa2c09152911a89887d995899598ab29c27f96e8
final_revision: 948b77026715bccda8433cf721d86f89d76b19c4
---

<intent-contract>

## Intent

**Problem:** On iOS the `keeper-media://` handler already runs (wry → WKURLSchemeHandler, registration is unconditional), but its Range path copies the sliced bytes with `.to_vec()`. A single open-ended `bytes=0-` request from a `<video>`/`<audio>` slices the *whole* body, allocating a second full-size copy alongside the whole in-memory media — the extra allocation that pushes an app past the iOS jetsam limit and gets it killed.

**Approach:** Cap the per-response Range slice at a named constant (`MAX_MEDIA_RANGE_CHUNK`, 8 MiB on iOS) so an open-ended Range returns a bounded 206 and the webview transparently continues with follow-up Range requests. The cap is a jetsam guard, so it is iOS-only; desktop keeps a `u64::MAX` cap that is a proven no-op, leaving desktop Range behavior byte-identical. Disk-backed streaming (avoiding the base whole-file in-RAM load) is recorded as deferred work, not implemented.

## Boundaries & Constraints

**Always:**
- Keep the `keeper-media://` registration unconditional (it already serves iOS via WKURLSchemeHandler); keep the async, fire-and-forget `responder.respond(...)` shape so a WebKit scheme-task invalidated mid-fetch is a tolerated no-op, never a panic.
- The cap is a named `const` with a host-runnable unit test asserting it. The 206 `Content-Range` must report the clamped inclusive `end` against the true `total`; `Accept-Ranges: bytes` stays set so the webview keeps seeking.
- Desktop build behavior stays byte-identical (`capped_range_end(_, end, u64::MAX) == end`); all desktop gates stay green. No `unsafe`, no `.unwrap()`/`.expect()` in production paths; iOS `cargo check`/`clippy` clean (no dead-code/unused-const).
- No media bytes cross IPC; the 200 full-body path stays a move (no second copy) so images (which issue no Range) are never truncated.

**Block If:**
- Bounding the Range slice cannot keep desktop byte-identical without a broader redesign (it can, via the `u64::MAX` no-op cap — if that proves false, stop).
- The iOS target toolchain (`aarch64-apple-ios`) is unavailable, so `cargo check --target aarch64-apple-ios` cannot run to verify the cap compiles.

**Never:**
- Do not implement disk-backed streaming, a 413/size-reject ceiling, or handle signing/auth (those are DW-29/DW-30/DW-31 — out of scope here).
- Do not cap the no-Range 200 full-body path (truncates images). Do not change `keeper-core` (`fetch_media`/`parse_media_url`) or the frontend retry path. Do not add `cfg(target_os)` to `keeper-core`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Open-ended Range, large media (iOS) | `bytes=0-`, body > 8 MiB | 206, body length == `MAX_MEDIA_RANGE_CHUNK`, `Content-Range: bytes 0-8388607/{total}` | No error |
| Open-ended Range, large media (desktop) | `bytes=0-`, body > 8 MiB | 206, full remaining body — byte-identical to today (cap = `u64::MAX` no-op) | No error |
| Small satisfiable Range | `bytes=2-5`, 10-byte body | 206, 4-byte slice, `Content-Range: bytes 2-5/10` (unchanged both platforms) | No error |
| No Range (image) | no `Range` header, any size | 200 full body, moved not copied (uncapped) | No error |
| Unsatisfiable Range | `bytes=100-200`, 8-byte body | 416, `Content-Range: bytes */8` (unchanged) | No error |
| Cold SDK cache after force-quit | media absent from cache | `get_media_content(_, true)` re-downloads → 200/206 and renders | Fetch failure → 404; frontend re-requests with `retry=1` cache-buster |
| Scheme task invalidated mid-fetch (iOS) | webview drops element before respond | `responder.respond` is a tolerated no-op | Fire-and-forget; no panic/crash |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper/src/media_protocol.rs` — the whole change lives here. `partial_or_full` (`:100-131`), `RangeParse::Satisfiable` slice arm (`:104-119`). Add `MAX_MEDIA_RANGE_CHUNK` const + pure `capped_range_end(start,end,cap)`; clamp `end` in the Satisfiable arm before slicing; extend the module doc (`:1-16`) with the iOS notes; add a host unit test in `mod tests` (`:236-385`, alongside `partial_response_slices_and_sets_content_range` at `:344`).
- `src-tauri/crates/keeper/src/lib.rs` — `register_asynchronous_uri_scheme_protocol("keeper-media", …)` (`:70-72`), unconditional. Reference only — do not modify (keeps desktop byte-identical).
- `src-tauri/crates/keeper-core/src/media.rs` — `fetch_media` / `get_media_content(_, true)` sole gate (`:247-267`); `parse_media_url` (`:128`). Reference only — unchanged.
- `src/components/chat/media-preview-overlay.tsx` (+ `.test.tsx:61-72`) — existing 404 retry affordance with `retry=1` cache-buster. Reference only — proves the retry-on-cache-miss path already exists.
- `_bmad-output/implementation-artifacts/deferred-work.md` — DW-29 (`:216`) tracks the whole-file-in-RAM concern; append a dated Story-12.4 progress note (append-only).

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper/src/media_protocol.rs` -- add `MAX_MEDIA_RANGE_CHUNK` (`8*1024*1024` on `cfg(target_os="ios")`, `u64::MAX` otherwise) and a pure `capped_range_end(start,end,cap)`; apply it to `end` in the `RangeParse::Satisfiable` arm of `partial_or_full` before slicing so the 206 body and `Content-Range` reflect the clamped end; extend the module doc with the iOS notes (WKURLSchemeHandler mapping, jetsam-cap rationale, scheme-task-invalidation tolerance, cold-cache retry). -- bounds the second in-memory copy on iOS while keeping desktop byte-identical.
- [x] `src-tauri/crates/keeper/src/media_protocol.rs` (`mod tests`) -- add a host-runnable unit test asserting `capped_range_end` clamps an over-long range to `cap` bytes (inclusive end `start+cap-1`), is a no-op when the range already fits and when `cap == u64::MAX` (the desktop no-op), and saturates near `u64::MAX` without overflow. -- satisfies "named constant + unit test asserting the cap" and pins the desktop byte-identical guarantee.
- [x] `_bmad-output/implementation-artifacts/deferred-work.md` -- append a dated `note:` under DW-29 recording that Story 12.4 bounded the Range seek-amplification leg via the slice cap, and that disk-backed streaming (the base whole-file in-RAM load) stays deferred. -- fulfills the epic's "disk-backed streaming is recorded as deferred work."

**Acceptance Criteria:**
- Given a satisfiable open-ended range on a body larger than the cap, when `capped_range_end(start, end, cap)` runs, then it returns `min(end, start+cap-1)` (host unit test), so the iOS 206 body length equals `MAX_MEDIA_RANGE_CHUNK` and `Content-Range` reports the clamped end with the true total.
- Given the desktop build, when `bun run check:all` runs, then it is green and media Range behavior is byte-identical — `capped_range_end(_, end, u64::MAX) == end` is asserted, and the desktop 206 path is functionally unchanged.
- Given the whole workspace, when `cargo check --target aarch64-apple-ios` and `cargo clippy --target aarch64-apple-ios -- -D warnings` run from `src-tauri/`, then both finish clean (cap compiles on iOS; no `unsafe`, no `.unwrap()`, no dead-code/unused-const).
- Given the media protocol, then registration remains unconditional (serves iOS via WKURLSchemeHandler) and the async fire-and-forget responder is unchanged; WebKit scheme-task invalidation is tolerated (no respond-time panic) — on-device confirmation folds into Story 12.6.
- Given a cold SDK cache after a force-quit, then a media request re-fetches via `get_media_content(_, true)` and renders; a fetch failure returns 404 and the existing `media-preview-overlay` retry re-requests with `retry=1` — no new code; on-device confirmation folds into Story 12.6.

## Design Notes

Why cap the slice, not the load: the 200 path *moves* the whole `Vec` into the response (no copy); only the 206 path `.to_vec()`s, so an open-ended `bytes=0-` copies the whole body → ~2× peak. Capping bounds that second allocation; the browser fetches later chunks. The base 1× atomic load is the disk-streaming concern (deferred, DW-29). 8 MiB is well below the 25 MB video bar (large video just streams in a few chunks) and images never issue Range (uncapped 200 move path), so nothing legitimate breaks. The cap is iOS-only (jetsam is iOS-only); desktop's `u64::MAX` cap provably returns `end` unchanged, keeping the helper unconditionally compiled and host-tested (no dead-code lint) while desktop stays byte-identical.

Golden shape (~9 lines):
```rust
/// Per-response Range slice ceiling. iOS: a hard jetsam guard so one open-ended
/// `bytes=0-` cannot allocate a second full-size copy of large media. Desktop:
/// `u64::MAX` — a no-op, keeping desktop Range behavior byte-identical.
#[cfg(target_os = "ios")]
const MAX_MEDIA_RANGE_CHUNK: u64 = 8 * 1024 * 1024;
#[cfg(not(target_os = "ios"))]
const MAX_MEDIA_RANGE_CHUNK: u64 = u64::MAX;

/// Clamp an inclusive range `end` so the slice is at most `cap` bytes. Pure.
fn capped_range_end(start: u64, end: u64, cap: u64) -> u64 {
    end.min(start.saturating_add(cap - 1))
}
// in partial_or_full, Satisfiable { start, end }:
let end = capped_range_end(start, end, MAX_MEDIA_RANGE_CHUNK);
```

## Verification

**Commands:**
- `cd src-tauri && cargo check --target aarch64-apple-ios` -- expected: `Finished` (iOS cap compiles for the whole workspace).
- `cd src-tauri && cargo clippy --target aarch64-apple-ios -- -D warnings` -- expected: no warnings (no unsafe, no unused const, no dead code).
- `bun run test:rust` -- expected: green, including the new `capped_range_end` test.
- `bun run check:all` -- expected: green (biome + tsc + vitest, `cargo fmt`/clippy, nextest, license firewall) — desktop unchanged.
- `git diff src-tauri/crates/keeper-core src-tauri/crates/keeper/src/lib.rs` -- expected: empty (core + registration untouched; change confined to `media_protocol.rs`).

**Manual checks (fold into Story 12.6 — not the blocking gate):** In the Simulator/on-device, seek a large video (repeated Range requests stay bounded, no jetsam kill), and after a force-quit with a cold cache confirm media re-fetches and renders (or shows the retry affordance). Not headless-automatable; the enforceable exit gate here is the iOS `cargo check` + `clippy` and the host unit test.

## Spec Change Log

## Review Triage Log

### 2026-07-11 — Review pass
- intent_gap: 0
- bad_spec: 0
- patch: 3: (high 0, medium 0, low 3)
- defer: 0
- reject: 8
- addressed_findings:
  - `[low]` `[patch]` `src-tauri/crates/keeper/src/media_protocol.rs` (`capped_range_end`) — both reviewers flagged the unguarded `cap - 1` as a latent underflow (debug panic / release wrap) if a future edit set the cap to 0. Changed to `cap.saturating_sub(1)` (byte-identical for both current cap values, total for all inputs) with a doc note. Re-verified green.
  - `[low]` `[patch]` `src-tauri/crates/keeper/src/media_protocol.rs` (`MAX_MEDIA_RANGE_CHUNK` doc) — the 8 MiB value was an unexplained magic number; documented that it sits well under the epic's 25 MB media bar so large media streams in successive capped chunks.
  - `[low]` `[patch]` `src-tauri/crates/keeper/src/media_protocol.rs` (module doc) — softened the WebKit scheme-task-invalidation tolerance from an asserted fact about wry internals to best-effort, with on-device confirmation deferred to Story 12.6.

Rejected (8): **suffix/explicit Range truncation** (the iOS cap clamps every satisfiable range's `end` to `start+cap-1`, so a `bytes=-N` suffix or an explicit finite range larger than the cap serves the head of the requested window) — refuted as actionable: the 206 is RFC-7233-compliant with an honest `Content-Range`, and the only in-app media consumers are WebKit `<video>`/`<audio>` (forward/open ranges, which continue) and `<img>` (no Range); nothing issues suffix `bytes=-N` or reads to `Content-Length` via `fetch`/XHR; a per-form anchored cap would require disproportionate parser restructuring for a case that does not occur, and real-media range behavior is exercised on-device in 12.6. **Test-coverage gaps** (no end-to-end assertion of the iOS clamp value) — the pure `capped_range_end` is exhaustively tested and the existing `partial_or_full` 206 test passes unchanged (desktop byte-identical wiring); the 8 MiB clamp cannot be host-tested because the const is `u64::MAX` off-iOS, matching the 12.3 iOS-only precedent. **Layered-not-duplicated clamp** — `parse_range`'s clamp-to-`last` (HTTP semantics) and the cap (memory guard) are separate concerns; the cap only lowers `end`. **Pre-existing/unreachable `unwrap_or_default` masking** — predates this story; the cap keeps `end` in `[start, last]`, so `.get` is always `Some`. **Jetsam-overstatement** — the doc claims only that the cap bounds the second copy (accurate), not that it solves the base load. **Deferred-note "bounded vs resolved"** — the note already states the leg is bounded and the base whole-file load stays deferred (DW-29 remains `open`). **Clippy / 32-bit truncation** — empirically clean on both desktop (`--all-targets`) and `aarch64-apple-ios`; iOS is 64-bit only.

## Auto Run Result

Status: done

**Summary of implemented change.** Story 12.4 caps the in-memory Range-slicing buffer of the `keeper-media://` handler so the protocol survives iOS jetsam limits. `fetch_media` loads the whole decrypted media atomically (matrix-sdk 0.18); the 200 no-Range path *moves* that `Vec` (no copy), but the 206 path `.to_vec()`s the slice — an open-ended `bytes=0-` from a `<video>`/`<audio>` would copy the whole body, allocating a second full-size copy and pushing peak past the jetsam limit. A named constant `MAX_MEDIA_RANGE_CHUNK` (8 MiB on iOS, `u64::MAX` — a proven no-op — on desktop) and a pure `capped_range_end(start, end, cap)` clamp the 206 slice; the webview transparently continues with follow-up Range requests. The cap is iOS-only, so desktop Range behavior stays byte-identical. The `keeper-media://` registration was already unconditional (runs on iOS via wry→WKURLSchemeHandler), the fire-and-forget responder already tolerates scheme-task invalidation, and the frontend cold-cache retry (`retry=1`) already exists — those legs were confirmed, not re-implemented. Disk-backed streaming (the base whole-file in-RAM load) remains deferred (DW-29).

**Files changed.**
- `src-tauri/crates/keeper/src/media_protocol.rs` — added `MAX_MEDIA_RANGE_CHUNK` + `capped_range_end`, applied the clamp in `partial_or_full`'s `Satisfiable` arm, extended the module doc with the iOS notes, and added the `capped_range_end_clamps_saturates_and_noops` unit test. No `unsafe`, no `.unwrap()`/`.expect()` in production paths.
- `_bmad-output/implementation-artifacts/deferred-work.md` — appended a dated Story-12.4 progress note under DW-29 (append-only; the seek-amplification leg is bounded, disk-backed streaming stays deferred).

**Review findings breakdown.** Two adversarial reviewers (Blind Hunter + Edge Case Hunter) at session model capability (Opus), run in parallel without prior context. 13 raw findings → 0 intent_gap, 0 bad_spec, **3 low patches** (the `cap - 1`→`saturating_sub` underflow hardening flagged by both, the 8 MiB rationale, and softening the scheme-task-invalidation doc claim), 0 defer, 8 reject (chiefly the suffix/explicit-range truncation — RFC-compliant with an honest `Content-Range` and no in-app suffix/`fetch` consumer; plus iOS-only test-coverage gaps folded into 12.6, and a clippy/32-bit concern empirically refuted). All patches are comment/one-token changes with no behavior change for the shipping cap values. No bad_spec loopback; `review_loop_iteration` stayed 0.

**Verification performed.**
- `cargo fmt --check` — clean.
- `cargo clippy --all-targets -- -D warnings` (desktop) — clean.
- `cargo clippy --target aarch64-apple-ios -- -D warnings` + `cargo check --target aarch64-apple-ios` — clean (iOS cap compiles; no unused-const/dead-code).
- `bun run test:rust` — 766/766 passed, incl. `media_protocol::tests::capped_range_end_clamps_saturates_and_noops`.
- `git diff` of `keeper-core` + `lib.rs` — empty (change confined to `media_protocol.rs`; frontend untouched, so biome/tsc/vitest unaffected).

**Residual risks.** (1) The iOS 8 MiB clamp is compile-verified but its runtime behavior (large-video seeking stays bounded, no jetsam kill) is confirmed on-device in Story 12.6, not host-tested — inherent to the cfg-gated const. (2) WebKit scheme-task-invalidation tolerance rests on wry's fire-and-forget responder; on-device confirmation also folds into 12.6. (3) The base whole-file in-RAM load (up to the 25 MB media bar) remains — disk-backed streaming stays deferred under DW-29.
