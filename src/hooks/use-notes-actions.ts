/**
 * The notes verbs, in one place (Epic 37, FR-98/FR-99/FR-113/FR-119, UX-DR42).
 *
 * Every notes action is reachable from at least three surfaces — a row, a key,
 * the palette, sometimes the tray — and UX-DR42's whole point is that they
 * cannot word or behave differently in any of them. So the verbs are module
 * functions here rather than callbacks inside a component: `actions.ts` calls
 * them for the palette (and therefore the ⌘? cheat sheet and the native menu),
 * `use-notes-shortcut.ts` calls them for the ⌘⌥ cluster, and the list rows call
 * them through {@link useNotesActions}, which is a thin binding to the active
 * vault and nothing more.
 *
 * **One verb is deliberately not in that binding: delete** (Story 45.17).
 * {@link deleteNote} is here, and it is still the only place a note is
 * removed — but it is reached through `NoteDeleteDialog` from each of its
 * three surfaces rather than through {@link useNotesActions}, because it is
 * the one act that must be confirmed and a bound verb that trashes a note on
 * call would be a second path around the confirmation. The rule above still
 * holds for it: one function, one wording, one behaviour, in every surface.
 *
 * They **reject to their caller** rather than swallowing, following the rule
 * `sync.ts` documents: a read failure belongs to the mirror, an action failure
 * belongs to the surface that asked for it, because only that surface knows
 * where the sentence goes. Nothing here catches.
 *
 * Nothing here holds state either. A verb that changes a note changes it in
 * Rust; the row that shows the change arrives back over the changes stream, so
 * there is no optimistic overlay to get out of step.
 */
import { useCallback } from "react";
import type { NoteCreateVm, NoteRefVm, NoteRowVm, NoteSpaceFieldVm } from "@/lib/ipc/client";
import {
  notesCaptureShow,
  notesCreate,
  notesDelete,
  notesJournalToday,
  notesMarkRead,
  notesReveal,
  notesSetFlag,
  notesSpaceSave,
} from "@/lib/ipc/client";
import { openCaptureWindow } from "@/lib/stores/capture-windows";
import type { TagChip } from "@/lib/stores/notes-filters";
import { noteQueryFor, notesFiltersStore } from "@/lib/stores/notes-filters";
import { notesListStore } from "@/lib/stores/notes-list";
import { notesVaultsStore } from "@/lib/stores/notes-vaults";
import { panelsStore } from "@/lib/stores/panels";

/**
 * The vault a verb acts on, or `null` when none is active.
 *
 * Read from the mirror at call time rather than closed over, because a verb can
 * be dispatched from the palette or a global key long after the closure that
 * would have captured it was built.
 */
function activeVaultId(): string | null {
  return notesVaultsStore.getState().activeVaultId;
}

/**
 * Create an empty note, put the cursor on it, and report anything the person
 * who asked has to be told (FR-98, FR-160, Story 44.6).
 *
 * No title, no folder, no template: the destination is a rule and not a
 * question, because the user has not written the words yet and so cannot answer
 * one (UX-DR35). The first line becomes the title afterwards.
 *
 * `spaceId` is the space the ask came from — a space row's own New Note — and
 * `null` is the rail's, which creates into the default list. It is the space's
 * **id** and nothing else: Rust reads that space's query and derives the tags,
 * folder and flags the new note needs, so a note created in a space is selected
 * by it. Working that out here would put a second copy of the space DSL in
 * TypeScript, and the second copy is always the one that is wrong.
 *
 * Selecting the note is what puts the caret in its body: the editor mounts on
 * the selected note and focuses itself, and the buffer it focuses is the body
 * alone — the frontmatter block never enters it.
 *
 * Resolves with `null` when no vault is flagged — the caller sends the user to
 * Settings → Sync rather than reporting a failure, because there is nothing
 * broken, only nothing configured.
 */
export async function createNote(spaceId: string | null = null): Promise<NoteCreateVm | null> {
  const vaultId = activeVaultId();
  if (vaultId === null) {
    return null;
  }
  const created = await notesCreate(vaultId, {
    title: null,
    body: null,
    template: null,
    dest: null,
    tags: [],
    space: spaceId,
  });
  panelsStore.getState().setActiveTarget({ kind: "note", vaultId, noteId: created.note.id });
  return created;
}

/** Open today's journal entry, creating it from the template if needed (FR-99). */
export async function openJournalToday(): Promise<NoteRefVm | null> {
  const vaultId = activeVaultId();
  if (vaultId === null) {
    return null;
  }
  const ref = await notesJournalToday(vaultId);
  panelsStore.getState().setActiveTarget({ kind: "note", vaultId, noteId: ref.id });
  return ref;
}

