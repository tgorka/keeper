/**
 * Spaces mirror store (AD-9, AD-20, Story 4.5).
 *
 * A vanilla zustand store created at module load *outside* React. It holds the
 * aggregated {@link SpaceVm} list streamed from the Rust `keeper-core::inbox`
 * merge's fifth (spaces) channel, plus the single active Space selection. The
 * list is a pure mirror of the Rust-authoritative {@link SpacesSnapshot} (a whole
 * snapshot replaces it wholesale — no diff protocol, since Spaces are few).
 *
 * The `activeSpace` selection is **ephemeral view state** (no persistence): it
 * identifies the selected Space by `(accountId, spaceId)`, and the actual inbox
 * filtering happens in Rust (poked via {@link setSpaceFilter}). This store only
 * mirrors the selection so the sidebar can render the active row and the chat
 * list its dismissible chip; it never derives, sorts, or filters inbox membership.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { SpacesSnapshot, SpaceVm } from "@/lib/ipc/client";

/** The active Space selection, identified by `(accountId, spaceId)`. */
export interface SpaceSelection {
  accountId: string;
  spaceId: string;
}

export interface SpacesState {
  /** The aggregated Space list, exactly as Rust streamed it (account-id order). */
  spaces: SpaceVm[];
  /** The active Space filter selection, or `null` when the inbox is unfiltered. */
  activeSpace: SpaceSelection | null;
  /** Replace the Space list from a streamed whole snapshot. */
  applySnapshot: (snapshot: SpacesSnapshot) => void;
  /** Set (or clear) the active Space selection. Does NOT poke the Rust filter — the
   *  caller pairs this with {@link setSpaceFilter}. */
  setActiveSpace: (selection: SpaceSelection | null) => void;
  /** Reset to the empty state (on unsubscribe / full sign-out). */
  clear: () => void;
}

/**
 * The vanilla store instance. Created once at module load, shared across the app;
 * the source of truth for the Space list stays in Rust.
 */
export const spacesStore = createStore<SpacesState>()((set) => ({
  spaces: [],
  activeSpace: null,
  applySnapshot: (snapshot) => set({ spaces: snapshot.spaces }),
  setActiveSpace: (selection) => set({ activeSpace: selection }),
  clear: () => set({ spaces: [], activeSpace: null }),
}));

/**
 * React selector hook over {@link spacesStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useSpacesStore<T>(selector: (state: SpacesState) => T): T {
  return useStore(spacesStore, selector);
}
