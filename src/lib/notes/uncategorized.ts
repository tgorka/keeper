/**
 * The synthetic complement space, beside All notes (`./all-spaces.ts`).
 *
 * File-backed spaces are markdown notes somebody wrote. This row is composed
 * on demand from their queries — their negation — so it has nothing on disk
 * to open, and the rail draws no pencil or bin beside it. All notes is the
 * other synthetic row: it selects the existing unscoped list.
 *
 * The value is Rust's. `UNCATEGORIZED_SPACE_ID` in
 * `src-tauri/crates/keeper/src/notes_ipc.rs` is what the wire actually carries,
 * and `uncategorized-id.test.ts` fails if this copy drifts from it — the two
 * halves are in different languages and nothing but that test connects them.
 */
export const UNCATEGORIZED_SPACE_ID = "keeper:uncategorized";
