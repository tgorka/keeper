/**
 * The sessions-roots mirror (Phase 7, FR-224, AD-107).
 *
 * Hydrate-and-write-back, the `notes-vaults.ts` shape: two Rust facts —
 * the root list and the active root id — and nothing decided locally. The
 * active root is UI state (which root the board shows), not a Rust-owned
 * choice like the active vault: nothing outside the board (no tray, no
 * capture window) reads it this phase, so a cookie-free local field is the
 * honest scope. `roots === null` means "not read yet", never "no roots".
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { type SessionRootVm, sessionsRoots } from "@/lib/ipc/client";

export interface SessionsRootsState {
  /** The mirrored roots, or `null` before the first successful read. */
  roots: SessionRootVm[] | null;
  /** The root the board currently shows, or `null` before one is picked. */
  activeRootId: string | null;
  /** Human-readable read failure, or `null`. */
  error: string | null;
  /** Replace the mirror wholesale from a served read. */
  setRoots: (roots: SessionRootVm[]) => void;
  /** Switch which root the board shows. */
  setActiveRootId: (rootId: string | null) => void;
  setError: (error: string | null) => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const sessionsRootsStore = createStore<SessionsRootsState>()((set) => ({
  roots: null,
  activeRootId: null,
  error: null,
  setRoots: (roots) =>
    set((state) => ({
      roots,
      error: null,
      // Keep the active choice when it survives the new set; fall to the
      // first root otherwise — a board with roots always shows one, and a
      // board whose root was unflagged says so by moving, not by blanking.
      activeRootId:
        state.activeRootId !== null && roots.some((root) => root.id === state.activeRootId)
          ? state.activeRootId
          : (roots[0]?.id ?? null),
    })),
  setActiveRootId: (activeRootId) => set({ activeRootId }),
  setError: (error) => set({ error }),
}));

export function useSessionsRootsStore<T>(selector: (state: SessionsRootsState) => T): T {
  return useStore(sessionsRootsStore, selector);
}

/** The active root's VM, or `null` while unread/empty. */
export function useActiveSessionsRoot(): SessionRootVm | null {
  return useSessionsRootsStore(
    (state) => state.roots?.find((root) => root.id === state.activeRootId) ?? null,
  );
}

/** Re-read the mirror. Never blanks on failure — the stale set stays up. */
export async function refreshSessionsRoots(): Promise<void> {
  try {
    sessionsRootsStore.getState().setRoots(await sessionsRoots());
  } catch (error) {
    sessionsRootsStore.getState().setError(error instanceof Error ? error.message : String(error));
  }
}

/** Test-only reset. */
export function resetSessionsRootsStoreForTest(): void {
  sessionsRootsStore.setState({ roots: null, activeRootId: null, error: null });
}
