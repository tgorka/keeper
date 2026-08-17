# Spec 49.3 — Spaces fold, and the fold has a default you can set

story: 49.3
status: implemented; reviewed twice and re-gated; the two new commands are owed to hesperia
branch: `feat/49-3-spaces-fold-by-default` (on top of `feat/49-2-a-space-you-can-write-into`)
binds: FR-275, FR-276; DW-172 (a fold's hydration is invisible to its store's own tests),
Story 48.1 (the fold/width interaction rules), Story 47.3 (`FoldSection`)
sentinel: `MUT49-3`

<intent-contract>

**The ask, verbatim.** *"spaces w sessions … moga byc foldable (ustaw w opcjach ze maja byc
folder/unfolder by default)"*

**Problem.** `SpaceSection` renders a bordered div whose rows are always visible
(`session-spaces.tsx:313-461`): no chevron, no `folded` prop, no store, and no import that could
supply one. The only fold-shaped state anywhere in Sessions is `session-tree.tsx:194` — a local
`useState<Set<string>>` that forgets itself on every remount, which is the exact defect
`notes-rail-fold.ts:10-13` records against the old Files tree. And there is no settings key for any
fold default in the whole app: `notesRailUnfolded()` (`notes-rail-fold.ts:74-76`) and
`columnsUnfolded()` (`column-fold.ts:53-55`) are hardcoded, reasoned defaults, not preferences.

**Two decisions, not one.** The mechanism (a fold that is remembered) and the default (what an
unremembered space does) are separate, and the second one is the one the owner named:
*"ustaw w opcjach"*. `favorites_collapsed` (`config/keys.rs:374-382`) shipped the mechanism alone and
left `keeper.toml` as the only default knob — no Settings control at all. That is not what was asked
for, so this story builds both halves and composes them:

> **a space with no remembered fold follows `sessions.spaces_folded`; a space the person folded or
> unfolded by hand keeps their answer.**

**Approach.** Two persistence planes, each used for what it is for. The per-space fold is chrome the
person arranged → `document.cookie`, TS-only, never IPC (`fold-cookie.ts:29-33`). The default is a
preference a config file may set → the `settings` k/v table through the eight-hop pipeline.

**Always.**
- `FoldSection` (`sidebar-group.tsx:40-171`) is the mechanism. It owns the disclosure, the
  `aria-label` verb, `aria-expanded`, `aria-controls` and the `hidden` body. No second copy.
- The space's own header controls (New note, Edit, Delete) go in `actions` — **outside** the folded
  region, per `sidebar-group.tsx:20-25`: "a header can carry a control the folded rows are the way to
  reach".
- A **fourth** cookie namespace, `keeper_session_spaces_fold`. Never a key inside
  `NOTES_RAIL_GROUPS`: `fold-cookie.ts:15-19` names this exact collision — the chat sidebar has a
  group called `spaces`, the notes rail's first section is called Spaces, and a session's spaces are
  a third thing with the same name. A shared namespace would fold two surfaces at once.
- An **open** key set, copied from `files-tree.ts` (`:64,81,93,109,273-292,340-353`), because spaces
  are user-created: a bounded entry count, a byte budget, and eviction — a cookie over the browser's
  limit is dropped whole and silently.
- Hydration is idempotent behind a module-level `let hydrated`, and it is called at the **mount
  point** (`session-detail.tsx`), with the restore test living there too. DW-172: the store's own
  suite cannot see a missing `hydrate…` call, and 48.1's mutation M3 measured exactly that.
- `sessions.spaces_folded` is registered in `config::keys::KEYS` and `docs/settings-keys.md` is
  **regenerated**, not edited: `cargo test -p keeper-core --lib config::keys::tests::docs::regenerate
  -- --ignored`.

**Block if.**
- The `Shape` in the `KeySpec` disagrees with the literal the registry getter compares against →
  the setting silently does the opposite of what a config file says (`config/mod.rs:394-399`). This
  story uses `Shape::Flag01` and `== Some("1")`, matching `system.menu_bar_presence`
  (`registry.rs:697-714`, `keys.rs:651-656`).
