/**
 * The openers a surface may hand a viewer (Story 45.2, AD-65).
 *
 * Separate from `registry.ts` so the table stays free of IPC: an icon lookup
 * in a virtualised Files row must not drag the Tauri client into its import
 * graph, and every test of the table would otherwise have to mock a command it
 * never calls.
 */

import { syncOpenEntry } from "@/lib/ipc/client";

/**
 * The `openWith` thunk for a file inside a sync profile.
 *
 * **Why this exists rather than each surface writing the arrow function.** The
 * choice of command is a containment decision, not a formatting one.
 * `sync_open_entry` takes a profile id and a profile-relative subpath — one
 * this surface was handed by `sync_browse` — so Rust re-resolves it through
 * the same rule the listing used and the command cannot be pointed at an
 * arbitrary location on disk. `recording_open_path`, the other opener in the
 * app, has the recordings destination as its root and would REFUSE a note in a
 * vault (AD-74). Two surfaces picking their own would eventually pick the
 * wrong one, and a refusal reads to a user as a broken button.
 *
 * Best-effort by design: the opener is a leave-keeper action, and a failure to
 * launch a handler is not something the viewer can repair. The caller decides
 * how loud to be; nothing here swallows the rejection on the caller's behalf.
 */
export function openWithForProfileEntry(
  profileId: string,
  relativePath: string,
): () => Promise<void> {
  return () => syncOpenEntry(profileId, relativePath);
}
