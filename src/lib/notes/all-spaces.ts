/** The synthetic unscoped rail row; mirrors notes_ipc::ALL_SPACE_ID. */
export const ALL_SPACE_ID = "keeper:all";

/** The synthetic group holding temporary spaces; mirrors `rail::TEMPORARY_SPACE_ID`. */
export const TEMPORARY_SPACE_ID = "keeper:temporary";

/**
 * Prefix of a synthetic group row materialised for a `/` segment that no space
 * file occupies — `Journal` when only `Journal/Bali` exists. Mirrors
 * `rail::GROUP_SPACE_PREFIX`; a row carrying it folds, it never scopes.
 */
export const GROUP_SPACE_PREFIX = "keeper:group:";