/** Show the quick-capture panel — the in-app twin of the global hotkey. */
export async function showCapture(): Promise<void> {
  await notesCaptureShow();
}

/**
 * Open a note as a capture window (Story 45.15, FR-191).
 *
 * The sentence the story exists for: **the small window is a way of looking at
 * a note, not a special kind of note.** A capture window opened on an existing
 * note is the same editor over the same file the Notes pane shows — nothing is
 * copied, nothing is converted, and closing it changes nothing.
 *
 * Idempotent by identity in Rust: asking twice raises the window that already
 * holds this note rather than making a second one.
 */
export async function openNoteAsCapture(vaultId: string, noteId: string): Promise<void> {
  await openCaptureWindow({ kind: "note", vaultId, noteId });
}

/** Pin or unpin a note (FR-119). Rewrites one frontmatter key and nothing else. */
export async function togglePin(vaultId: string, row: NoteRowVm): Promise<void> {
  await notesSetFlag(vaultId, row.id, "pinned", !row.pinned);
}

/**
 * Archive or unarchive a note (FR-119). Removes it from the default lens without
 * deleting anything — the file on disk is untouched apart from the one key.
 */
export async function toggleArchive(vaultId: string, row: NoteRowVm): Promise<void> {
  await notesSetFlag(vaultId, row.id, "archived", !row.archived);
}

/**
 * Acknowledge a note's changes, clearing its unread mark and its share of the
 * tray dot (FR-113).
 *
 * The revision acknowledged is exactly the one the row is unread AGAINST, not
 * "whatever is head now": a change landing between the render and the press has
 * to stay unread, or the mark would clear things the user never saw.
 *
 * Two rows are left alone rather than round-tripped. A row that is already read
 * has nothing to acknowledge — and nothing to un-acknowledge either, because
 * `notes_mark_read` is the whole of the read-state surface and there is no
 * mark-unread twin to call. A row with an empty `headRev` has never been
 * committed, so it has no revision and, by construction, no unread mark.
 */
export async function markNoteRead(vaultId: string, row: NoteRowVm): Promise<void> {
  if (!row.unread || row.headRev === "") {
    return;
  }
  await notesMarkRead(vaultId, row.id, row.headRev);
}

/**
 * Move a note to the vault's trash (NFR-30). Never an unlink.
 *
 * **Every delete in the app goes through here**, which is why it takes an id
 * rather than a row (Story 45.17): the confirmation dialog holds an id, a space
 * row holds an id, and a `NoteRowVm` parameter would have forced two of the
 * three callers to either build a fake row or call `notesDelete` directly —
 * and the one that called directly would be the one that skipped the panel
 * close below.
 */
export async function deleteNote(vaultId: string, noteId: string): Promise<void> {
  await notesDelete(vaultId, noteId);
  // A panel showing a note the user just deleted stops showing it. This is the
  // one case that is NOT "the target no longer resolves, so say so and keep the
  // place": the note is not missing, it was thrown away on purpose, and a pane
  // explaining its absence would be keeper reporting the user's own action back
  // to them as a fault.
  panelsStore.getState().closeTarget({ kind: "note", vaultId, noteId });
}

/** Reveal a note's real path in the OS file manager (UX-DR38). */
export async function revealNote(vaultId: string, row: NoteRowVm): Promise<void> {
  await notesReveal(vaultId, row.id);
}

/**
 * Write the active chip set as a space note under `spaces/` (FR-105, UX-DR37).
 *
 * A filter you can build but not keep trains people not to build filters, and a
 * space is an ordinary markdown note, so the organisation syncs, diffs and is
 * agent-editable like everything else. The query is composed from the same
 * function the list itself uses, so a saved space reproduces exactly the set
 * that was on screen when it was saved.
 */
export async function saveFilterAsSpace(name: string): Promise<NoteRefVm | null> {
  const vaultId = activeVaultId();
  if (vaultId === null) {
    return null;
  }
  const { limit } = notesListStore.getState();
  const filters = notesFiltersStore.getState();
  const query = noteQueryFor(filters, 0, limit);
  return await notesSpaceSave(vaultId, {
    id: null,
    name,
    // The space note carries the query as text, because that is what an agent or
    // Obsidian will read and edit. Rust composes it from the same request the
    // list ran, so the two can never drift into different result sets.
    query: spaceQueryText({
      tags: filters.tagTerms,
      flags: query.flags,
      origin: query.origin,
      text: query.text,
    }),
    sort: "modified desc",
    limit,
    icon: null,
    // Unpositioned, so a space made from the filter bar lands in the rail's
    // alphabetical block rather than jumping above spaces somebody placed by
    // hand. There is nowhere on this path to ask, and guessing a number would
    // be keeper deciding the shape of a rail it was not asked about.
    order: 0,
    // No template either, and for the same reason: this path saves the filter
    // that is on screen, and a template is not part of a filter. The space
    // editor is where one is chosen (Story 44.7).
    template: null,
  });
}

