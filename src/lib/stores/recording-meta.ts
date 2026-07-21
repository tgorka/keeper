/**
 * The ephemeral next-session metadata store (Story 21.5).
 *
 * Holds the optional Title / Participants / Note the user types before Start.
 * The values describe exactly ONE session: Start consumes and clears them, but
 * the consumed values are kept as `last` so a follow-up session can re-fill
 * with one click (recording the same standing meeting twice in a row is the
 * common case). Session-scoped only — never persisted, never uploaded; the
 * values land solely in the local session manifest.
 */
import { create } from "zustand";

export interface RecordingMetaFields {
  /** Optional human title (also drives the session folder name). */
  title: string;
  /** Optional "who is this with" free text. */
  participants: string;
  /** Optional program/session note. */
  note: string;
}

const EMPTY: RecordingMetaFields = { title: "", participants: "", note: "" };

interface RecordingMetaState {
  /** The fields describing the NEXT session (cleared by `consume`). */
  fields: RecordingMetaFields;
  /** The previous session's consumed fields, for the one-click re-fill. */
  last: RecordingMetaFields | null;
  /** Patch one or more fields. */
  setFields: (patch: Partial<RecordingMetaFields>) => void;
  /** Take the fields for a starting session: clears the form, remembers `last`. */
  consume: () => RecordingMetaFields;
  /** Re-fill the form from the previous session's values. */
  refillLast: () => void;
}

export const recordingMetaStore = create<RecordingMetaState>((set, get) => ({
  fields: EMPTY,
  last: null,
  setFields: (patch) => {
    set((state) => ({ fields: { ...state.fields, ...patch } }));
  },
  consume: () => {
    const taken = get().fields;
    const hasAny =
      taken.title.trim() !== "" || taken.participants.trim() !== "" || taken.note.trim() !== "";
    set({ fields: EMPTY, last: hasAny ? taken : get().last });
    return taken;
  },
  refillLast: () => {
    const last = get().last;
    if (last !== null) {
      set({ fields: last });
    }
  },
}));

export const useRecordingMeta = recordingMetaStore;

/** Imperative click-time read used by the Start/Restart paths: consumes the
 * fields (clearing the form) and maps empties to `undefined` so an untouched
 * form ships NO meta wire fields at all. */
export function consumeRecordingMeta(): {
  title?: string;
  participants?: string;
  note?: string;
} {
  const taken = recordingMetaStore.getState().consume();
  const clean = (value: string) => {
    const trimmed = value.trim();
    return trimmed === "" ? undefined : trimmed;
  };
  return {
    title: clean(taken.title),
    participants: clean(taken.participants),
    note: clean(taken.note),
  };
}
