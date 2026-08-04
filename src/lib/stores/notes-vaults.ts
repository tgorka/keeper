/**
 * Notes vault mirror store (Epic 37, Story 37.1, FR-94/FR-95, AD-54).
 *
 * A vault is not a configured object: it is a notes-flagged sync profile plus a
 * subfolder, so the vault list IS a filter over the profile list and there is
 * nothing here to persist. This store mirrors two Rust-owned facts and writes
 * one of them back — the hydrate-and-write-back shape `sync.ts` established:
 *
 *   - `vaults`: the {@link NoteVaultVm} list, re-read after any write.
 *   - `activeVaultId`: which vault everything vault-scoped resolves against.
 *
 * The active vault lives in Rust rather than here, and that is load-bearing
 * rather than tidiness. The tray's New Note / Today's Journal / recent slots and
 * the capture window's commit all run with no main window open, and they resolve
 * their vault through the same stored id. A second selection held only in this
 * store would let the tray write into a different vault than the one on screen.
 * So `setActiveVault` tells Rust first and mirrors the answer; this store never
 * decides.
 *
 * Switching is a FILTER, never a navigation (UX-DR41): nothing in here unmounts
 * a view, clears the open note, or triggers a load state over the frame. The
 * note in the editor survives a vault switch whenever it belongs to the vault
 * being switched to, which is a decision the pane makes from the note's own
 * vault id — not something this store can take away.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import {
  type NoteVaultVm,
  notesVaultActive,
  notesVaultSetActive,
  notesVaults,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

/** Last-resort message when a rejection carries no readable one. */
export const NOTES_UNKNOWN_ERROR = "keeper could not read this vault.";

