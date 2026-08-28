/**
 * The sessions key surface (Phase 7, FR-251): ⌘7 switches to the board, and
 * ⌘⌥L logs today in the first active session of the active root.
 *
 * The `use-notes-shortcut` shape verbatim: one window listener, self-gated on
 * the sessions capability from the Rust-authored mirror (never a platform
 * sniff), typing targets skipped, and the ⌥ chord matched on `event.code`
 * (the physical key — `event.key` under Alt is layout-dependent).
 */
import { useEffect } from "react";
import { SESSION_RECORD_NAME } from "@/components/sessions/session-detail";
import { sessionsLogToday } from "@/lib/ipc/client";
import { capabilitiesStore } from "@/lib/stores/capabilities";
import { panelsStore } from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { sessionsListStore } from "@/lib/stores/sessions-list";
import { sessionsRootsStore } from "@/lib/stores/sessions-roots";

function isTypingTarget(target: EventTarget | null): boolean {
  const element = target as HTMLElement | null;
  if (element === null) {
    return false;
  }
  return (
    element.isContentEditable ||
    element.tagName === "INPUT" ||
    element.tagName === "TEXTAREA" ||
    element.tagName === "SELECT"
  );
}

/** The ⌘⌥L verb, shared with the palette's sessions-log-today handler. */
export async function logTodayInCurrentSession(): Promise<void> {
  const rootId = sessionsRootsStore.getState().activeRootId;
  const rows = sessionsListStore.getState().rows;
  // FR-242's fallback until the sticky choice ships: the first active row,
  // which the board sorts most-recently-touched first.
  const target = rows?.find((row) => row.status === "active");
  if (rootId === null || target === undefined) {
    return;
  }
  const ref = await sessionsLogToday(rootId, target.id);
  const subfolder =
    sessionsRootsStore.getState().roots?.find((root) => root.id === rootId)?.subfolder ??
    "60-sessions";
  // The record, by the one name the detail and the board both read. The string it
  // resolves to is the `README.md` this line already composed, so ⌘⌥L opens
  // exactly what it opened before Story 52.1: what the constant buys is a single
  // reader of the record's name, not a fix. A session whose record has not been
  // moved yet still has no `README.md` to open, and moving it is
  // `sessions_record_migrate`'s job — offered on the session detail.
  panelsStore.getState().setActiveTarget({
    kind: "file",
    profileId: rootId,
    relativePath: `${subfolder}/${ref.path}/${SESSION_RECORD_NAME}`,
  });
}

export function useSessionsShortcut(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (!mod) {
        return;
      }
      // No dead sessions chord where no zone can exist (FR-223).
      if (!capabilitiesStore.getState().capabilities.sessions) {
        return;
      }
      if (isTypingTarget(event.target)) {
        return;
      }
      if (!event.altKey) {
        // ⌘7 — the board. Ctrl+7 is the non-mac twin, matching ⌘1–⌘6.
        if (event.key === "7") {
          event.preventDefault();
          primaryViewStore.getState().setView("sessions");
        }
        return;
      }
      if (event.code === "KeyL") {
        event.preventDefault();
        primaryViewStore.getState().setView("sessions");
        void logTodayInCurrentSession();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
