/**
 * The one space in the rail with no note behind it.
 *
 * Every other space is a markdown file somebody wrote, so it can be renamed,
 * re-queried and deleted. This one is composed on demand from all the others —
 * the negation of every space's query — which means there is nothing on disk to
 * open, and the rail draws no pencil and no bin beside it.
 *
 * The value is Rust's. `UNCATEGORIZED_SPACE_ID` in
 * `src-tauri/crates/keeper/src/notes_ipc.rs` is what the wire actually carries,
 * and `uncategorized-id.test.ts` fails if this copy drifts from it — the two
 * halves are in different languages and nothing but that test connects them.
 */
export const UNCATEGORIZED_SPACE_ID = "keeper:uncategorized";
