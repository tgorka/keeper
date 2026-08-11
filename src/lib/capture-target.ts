/**
 * What a capture window holds, and how it is addressed (Story 45.15, FR-191).
 *
 * **The key is a mirror of Rust's, and that is the whole reason this file is
 * separate from the store that uses it.** `keeper_core::capture::capture_key`
 * builds the same string, and the two never meet at runtime: Rust stores a
 * window's remembered placement and its draft pointer under this key, and the
 * webview asks about them by the same key. They are pinned to each other by
 * `src-tauri/crates/keeper-core/src/capture-key-vectors.json`, which both test
 * suites load — the mechanism `keeper_core::size` and `file-size.ts` already
 * use, and the mechanism `file-asset-url.ts` uses for exactly this hazard: a
 * name with a space in it that agrees in every developer's fixture and
 * disagrees for the first vault a person actually names.
 *
 * A drift here is silent. Nothing throws, nothing renders wrong; the window
 * simply asks about a row that is not there, and every remembered position is
 * forgotten with no error anywhere.
 *
 * **Which window am I?** A capture window is a separate webview with its own
 * document, so it learns its target from its own URL — `capture.html` with no
 * query is the prewarmed draft window, `capture.html?vault=…&note=…` is a
 * window opened on a note. The URL rather than an IPC round trip because the
 * NFR-27 budget is 300 ms from hotkey to focused caret and a command is a round
 * trip that budget does not have.
 */
import type { CaptureTargetVm } from "@/lib/ipc/client";

/**
 * The key of the prewarmed, note-less capture window.
 *
 * Mirrors `keeper_core::capture::DRAFT_CAPTURE_KEY`. Exported because the
 * capture surface's document half addresses its draft with it before it has
 * resolved a note.
 */
export const DRAFT_CAPTURE_KEY = "draft";

/** The query parameter naming the vault a capture window's note lives in. */
export const CAPTURE_VAULT_PARAM = "vault";

/** The query parameter naming the note a capture window holds. */
export const CAPTURE_NOTE_PARAM = "note";

/** The prewarmed window's target, as a value rather than a shape to rebuild. */
export const DRAFT_CAPTURE_TARGET: CaptureTargetVm = { kind: "draft" };

/**
 * The storage and lookup key for a capture target.
 *
 * Both components are `encodeURIComponent`d before being joined, and the reason
 * is not cosmetic: without it a vault called `a` holding a note called `b/c`
 * and a vault called `a/b` holding a note called `c` produce one key, and two
 * different notes then share one window, one draft and one remembered
 * position. Note ids are derived from paths, so a slash in one is ordinary.
 */
export function captureKey(target: CaptureTargetVm): string {
  if (target.kind === "draft") {
    return DRAFT_CAPTURE_KEY;
  }
  return `note:${encodeURIComponent(target.vaultId)}/${encodeURIComponent(target.noteId)}`;
}

/*
 * There is deliberately no `captureSearch` here. **Rust composes a capture
 * window's URL and this file only parses it**, because Rust is the only thing
 * that creates a window — a composer here would be a second spelling of a
 * string with one producer, and the two would agree on every ASCII name and
 * disagree on the first one with a space in it (`URLSearchParams` writes `+`
 * where `keeper_core::capture::capture_search` writes `%20`; both decode the
 * same, which is exactly what makes the drift invisible).
 *
 * The two halves are pinned to each other by `capture-key-vectors.json`, which
 * carries the `search` Rust produces for every target: Rust asserts it composes
 * it and this file asserts it parses back to the target it came from.
 */

/**
 * Read a capture window's target out of its own `location.search`. **Total.**
 *
 * Anything that does not name both a vault and a note is the draft window. Half
 * a target is not half a window: a URL carrying a note and no vault cannot be
 * resolved — a note id is unique only inside its vault — so guessing a vault
 * would open somebody else's note under this note's name. The draft window is
 * the honest answer, and it is the answer that loses nothing, because the
 * prewarmed window resolves a note of its own.
 */
export function captureTargetFromSearch(search: string): CaptureTargetVm {
  const params = new URLSearchParams(search);
  const vaultId = params.get(CAPTURE_VAULT_PARAM) ?? "";
  const noteId = params.get(CAPTURE_NOTE_PARAM) ?? "";
  if (vaultId === "" || noteId === "") {
    return DRAFT_CAPTURE_TARGET;
  }
  return { kind: "note", vaultId, noteId };
}