export interface NotesVaultsState {
  /**
   * The mirrored vaults, or `null` before the first successful read. `null`
   * means "unknown", never "none": the pane renders its unknown state against
   * it rather than claiming a vault list nobody has read yet — the difference
   * between "you have no vault" and "keeper has not looked" is the difference
   * between an invitation and a lie.
   */
  vaults: NoteVaultVm[] | null;
  /**
   * The active vault's id as Rust holds it, or `null` when nothing is selected
   * or the stored id no longer names a flagged profile.
   */
  activeVaultId: string | null;
  /** Whether a first read has landed; the boolean twin of `vaults !== null`. */
  hydrated: boolean;
  /** The last read failure's message, cleared by the next successful read. */
  error: string | null;
  /**
   * A monotonic nonce bumped by `⌘⌥V` and the palette's Switch Vault action.
   * The switcher's open/closed state belongs to the `DropdownMenu` that renders
   * it, so rather than lift that out — and risk two components disagreeing about
   * whether a menu is open — the switcher subscribes to this and opens itself on
   * each bump. The same shape `chat-list-focus.ts` uses for the summon hotkey.
   */
  switcherNonce: number;
  /** Replace the mirrored vault list and mark the mirror hydrated. */
  setVaults: (vaults: NoteVaultVm[]) => void;
  /** Mirror Rust's active-vault answer. */
  setActiveVaultId: (vaultId: string | null) => void;
  /** Record (or clear) the last read failure. */
  setError: (error: string | null) => void;
  /** Ask the vault switcher to open its menu. */
  requestSwitcherOpen: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const notesVaultsStore = createStore<NotesVaultsState>()((set) => ({
  vaults: null,
  activeVaultId: null,
  hydrated: false,
  error: null,
  switcherNonce: 0,
  setVaults: (vaults) =>
    set((state) => ({
      vaults,
      hydrated: true,
      // A vault that lost its notes flag must not stay active. Rust enforces the
      // same rule on its own read, but the list can arrive first, and an active
      // id pointing at a vault no longer in the list would render a switcher
      // naming a vault the user cannot see.
      activeVaultId:
        state.activeVaultId !== null && vaults.some((vault) => vault.id === state.activeVaultId)
          ? state.activeVaultId
          : null,
    })),
  setActiveVaultId: (activeVaultId) => set({ activeVaultId }),
  setError: (error) => set({ error }),
  requestSwitcherOpen: () => set((state) => ({ switcherNonce: state.switcherNonce + 1 })),
}));

/** In-flight hydration, deduped so concurrent surfaces trigger one read. */
let hydration: Promise<void> | null = null;

/**
 * Switch the active vault (FR-95).
 *
 * Rust is told first and the mirror follows, so the tray and this window can
 * never disagree about which vault a write lands in. A rejected switch leaves
 * the previous selection in place rather than half-applying one — a switcher
 * showing a vault the backend did not switch to is the worst of both.
 */
export async function setActiveVault(vaultId: string): Promise<void> {
  try {
    await notesVaultSetActive(vaultId);
    const state = notesVaultsStore.getState();
    state.setActiveVaultId(vaultId);
    state.setError(null);
  } catch (raw) {
    notesVaultsStore.getState().setError(syncErrorMessage(raw, NOTES_UNKNOWN_ERROR));
  }
}

/** Read both halves of the mirror in one round trip. */
async function loadSnapshot(): Promise<void> {
  const [vaults, active] = await Promise.all([notesVaults(), notesVaultActive()]);
  const state = notesVaultsStore.getState();
  // Order matters: `setVaults` drops an active id that is not in the list, so
  // the id has to land after the list it is validated against.
  state.setVaults(vaults);
  state.setError(null);
  if (active !== null && vaults.some((vault) => vault.id === active)) {
    state.setActiveVaultId(active);
    return;
  }
  // One vault and no stored selection is not a choice, it is a missing step —
  // and it is the common case the first time a folder is flagged. Route it
  // through Rust rather than setting the mirror directly, so the tray resolves
  // the same vault this window is about to show.
  const first = vaults[0];
  if (first !== undefined) {
    await setActiveVault(first.id);
  }
}

/**
 * Lazily hydrate the mirror (once per app lifetime; concurrent callers share one
 * read). Best-effort: a read failure leaves the mirror unhydrated, records the
 * message, and nulls the shared promise so the next call retries.
 */
export async function ensureNotesVaultsHydrated(): Promise<void> {
  if (notesVaultsStore.getState().hydrated) {
    return;
  }
  hydration ??= loadSnapshot().catch((raw: unknown) => {
    notesVaultsStore.getState().setError(syncErrorMessage(raw, NOTES_UNKNOWN_ERROR));
    // Allow a later retry rather than caching the failure forever.
    hydration = null;
  });
  await hydration;
}

/**
 * Re-read the vault list after a write elsewhere — a folder flagged or unflagged
 * in Settings, a vault's settings saved. Never throws and never blanks: a failed
 * read keeps the previous list, so a transient IPC fault cannot flicker a
 * working vault into the "no vault yet" invitation.
 */
export async function refreshNoteVaults(): Promise<void> {
  try {
    await loadSnapshot();
  } catch (raw) {
    notesVaultsStore.getState().setError(syncErrorMessage(raw, NOTES_UNKNOWN_ERROR));
  }
}

/**
 * React selector hook over {@link notesVaultsStore}. Pass a selector to
 * subscribe to just the slice a component needs.
 */
export function useNotesVaultsStore<T>(selector: (state: NotesVaultsState) => T): T {
  return useStore(notesVaultsStore, selector);
}

/** The active vault's view model, or `null` when none is selected. */
export function useActiveVault(): NoteVaultVm | null {
  return useNotesVaultsStore((s) => s.vaults?.find((v) => v.id === s.activeVaultId) ?? null);
}

/** Test-only reset: clear the mirror and forget any in-flight hydration. */
export function resetNotesVaultsStoreForTest(): void {
  notesVaultsStore.setState({
    vaults: null,
    activeVaultId: null,
    hydrated: false,
    error: null,
  });
  hydration = null;
}