- The Rust command pair is not in `generate_handler!` → the whole TS suite still passes and the app
  fails at runtime with "command not found" (`lib.rs:838-839` is the hop with no TS coverage).

**Never.**
- Never a fold that unmounts its rows: `hidden`, so the state survives and the section stops claiming
  height (`sidebar-group.tsx:25-30`).
- Never a fold control as a fourth icon button in the space's header row — the title is the toggle.
  At ~208px of card width a fourth control leaves the space name ~36px (Story 49.2's arithmetic).
- Never a Settings section that renders when there is nothing to configure: it removes itself when no
  synced folder is flagged as a sessions root, the way `CaptureSettingsSection` removes itself
  without a vault (`settings-dialog.tsx:210-212`).

**I/O and edge-case matrix.** Every row is a test.

| # | input | expected |
|---|---|---|
| 1 | a space, default setting off, nothing remembered | unfolded; rows visible; `aria-expanded="true"` |
| 2 | the same, `sessions.spaces_folded = true` | folded on arrival; the header, its count and its buttons still there; `aria-expanded="false"` |
| 3 | press the title of a folded space | rows appear, `aria-expanded` flips, the cookie records `false` for that space |
| 4 | fold space A, leave B alone, with the setting ON | A folded (remembered), B folded (default) — and after unfolding B, B stays unfolded across a remount while the setting is still ON |
| 5 | remount the detail with a written cookie | the fold comes back as left — the DW-172 test, at the mount point, not in the store |
| 6 | 40 spaces folded | the cookie stays inside its entry limit and byte budget; the oldest entries are evicted, nothing is corrupted, no exception |
| 7 | a garbage cookie value | ignored; every space falls back to the setting |
| 8 | `New note` / `Restore` pressed while folded | works, and its sentence is readable — `actions` and `notice` are outside the folded region |
| 9 | `sessions.spaces_folded` round trip in `registry.rs` | absent → `false`; `set(true)` → `"1"`; `set(false)` → `"0"` |
| 10 | the Settings switch | loads on open (`undefined` while loading, never a claimed state), sets optimistically, reverts on failure — the `menu_bar_presence` idiom verbatim |
| 11 | flipping the switch with a detail open | already-open spaces keep what they are; only spaces with nothing remembered follow the new value |
| 12 | no sessions root flagged | the Settings section is absent, not empty and not disabled |
| 13 | `docs/settings-keys.md` | contains `sessions.spaces_folded`, and `the_checked_in_table_matches_the_registry` passes |

</intent-contract>

## Code Map

### Rust — the eight hops (copy `system.menu_bar_presence` at every one)

| file:line | change |
|---|---|
| `keeper-core/src/registry.rs:697-714` | `const SESSIONS_SPACES_FOLDED_KEY: &str = "sessions.spaces_folded"`, `get_sessions_spaces_folded` (`== Some("1")`), `set_sessions_spaces_folded` (`"1"`/`"0"`), round-trip test beside `:3774-3783` |
| `keeper-core/src/config/keys.rs` (namespace order) | `KeySpec { key: "sessions.spaces_folded", family: false, scope: Scope::UserGlobal, settable: Settable::AnyLayer, shape: Shape::Flag01, default: "0", … }` |
| `docs/settings-keys.md` | REGENERATED by the ignored `docs::regenerate` test |
| `keeper-core/src/account.rs:3740-3756` | the get/set wrappers over `platform.data_dir()` |
| `keeper/src/ipc.rs:10333-10381` | `sessions_spaces_folded_get` / `_set`, with honest non-desktop behaviour |
| `keeper/src/lib.rs:838-839` | both in `generate_handler!` |
| `src/lib/ipc/client.ts:2657-2668` | `sessionsSpacesFoldedGet()` / `sessionsSpacesFoldedSet(folded)` |

### TypeScript

| file:line | change |
|---|---|
| `src/lib/stores/session-spaces-fold.ts` — new | `SESSION_SPACES_FOLD_COOKIE = "keeper_session_spaces_fold"`, `spaceKey(rootId, spaceId)`, an open `recorded: ReadonlyMap<string, boolean>` + `defaultFolded: boolean`, `isFolded(key)` = recorded ?? default, `setSpaceFolded` (persist then set, with the limit/budget/eviction from `files-tree.ts`), `setDefaultFolded`, `hydrateSessionSpacesFold(cookie, defaultFolded)` behind `let hydrated`, `resetSessionSpacesFoldForTest()` |
| `src/lib/stores/session-spaces-fold.test.ts` — new | rows 6, 7 and the encoder round trip |
| `src/components/sessions/session-spaces.tsx:313-461` | `SpaceSection` renders through `FoldSection`: `label = space.name`, the count and the fault lamp in the header, New note/Edit/Delete in `actions`, the sentence in `notice`, the rows as the body |
| `src/components/sessions/session-detail.tsx` | `hydrateSessionSpacesFold(document.cookie, await sessionsSpacesFoldedGet())` on mount; the mount-point restore test (row 5) lives in `session-detail.test.tsx` |
| `src/components/sessions/sessions-settings.tsx` — new | the Sessions settings section: one `FileControlled settingKey="sessions.spaces_folded"` + `Switch`, self-removing when no root is flagged, modelled on `capture-settings.tsx` |
| `src/components/settings/settings-dialog.tsx:199-242` | render it between Recording (`:215`) and Sync (`:218`) |
| `src/components/sessions/session-spaces.test.tsx` | rows 1–4, 8 |
| `src/components/sessions/sessions-settings.test.tsx` — new | rows 10, 12 |

## Tasks & Acceptance

- [x] the settings key through all eight hops, `docs/settings-keys.md` regenerated, `keys.rs` gates green
- [x] `session-spaces-fold.ts` with the open key set, the limit, the budget and eviction
- [x] `SpaceSection` folds through `FoldSection`, with `actions`/`notice` outside the fold
- [x] hydration at the detail mount, and the DW-172 restore test at the mount point
- [x] the Sessions settings section, self-removing, with load-on-open and revert-on-failure
- [x] matrix rows 1–13 covered; row 5 is the one that catches an unwired mount point
- [x] `docs/sessions.md` + `docs/settings-keys.md` reflect both halves

**Acceptance.** A person can fold any space in a session and find it exactly as they left it after a
restart, and can say once, in Settings, whether spaces they have not touched arrive folded.

## Design Notes

**The two planes, and why this story did not get to pick one.** The fold is chrome the person
arranged, so it is a cookie and never IPC (`fold-cookie.ts:29-33` argues that out loud). The default
is a preference a config file may set, so it is a `settings` row with a `KeySpec`. The owner's ask
needed both, and the composition — `recorded ?? defaultFolded` in exactly one function
(`isSpaceFolded`) — is the whole story. A hand-set `false` must never read as "nothing recorded":
`??` is nullish, the encoder writes `0`/`1`, and the reader rejects anything else, so a present
`false` survives every hop.

**Deviations and decisions.**

1. **An open key set, not a closed one.** The other three fold stores enumerate their sections at
   compile time. Session spaces are files a person creates, so this copies `files-tree.ts`: an entry
   limit of 32, a 2000-byte budget and oldest-first eviction. A cookie over the browser's limit is
   dropped whole and silently, which would lose every fold rather than the thirty-third.
2. **A fourth cookie namespace.** `fold-cookie.ts:15-19` names this exact collision: the chat sidebar
   has a group called `spaces`, the notes rail's first section is called Spaces, and a session's
   spaces are a third thing with that name. One namespace would fold two surfaces at once.
3. **`FoldSection` gained `labelClassName` and a truncating span, additively.** SPACES/TAGS/FILES are
   a register and 11px uppercase is right for them; a space's name is text a person typed. The
   default reproduces the previous classes byte for byte, so the five existing callers (three direct,
   two through `FoldableGroup`) are unchanged, and the `aria-label` still carries the whole name so a
   truncated one is announced whole.
4. **`iconProps` on `FoldSection`, from review.** Moving the glyph into the `icon` prop had dropped
   `data-slot="space-icon"` and `data-space-icon`, and the second one is the STORED icon name — the
   only way to tell a space whose `keeper.space` icon is unknown from one with no icon at all, which
   is why `space-list.test.tsx:256-272` exists. Typed to `data-*` keys only, so a caller cannot pass
   a `className` and fight the sizing `FoldSection` computes.
5. **The fold region id is percent-encoded, from review.** A space's id is its path, and a
   hand-written `_spaces/my tasks.md` made the DOM id `session-space-_spaces/my tasks.md`, which HTML
   parses as TWO IDREFs — so the disclosure controlled nothing for assistive technology while working
   for a pointer. Percent-encoding is reversible, so `my tasks.md` and `my-tasks.md` keep distinct
   ids; a slug would have mapped them onto one.
6. **The cookie is restored synchronously; only the default waits on Rust, from review.** The first
   cut gated `readSessionSpacesFold(document.cookie)` behind `sessionsSpacesFoldedGet()`, so a space
   the person had folded painted OPEN and then snapped shut when the read landed. The hydrate stays
   at the mount point (DW-172) and the async default is applied compare-and-set, so a flip in
   Settings while the read is in flight is not clobbered by it.
7. **The Settings writes are serialised, from review.** `writeId` guarded the revert but not the
   write, and two `invoke`s in flight are unordered — flip on-then-off could leave Rust holding `"1"`
   while the switch and the store showed off, with nothing re-reading. The writes now chain, so the
   last flip is the last write.
8. **A failed read reports the value the app is obeying, from review.** The failure arm hard-coded
   `false`, so a detail already folding untouched spaces could sit under a switch that said "off" —
   enabled, unbadged, and flipping it on a no-op. It now reports the store's own `defaultFolded` and
   still writes nothing, so reporting a fold folds nothing.

## Verification

**Ran here, on Linux, at the tip of the stack (49.1 + 49.2 + 49.3).**

| command | result |
|---|---|
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all` | clean |
| `cargo clippy … -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` | clean |
| `cargo test -p keeper-core` | 11 suites, 0 failed, including `sessions_spaces_folded_defaults_off_and_round_trips` and the three settings-key registry gates |
| `cargo test -p keeper-core --lib config::keys::tests::docs::regenerate -- --ignored` then `git diff docs/settings-keys.md` | the generated row is what the registry emits; `the_checked_in_table_matches_the_registry` passes |
| `bun run typecheck` | clean |
| `bun run lint` | exit 0; 4 warnings, all pre-existing |
| `bun run test` | 275 files, **4199** tests, 0 failed — two consecutive runs |
| `bun run check:design` | clean (358 web + 184 native files) |
| `check:core-tauri-free`, `check:core-sync-free`, `check:syncd-lean` | clean |

**Mutations run, specified by the defect review and performed by the coordinator** (each file
restored afterwards and the restore verified by probing for the mutated text):

| mutation | tests killed |
|---|---|
| `hydrateSessionSpacesFold(document.cookie, …)` → `("", …)` | **3** — the row-5 remount restore and the two `SessionDetail` cases re-anchored to the cookie. This is the DW-172 measurement: the mount point's call is the only thing between a cookie that is written and one that is never read back |
| `setSpacesFoldedDefault` also clearing `recorded` | 1 — *rows 4 and 11: the setting moves an untouched space and never a decided one* |
| the count in `actions` gated on `!folded` | 1 — *row 2: arrives folded when the setting says so, and keeps its whole header* |

**Not run here, and owed to hesperia.** `sessions_spaces_folded_get`/`_set` live in the `keeper`
shell crate, which does not link on this box: they are registered and read, never compiled. Their
first compile and the binding-drift check happen in `bun run check:rust:macos` over the whole stack.

**Manual smoke test on hesperia:** open a session, fold Tasks by its title, quit and relaunch —
Tasks is still folded and the others are not. Turn on Settings → Sessions → *Start spaces folded*:
the spaces you never touched arrive folded, Tasks stays as you left it, and the count, the verbs and
any fault sentence are still there on a folded space. Press *New note* on a folded space and read
what it says without opening it.