/**
 * Render a chip set into the one-line DSL a space note stores.
 *
 * The grammar is `keeper_core::notes::query`'s: juxtaposition is AND, `-`
 * negates, and a bareword is sugar for `text:`. Quoting the free-text term is
 * what keeps a two-word search from becoming two AND-ed terms.
 *
 * An excluded chip becomes `-tag:x` (Story 43.3). That is the whole reason the
 * three-state chip needed no new grammar: `-` has negated a term since FR-105,
 * so a space saved from the bar and a space typed by hand in Obsidian are the
 * same text, and the chip is a face for what the DSL already said.
 *
 * Exported because the space editor (Story 43.4) writes the same text when it
 * saves an edited space, and two writers of one grammar is how a space saved
 * from the bar and a space saved from the editor start disagreeing.
 *
 * Tags arrive as an ordered list rather than as {@link NoteQueryReq}'s map: an
 * object's key order puts integer-like keys first, so a vault with a `2026` tag
 * would silently reorder its own saved query.
 */
export function spaceQueryText(parts: {
  tags: readonly TagChip[];
  flags: readonly string[];
  origin: string | null;
  text: string | null;
  /**
   * `field:key=value` terms, as Rust decomposed them. Optional because the
   * filter bar has no field control and never writes one — it saves what is on
   * screen, and a `field:` term can only arrive from a space that already had
   * it. The editor, which round-trips a stored query, always passes them.
   */
  fields?: readonly NoteSpaceFieldVm[];
}): string {
  const terms: string[] = [];
  for (const { tag, term } of parts.tags) {
    terms.push(term === "exclude" ? `-tag:${tag}` : `tag:${tag}`);
  }
  for (const flag of parts.flags) {
    terms.push(`is:${flag}`);
  }
  if (parts.origin !== null) {
    terms.push(`origin:${parts.origin}`);
  }
  for (const { key, op, value } of parts.fields ?? []) {
    // No quoting: `op` is one of the two Rust admitted, and both `key` and
    // `value` came back trimmed from a query that already parsed. Re-quoting a
    // value the tokenizer handed over unquoted would change the bytes of a
    // space that was only opened and saved (FR-121).
    terms.push(`field:${key}${op}${value}`);
  }
  if (parts.text !== null) {
    terms.push(`text:"${parts.text.replace(/"/g, '\\"')}"`);
  }
  return terms.join(" ");
}

/** The verbs bound to the active vault, for the surfaces that render rows. */
export interface NotesActions {
  /** Pin or unpin the row. */
  pin: (row: NoteRowVm) => Promise<void>;
  /** Archive or unarchive the row. */
  archive: (row: NoteRowVm) => Promise<void>;
  /** Acknowledge the row's changes; a no-op on a read or uncommitted row. */
  markRead: (row: NoteRowVm) => Promise<void>;
  /** Reveal the row's real path. */
  reveal: (row: NoteRowVm) => Promise<void>;
}

/**
 * Bind the row verbs to one vault. A no-vault binding resolves every verb
 * immediately: there is no row to act on in that state, so the alternative would
 * be a nullable action object every caller has to unwrap.
 */
export function useNotesActions(vaultId: string | null): NotesActions {
  const pin = useCallback(
    async (row: NoteRowVm) => {
      if (vaultId !== null) {
        await togglePin(vaultId, row);
      }
    },
    [vaultId],
  );
  const archive = useCallback(
    async (row: NoteRowVm) => {
      if (vaultId !== null) {
        await toggleArchive(vaultId, row);
      }
    },
    [vaultId],
  );
  const markRead = useCallback(
    async (row: NoteRowVm) => {
      if (vaultId !== null) {
        await markNoteRead(vaultId, row);
      }
    },
    [vaultId],
  );
  // No `remove` verb. Deleting is the one act in this file that must be
  // confirmed (Story 45.17), and a bound verb that trashes a note on call
  // would be a second path around the confirmation — reachable, destructive
  // and one keystroke from any surface that took this object. The three real
  // entry points all mount `NoteDeleteDialog`, which calls `deleteNote` above
  // after a person has pressed Delete in a dialog naming the note.
  const reveal = useCallback(
    async (row: NoteRowVm) => {
      if (vaultId !== null) {
        await revealNote(vaultId, row);
      }
    },
    [vaultId],
  );
  return { pin, archive, markRead, reveal };
}
