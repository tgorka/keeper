/**
 * Live session-list ownership for the active root (Phase 7, FR-234 list half).
 *
 * The `use-notes-changes` shape at zone scale: one read effect (re-run on root
 * change), one event effect (`keeper://sessions-changed` → re-read). The event
 * carries only the root id and the handler re-reads through the command — at
 * tens of rows a wholesale read IS the cheap path, and a payload the UI trusts
 * is a payload that can drift (AD-114).
 */
import { useEffect } from "react";
import { listenSessionsChanged, sessionsList } from "@/lib/ipc/client";
import { sessionsListStore } from "@/lib/stores/sessions-list";
import { refreshSessionsRoots } from "@/lib/stores/sessions-roots";

/** Read one root's rows into the mirror, stale-guarded by the caller. */
async function readRows(rootId: string, isLive: () => boolean): Promise<void> {
  try {
    const rows = await sessionsList(rootId);
    if (isLive()) {
      sessionsListStore.getState().reset(rootId, rows);
    }
  } catch (error) {
    if (isLive()) {
      sessionsListStore.getState().setError(error instanceof Error ? error.message : String(error));
    }
  }
}

/**
 * Keep `sessionsListStore` mirroring `rootId`'s rows, live. Mount one per
 * board. `null` clears the mirror (no root flagged, or none picked yet).
 */
export function useSessionsChanges(rootId: string | null): void {
  // The read effect: a root switch replaces the mirror, and a stale read from
  // the previous root can never paint over the new one.
  useEffect(() => {
    if (rootId === null) {
      sessionsListStore.getState().clear();
      return;
    }
    let live = true;
    void readRows(rootId, () => live);
    return () => {
      live = false;
    };
  }, [rootId]);

  // The event effect: one listener per mounted board. A change for another
  // root still refreshes the roots mirror (its counts moved) but leaves this
  // root's rows alone.
  useEffect(() => {
    if (rootId === null) {
      return;
    }
    let live = true;
    let unlisten: (() => void) | null = null;
    void listenSessionsChanged((changedRootId) => {
      if (!live) {
        return;
      }
      void refreshSessionsRoots();
      if (changedRootId === rootId) {
        void readRows(rootId, () => live);
      }
    }).then((stop) => {
      if (live) {
        unlisten = stop;
      } else {
        stop();
      }
    });
    return () => {
      live = false;
      unlisten?.();
    };
  }, [rootId]);
}
