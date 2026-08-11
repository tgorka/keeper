/**
 * What happens when somebody presses a link in a note (Story 45.18, FR-196).
 *
 * **Both halves were dead until this story.** `.cm-lp-wikilink` and
 * `.cm-lp-link` have been `cursor: pointer` since 37.6; the wikilink half
 * reached `NoteEditor.onFollowLink`, a prop no caller ever passed, and the
 * external half reached nothing at all. Two hosts mount the decoration layer —
 * the note editor and `viewers/markdown-preview.ts`, which draws a `.md` file
 * opened from the Files pane — so the following lives here rather than in
 * either of them, and a fix to one is a fix to both.
 *
 * Neither function navigates. Each answers "what does this link point at, and
 * may keeper act on it", and hands the answer back as a note or as a finished
 * sentence. Where the note opens is the host's decision, because a note panel
 * and a Files panel are different places to put it.
 */
import { openUrl } from "@tauri-apps/plugin-opener";
import { type NoteRefVm, notesResolveLink } from "@/lib/ipc/client";

/**
 * The URL schemes keeper is permitted to hand to the OS.
 *
 * **This list is not a policy of ours; it is a mirror of one.** The opener
 * plugin's `allow-default-urls` permission — the set `opener:default` grants in
 * `capabilities/default.json` — allows exactly `http://*`, `https://*`,
 * `mailto:*` and `tel:*`. Tauri refuses everything else, so `javascript:`,
 * `file:` and `data:` are already denied at the boundary that can actually
 * enforce it.
 *
 * Checking here as well is not a second policy: it is the difference between a
 * refusal the reader can understand and a rejected promise carrying the
 * plugin's own words about a scope they have never seen. A note is
 * agent-writable, so a `javascript:` destination in one is a thing that can
 * genuinely happen, and being told which scheme was refused is the whole of
 * what makes it not frightening.
 *
 * A trailing colon on each, because that is what the URL grammar has and what
 * the comparison below produces.
 */
const OPENABLE_SCHEMES: readonly string[] = ["http:", "https:", "mailto:", "tel:"];

/** How a destination's scheme is spelled: letters, then `+`, `-`, `.` or a
 *  digit, then a colon (RFC 3986 §3.1). A destination with no scheme is not a
 *  URL, and this returns null for it rather than guessing one. */
const SCHEME = /^([a-z][a-z0-9+\-.]*):/i;

/** What a link keeper will not hand to the OS says. Names the scheme, because
 *  "keeper cannot open this" without saying what it is reads as a fault in
 *  keeper rather than a fact about the link. */
export function unopenableSchemeSentence(url: string): string {
  const scheme = SCHEME.exec(url)?.[1];
  return scheme === undefined
    ? `keeper follows web links and wikilinks from a note; ${url} is neither.`
    : `keeper will not open a ${scheme.toLowerCase()}: link from a note. Web, mail and telephone links open in the app that owns them.`;
}

/** What a link the OS refused says. Rust's — really the plugin's — own words
 *  are appended rather than replaced: a refusal we did not predict is one the
 *  reader should see verbatim. */
export function openerRefusedSentence(url: string, detail: string): string {
  return `keeper could not hand ${url} to your browser: ${detail}`;
}

/** What a wikilink pointing at nothing says. Names the target, because that is
 *  the whole of what the reader has to correct. */
export function noSuchNoteSentence(target: string): string {
  return `No note in this vault answers to ${target}. Nothing has been written there yet.`;
}

/** Either the note a wikilink names, or the sentence saying why there is none.
 *  Exactly one is non-null. */
export interface LinkFollowResult {
  readonly note: NoteRefVm | null;
  readonly reason: string | null;
}

/**
 * The note this wikilink target names.
 *
 * One IPC call to `notes_resolve_link`, which is the index's own resolver — the
 * same one the backlink map is built from. Deliberately not a filter over
 * `notes_link_targets`: that is a substring search for a completion popup, and
 * a second definition of "what names this note" would open one note while the
 * link kept showing in another's backlinks, with nothing in the UI able to
 * explain it (`keeper-core/src/notes/index.rs` says so in as many words).
 *
 * An empty vault id means the host has no vault to resolve against — a `.md`
 * file opened from a plain sync profile. Refused before the call rather than
 * after it, so the reader gets a sentence about their file rather than Rust's
 * sentence about a vault id that was never real.
 */
export async function resolveWikilink(vaultId: string, target: string): Promise<LinkFollowResult> {
  if (vaultId === "") {
    return {
      note: null,
      reason: `This file is not inside a notes vault, so keeper cannot look up ${target}.`,
    };
  }
  let found: NoteRefVm | null;
  try {
    found = await notesResolveLink(vaultId, target);
  } catch (raw) {
    const detail = raw instanceof Error ? raw.message : String(raw);
    return { note: null, reason: `keeper could not look up ${target}: ${detail}` };
  }
  return found === null
    ? { note: null, reason: noSuchNoteSentence(target) }
    : { note: found, reason: null };
}

/**
 * Hand an ordinary link's destination to the application that owns it.
 *
 * Resolves to `null` when it went, and to a finished sentence when it did not,
 * matching `TextFileFrame`'s `refusal` shape: the host renders it where the
 * person pressed rather than in a toast that has gone by the time they look up.
 *
 * **Rejections are expected, not exceptional, and this is why they get a
 * sentence rather than a `.catch(() => {})`.** `capabilities/quick-capture.json`
 * grants no opener at all, so the identical press in a capture window — which
 * Story 45.14 is about to fill with this same editor — is refused by Tauri. A
 * silent failure there would be the same defect this story exists to remove,
 * one window along.
 */
export async function followExternalUrl(url: string): Promise<string | null> {
  const scheme = SCHEME.exec(url)?.[0]?.toLowerCase();
  if (scheme === undefined || !OPENABLE_SCHEMES.includes(scheme)) {
    return unopenableSchemeSentence(url);
  }
  try {
    await openUrl(url);
    return null;
  } catch (raw) {
    return openerRefusedSentence(url, raw instanceof Error ? raw.message : String(raw));
  }
}
