/**
 * Export-surface open/preset state + live-job mirror (Story 5.5, FR-35, UX-DR11).
 *
 * A vanilla zustand store created at module load *outside* React so the Export
 * affordances (conversation header, search results) can open the single Export
 * dialog with a scope preset from anywhere without prop-drilling. It holds the
 * dialog open state, the scope preset the dialog seeds from, and a mirror of the
 * currently-running job's streamed {@link ExportProgressVm} state (phase, counts,
 * output paths, error). The job state is a pure mirror of the Rust progress stream
 * — never a source of truth; the archive on disk is authoritative.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { ExportPhase, ExportProgressVm, ExportScopeKind } from "@/lib/ipc/client";

/**
 * The scope the Export dialog opens preset to. `chat` carries the open Chat's
 * `(accountId, roomId)`; `account` carries just the `accountId`; `everything`
 * carries neither. The user can still widen/narrow inside the dialog.
 */
export interface ExportPreset {
  scope: ExportScopeKind;
  accountId: string | null;
  roomId: string | null;
}

/** The mirrored live-job state, or `null` when no export is running/finished. */
export interface ExportJobState {
  /** The backend job id (the cancel handle). */
  exportId: number;
  /** The job's current phase (`running` until exactly one terminal batch). */
  phase: ExportPhase;
  /** Logical messages written so far. */
  messagesWritten: number;
  /** Total logical messages in scope, or `null` before it is known. */
  totalMessages: number | null;
  /** Media items copied so far (best-effort). */
  mediaCopied: number;
  /** Media items skipped (unresolvable / uncached / no session). */
  mediaSkipped: number;
  /** The written output file paths, populated on the `completed` batch. */
  outputPaths: string[];
  /** A human failure description on the `failed` batch, else `null`. */
  error: string | null;
}

export interface ExportState {
  /** Whether the Export dialog is open. */
  isOpen: boolean;
  /** The scope preset the dialog seeds from (meaningful only while open). */
  preset: ExportPreset;
  /** The currently-running/finished job mirror, or `null` when idle. */
  job: ExportJobState | null;
  /** Open the dialog with the given preset; clears any prior finished-job state. */
  open: (preset: ExportPreset) => void;
  /** Close the dialog. Leaves an in-flight job running (it keeps streaming). */
  close: () => void;
  /** Record the just-started job's id and reset the mirror to a fresh `running`. */
  startJob: (exportId: number) => void;
  /** Fold a streamed progress batch into the job mirror. */
  applyProgress: (vm: ExportProgressVm) => void;
  /** Clear the job mirror (e.g. after acknowledging a terminal state). */
  clearJob: () => void;
}

/** The default preset (global everything) used before any affordance opens it. */
const DEFAULT_PRESET: ExportPreset = { scope: "everything", accountId: null, roomId: null };

/** The vanilla store instance, created once at module load and shared app-wide. */
export const exportStore = createStore<ExportState>()((set) => ({
  isOpen: false,
  preset: DEFAULT_PRESET,
  job: null,
  open: (preset) => set({ isOpen: true, preset, job: null }),
  close: () => set({ isOpen: false }),
  startJob: (exportId) =>
    set({
      job: {
        exportId,
        phase: "running",
        messagesWritten: 0,
        totalMessages: null,
        mediaCopied: 0,
        mediaSkipped: 0,
        outputPaths: [],
        error: null,
      },
    }),
  applyProgress: (vm) =>
    set((state) => {
      // Ignore a batch for a stale job (a newer export superseded this one).
      if (state.job === null || state.job.exportId !== vm.exportId) {
        return state;
      }
      return {
        job: {
          exportId: vm.exportId,
          phase: vm.phase,
          messagesWritten: vm.messagesWritten,
          totalMessages: vm.totalMessages,
          mediaCopied: vm.mediaCopied,
          mediaSkipped: vm.mediaSkipped,
          outputPaths: vm.outputPaths,
          error: vm.error,
        },
      };
    }),
  clearJob: () => set({ job: null }),
}));

/**
 * React selector hook over {@link exportStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useExportStore<T>(selector: (state: ExportState) => T): T {
  return useStore(exportStore, selector);
}
