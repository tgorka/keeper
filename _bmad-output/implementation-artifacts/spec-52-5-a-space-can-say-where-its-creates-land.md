# Spec 52.5 — A space can say where its creates land

story: 52.5
status: review
branch: `work/epic-52-space-create-dir` (on top of `work/epic-52-spaces-first`)
baseline_revision: c873fa6
final_revision: ''
binds: FR-309; AD-120 (kind is a tag, never a folder), AD-65
sentinel: `MUT52-5`

<intent-contract>

**The ask, verbatim.** *"sessions pliki md stworzone w spaces wciaz sa w glownym
folderze zamiast w folderze spaces"*.

**What is actually true today.** No space has a directory. A space is a saved query
over tags (`spaces.rs:180`), `_spaces/` at the ZONE root holds the query
DEFINITIONS and never session content (`spaces.rs:31-45`), and `kind_dir`
(`shape.rs:262-283`) picks a destination from the session's SHAPE — `Flat =>
None`, i.e. the session root. His `Test5` is flat, so every create lands at the
root by contract, and `sessions_file_new_kind` is never even told which space was
pressed.

**The carve-out, and why it does not break AD-120.** A space MAY name a directory
that its creates go into. Reading is untouched: kind still comes from the tag, and
nothing infers a kind from a folder. That is the asymmetry `spec-50-1:57` already
states — "the directory is where a create *puts* a file; the tag is what makes it
that kind". The tag is still written into the new file's frontmatter, so a file
created into `logs/` is a log because it says so, not because of where it sits.

**Always**
- A space's create destination is a field on the space, edited in the space editor,
  empty by default.
- Empty means today's behaviour exactly: `kind_dir`'s answer, unchanged.
- Rust composes the path (AD-65). The create verb learns WHICH space asked.
- The directory is created if absent, with the same journaled `MkDir` the folder
  verb uses — one plan, one journal row.
- A file created into a space's directory still carries its kind tag in
  frontmatter, and still appears in that space because the QUERY matched the tag.

**Block if**
- The destination escapes the session (`..`, absolute, a symlink out): refused by
  the existing path guard, with its sentence.
- The destination names `workspace/` — scratch that dies with the session — or a
  dotted directory the scan never reads. Refuse and say which.

**Never**
- Never derive a kind from the directory on READ. AD-120 stands.
- Never write outside the session root.
- Never move existing files when a destination is set: it governs creates only.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `keeper-core/src/sessions/spaces.rs` | `SessionSpace` gains the destination; parsed from the definition file, defaulted empty |
| `keeper-core/src/sessions/shape.rs:262-283` | `kind_dir` gains the space's override as an argument, keeping the shape answer as the fallback |
| `keeper/src/sessions_ipc.rs:3243-3249,3306-3315` | `sessions_file_new_kind` takes the space id and composes `rel` from the override when there is one |
| `src/components/sessions/session-space-editor.tsx` | the destination field, with the refusals rendered inline |
| `src/components/sessions/session-spaces.tsx` | the create verb passes its space's id |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | a space with no destination creates exactly where it does today — byte-identical plan |
| 2 | a space with `logs` creates `logs/<stamp>-<slug>.md` inside the session, and the directory is made if absent |
| 3 | the created file carries its kind tag in frontmatter and the space lists it |
| 4 | a destination that escapes the session is refused with the path sentence |
| 5 | `workspace` and a dotted directory are refused, each naming itself |
| 6 | reading still derives kind from the tag: a file in `logs/` tagged `[ref]` is a ref, not a log |
| 7 | the editor round-trips the field, and clearing it restores the default |

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
