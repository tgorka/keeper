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

export interface RecordingMetaCustomRow {
  /** The field's user-chosen name. */
  name: string;
  /** The field's value. */
  value: string;
}

export interface RecordingMetaFields {
  /** Optional human title (also drives the session folder name). */
  title: string;
  /** Optional "who is this with" free text. */
  participants: string;
  /** Optional program/session note. */
  note: string;
  /**
   * The tags field, exactly as typed (Story 22.3; Story 42.5).
   *
   * Comma-separated by convention, but the convention is Rust's: this string
   * travels to `keeper-core`'s tag module verbatim, which splits it and
   * decides case, whitespace, separators and emptiness in the one place those
   * rules live. Splitting or trimming per-tag here would be a second answer to
   * "what is a tag", which is the whole thing Story 42.5 deletes.
   */
  tags: string;
  /** Repeatable custom name/value rows (Story 22.3). */
  custom: RecordingMetaCustomRow[];
}

const EMPTY: RecordingMetaFields = {
  title: "",
  participants: "",
  note: "",
  tags: "",
  custom: [],
};

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
      taken.title.trim() !== "" ||
      taken.participants.trim() !== "" ||
      taken.note.trim() !== "" ||
      taken.tags.trim() !== "" ||
      taken.custom.some((row) => row.name.trim() !== "");
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

/**
 * The next-session metadata as it crosses the wire: every field optional, an
 * absent one meaning "the form was untouched here, change nothing".
 */
export interface RecordingMetaWire {
  title?: string;
  participants?: string;
  note?: string;
  /**
   * The tags field exactly as typed (Story 42.5) — one string, commas and all.
   * Rust splits and normalises it in the one place that rule lives, so no
   * caller of this may treat it as a list.
   */
  tags?: string;
  custom?: RecordingMetaCustomRow[];
}

/** Imperative click-time read used by the Start/Restart paths: consumes the
 * fields (clearing the form) and maps empties to `undefined` so an untouched
 * form ships NO meta wire fields at all. */
export function consumeRecordingMeta(): RecordingMetaWire {
  const taken = recordingMetaStore.getState().consume();
  const clean = (value: string) => {
    const trimmed = value.trim();
    return trimmed === "" ? undefined : trimmed;
  };
  // Story 42.5: the tag text ships whole. `clean` is the same emptiness rule
  // the other three free-text fields get — an untouched field ships nothing —
  // and NOT a per-tag normalisation: `Client/Acme , acme` leaves here intact.
  const custom = taken.custom
    .map((row) => ({ name: row.name.trim(), value: row.value.trim() }))
    .filter((row) => row.name !== "");
  return {
    title: clean(taken.title),
    participants: clean(taken.participants),
    note: clean(taken.note),
    tags: clean(taken.tags),
    custom: custom.length > 0 ? custom : undefined,
  };
}
