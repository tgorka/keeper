# Spec 52.2 — A rename the open pane follows

story: 52.2
status: in-progress
branch: `work/epic-52-rename-follows` (on top of `work/epic-52-readme-record`)
baseline_revision: c873fa6
final_revision: ''
binds: FR-302; AD-65 (Rust composes every path), AD-107
sentinel: `MUT52-2`

<intent-contract>

**The ask, verbatim.** *"zmiana title property zmienia nazwe teraz ale tez
wyswietla '<path> is no longer in tgdrive. It was moved, renamed or deleted
outside keeper.' zamiast przeladowac plik z nowa nazwa"*

**What actually happens.** The rename succeeds. `sessions_file_rename` returns the
new subpath — `Ok(new_subpath)` at `sessions_ipc.rs:3541`, documented at
`:3419-3420` as existing precisely "so the caller re-addresses its panel without
joining a path (AD-65)". `properties-panel.tsx:945-949` does
`.then(() => { onWritten(); })` and drops it. `onWritten` re-reads the OLD
address, `browse::resolve` answers `None`, and `missing_sentence`
(`sync_ipc.rs:1849-1857`) words the miss with the profile's name — "tgdrive".

**Always**
- After a rename, the open pane shows the SAME file at its new name. No red
  banner, no empty pane, no second click.
- The new address comes from Rust's return value. TypeScript joins nothing
  (AD-65).
- A rename from the space row menu re-points an open pane the same way.
- A host that supplies no `onRenamed` keeps today's behaviour exactly, so no
  caller changes by accident.

**Block if**
- The file has no profile (`file.profileId === null`): there is no panel target to
  re-point, so the existing guard stands and the old path is not re-read either.

**Never**
- Never reconstruct the new path in TypeScript from the old one plus the title.
- Never swallow the rename's failure: a refused rename still reports its sentence.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `src/components/notes/properties-panel.tsx:844-857,945-949` | new `onRenamed?: (next: string) => void`; the rename arm calls it with the resolved subpath instead of discarding it |
| `src/components/viewers/text-file-frame.tsx:191,391-395` | forwards `onPropertiesRenamed` |
| `src/components/viewers/text-file-viewer.tsx:194-197` | re-points via `panelsStore.getState().setActiveTarget({ kind: "file", profileId, relativePath: next })` — the gesture already used at `session-detail.tsx:337` |
| `src/components/sessions/space-row-menu.tsx:242-245` | same one-line change for the row-menu rename |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | renaming through the title property leaves the pane showing the file at its new name; the missing-file sentence never renders |
| 2 | the panel target's `relativePath` is the exact string Rust returned — asserted against a mock that returns a path TypeScript could not have composed |
| 3 | a rename from the space row menu re-points an open pane |
| 4 | a host with no `onRenamed` still calls `onWritten` exactly once |
| 5 | a file with `profileId === null` renames without a re-point and without a stale read |
| 6 | `file-properties.test.tsx:352-359` — which mocks the new path and asserts only that *someone was told* — is strengthened to assert WHERE |

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
