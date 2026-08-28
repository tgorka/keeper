# Spec 50.4 — A file's own properties

story: 50.4
status: in-progress
branch: `feat/50-4-a-files-own-properties` (on top of 50.3)
binds: FR-283; FR-233; AD-65, the notes three-tier frontmatter contract, AD-120
sentinel: `MUT50-4`

<intent-contract>

**Why this exists.** With 50.3 a session markdown file can be written like a note. It still cannot be
*described* like one: the properties panel — frontmatter and tags — is mounted only by the note editor
(`note-editor.tsx:1039-1045`), and its write path is addressed by a note subscription and a vault id,
neither of which a session file has. And tags are not decoration here: **AD-120 makes the tag the thing
that decides which space lists a file** (`pool.rs:253`), so on a sessions zone the properties panel is
the surface that files a file.

That is also the honest answer to the owner's count of zero: their live `README.md` carries no
frontmatter at all (`migrate.rs:446-462` is the fixture of exactly that shape), so no space selects it
— and today there is no way to give it one from inside keeper.

**Approach.** One byte-preserving frontmatter write addressed by `(profileId, relativePath)`, and the
existing panel mounted over it.

**Always.**
- The **existing** frontmatter reader and writer do the work: `notes::frontmatter` parse and the
  byte-preserving writer the notes side already uses. A second frontmatter writer is how two surfaces
  start disagreeing about a file's own bytes.
- The three-tier ownership contract is unchanged: keeper-owned keys, Obsidian-native keys, and the
  person's own — the panel already knows these rules; only its address changes.
- Byte-preserving means byte-preserving: a file with CRLF, no trailing newline, or a body containing
  `---` comes back with exactly those bytes outside the block that changed.
- The write refuses `workspace/` structurally, as every file write there already does (AD-113).
- After a tag write the surface that showed the file re-reads, so a file that just became `tag:ref`
  appears in References without a manual refresh.

**Block if.**
- The file is not markdown → no panel. A CSV has no frontmatter.
- The path resolves outside the profile, or into `workspace/` → refused with the existing sentence.
- The file changed on disk since it was read → the write refuses rather than clobbering, using the
  guarded-write primitive the plan vocabulary already has (`plan.rs:36-43`) or the notes side's
  equivalent. A properties panel that silently overwrites an agent's edit is worse than one that
  refuses.

**Never.**
- Never write a kind tag the user did not ask for. A file's kind is what its frontmatter says; the
  panel is where a person says it, and nothing infers it from where the file sits.
- Never stamp an `id:` into a file keeper did not author — the sessions contract is explicit
  (`docs/sessions.md`), and the unstable-identity caveat exists because of it.
- Never mount the panel where the buffer is read-only.

**I/O and edge-case matrix.** Every row is a test.

| # | input | expected |
|---|---|---|
| 1 | a session markdown file with no frontmatter | the panel offers an empty property set and can add the first key, producing a well-formed block above the body |
| 2 | the owner's live `README.md` shape (no frontmatter, body starts with `# …`) | after adding `tags: [about]`, the body is byte-identical below the new block |
| 3 | a file with CRLF line endings | endings preserved outside the block |
| 4 | a file whose body contains a `---` line | only the leading block is treated as frontmatter |
| 5 | a file with an existing block, one key edited | every other key, its order and its formatting survive |
| 6 | adding `tag:ref` to a file in a folder-shaped session's `refs/` | the References space lists it on the next read |
| 7 | the same on a file at the session root of a folder-shaped session | it does NOT appear — the pool does not read root markdown there (this is 50.1's territory and the test states the boundary) |
| 8 | a `workspace/` file | no panel |
| 9 | a CSV | no panel |
| 10 | the file changed on disk since it was read | the write refuses, says so, and the panel offers to re-read |
| 11 | a note on a note target | unchanged — the notes suite passes untouched |
| 12 | a keeper-owned key | the panel's existing tier rules apply, unchanged |

</intent-contract>

## Code Map

### Rust

| file | change |
|---|---|
| `keeper/src/notes_ipc.rs` or `sync_ipc.rs` (decide by ownership: the write is over a sync profile path, not a vault) | `file_frontmatter_get(profile_id, rel)` / `file_frontmatter_set(profile_id, rel, …)` — reusing `keeper_core::notes::frontmatter` for parse and the byte-preserving writer, and the existing file-write refusal for `workspace/` (`keeper-sync/src/files_write.rs:370-375`). Return the same VM the notes properties panel already consumes, so the panel does not fork |
| `keeper/src/lib.rs` | register both |
| tests | rows 1–5, 10, 12 as Rust tests over real bytes — this is a byte-preservation story and jsdom cannot see bytes |

### TypeScript

| file | change |
|---|---|
| `src/lib/ipc/client.ts` | the two wrappers |
| `src/components/notes/properties-panel.tsx` | takes its read/write as injected functions (or a small adapter) so one panel serves a note and a file. **No second panel** |
| `src/components/viewers/text-file-frame.tsx` | mounts the panel for a writable markdown file, beside where 50.3 put the toolbar |
| `src/components/sessions/session-spaces.tsx` / `session-detail.tsx` | the re-read after a tag write, so row 6 is true without a manual refresh |
| tests | rows 6–9, 11 |

## Tasks & Acceptance

- [ ] the two commands, byte-preserving, guarded, registered, with Rust tests over real bytes
- [ ] one properties panel, two addresses
- [ ] mounted for writable markdown files only
- [ ] the re-read that makes a newly tagged file appear in its space
- [ ] rows 1–12 covered; the notes suite untouched
- [ ] `docs/sessions.md`: how a file gets filed, and that a folder-shaped session's root markdown is
      not in the pool

**Acceptance.** The owner can open the `README.md` of a live session, add `tags: [about]` in the
properties panel, and watch it appear in the About space — with every other byte of the file unchanged.

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
