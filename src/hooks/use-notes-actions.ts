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
import type { NoteRefVm, NoteRowVm } from "@/lib/ipc/client";
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
import { noteQueryFor, notesFiltersStore } from "@/lib/stores/notes-filters";
import { notesListStore } from "@/lib/stores/notes-list";
import { notesVaultsStore } from "@/lib/stores/notes-vaults";

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
 * Create an empty note and put the cursor on it (FR-98).
 *
 * No title, no folder, no template: the destination is a rule and not a
 * question, because the user has not written the words yet and so cannot answer
 * one (UX-DR35). The first line becomes the title afterwards.
 *
 * Resolves with `null` when no vault is flagged — the caller sends the user to
 * Settings → Sync rather than reporting a failure, because there is nothing
 * broken, only nothing configured.
 */
export async function createNote(): Promise<NoteRefVm | null> {
  const vaultId = activeVaultId();
  if (vaultId === null) {
    return null;
  }
  const ref = await notesCreate(vaultId, {
    title: null,
    body: null,
    template: null,
    dest: null,
    tags: [],
  });
  notesListStore.getState().select(vaultId, ref.id);
  return ref;
}

/** Open today's journal entry, creating it from the template if needed (FR-99). */
export async function openJournalToday(): Promise<NoteRefVm | null> {
  const vaultId = activeVaultId();
  if (vaultId === null) {
    return null;
  }
  const ref = await notesJournalToday(vaultId);
  notesListStore.getState().select(vaultId, ref.id);
  return ref;
}

/** Show the quick-capture panel — the in-app twin of the global hotkey. */
export async function showCapture(): Promise<void> {
  await notesCaptureShow();
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

/** Move a note to the vault's trash (NFR-30). Never an unlink. */
export async function deleteNote(vaultId: string, row: NoteRowVm): Promise<void> {
  await notesDelete(vaultId, row.id);
  if (notesListStore.getState().selected?.noteId === row.id) {
    notesListStore.getState().clearSelection();
  }
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
  const query = noteQueryFor(notesFiltersStore.getState(), 0, limit);
  return await notesSpaceSave(vaultId, {
    id: null,
    name,
    // The space note carries the query as text, because that is what an agent or
    // Obsidian will read and edit. Rust composes it from the same request the
    // list ran, so the two can never drift into different result sets.
    query: spaceQueryText(query),
    sort: "modified desc",
    limit,
  });
}

/**
 * Render a composed query back into the one-line DSL a space note stores.
 *
 * The grammar is `keeper_core::notes::query`'s: juxtaposition is AND, `-`
 * negates, and a bareword is sugar for `text:`. Quoting the free-text term is
 * what keeps a two-word search from becoming two AND-ed terms.
 */
function spaceQueryText(query: {
  text: string | null;
  tags: string[];
  origin: string | null;
  flags: string[];
}): string {
  const terms: string[] = [];
  for (const tag of query.tags) {
    terms.push(`tag:${tag}`);
  }
  for (const flag of query.flags) {
    terms.push(`is:${flag}`);
  }
  if (query.origin !== null) {
    terms.push(`origin:${query.origin}`);
  }
  if (query.text !== null) {
    terms.push(`text:"${query.text.replace(/"/g, '\\"')}"`);
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
  /** Trash the row's note. */
  remove: (row: NoteRowVm) => Promise<void>;
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
  const remove = useCallback(
    async (row: NoteRowVm) => {
      if (vaultId !== null) {
        await deleteNote(vaultId, row);
      }
    },
    [vaultId],
  );
  const reveal = useCallback(
    async (row: NoteRowVm) => {
      if (vaultId !== null) {
        await revealNote(vaultId, row);
      }
    },
    [vaultId],
  );
  return { pin, archive, markRead, remove, reveal };
}
