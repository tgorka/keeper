/**
 * A note knows its file, a file knows its note (Story 45.18, FR-196, UX-DR79).
 *
 * Every surface imports from here — `@/lib/vault-link` — rather than from a
 * file inside it, matching `@/lib/viewers`, so the split between the mirrored
 * rule and the two actions it makes possible can move without a rename reaching
 * every call site. See `rule.ts` for why a rule authored in Rust also runs
 * here, and `actions.ts` for why both actions open beside rather than replace.
 */

export {
  FILE_HAS_NO_NOTE_SENTENCE,
  noteNotIndexedSentence,
  OPEN_IN_NOTES_LABEL,
  openNoteForFile,
  SHOW_IN_FILES_LABEL,
  showNoteInFiles,
} from "./actions";
export {
  filePathForNote,
  notePathForFile,
  type ProfileFilePath,
  type VaultFilePath,
  type VaultLocation,
} from "./rule";
