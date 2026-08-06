---
title: 'Story 40.1: The Path Template, Rendered Purely'
type: 'feature'
created: '2026-08-05'
status: 'done'
blocking_condition: ''
baseline_revision: '7615e3065968543d746e8fb58e18e39e28198f1d'
final_revision: '520f54bd08a9a8dc26e26e1a52ffd1a6e6de1c96'
review_loop_iteration: 4
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-40-context.md'
warnings: [oversized]
---

<intent-contract>

## Intent

**Problem:** A recording session's folder name is a fixed `keeper-rec <ts>` (or `<title> <ts>`)
string, flat under the destination root — it does not sort, it does not nest, and it is formatted in
the Tauri shell where nothing can test it. Everything epic 40 wants (year nesting, chronological
sort, retitle-as-move) needs that name to become a user-editable relative *path*, rendered by a
function with no clock and no filesystem under it.

**Approach:** Add a pure `keeper-core` module that parses a path template over the shared
`{yyyy}`-style token vocabulary into a validated `PathTemplate`, and renders it against a caller-
supplied `RenderCtx` (civil datetime, optional title, collision ordinal) into a `RelativePath` that
is legal on the APFS ∩ exFAT ∩ NTFS intersection. Nothing calls it yet — stories 40.2 and 40.3 are
its first consumers.

## Boundaries & Constraints

**Always:**
- The module is pure: no clock, no filesystem, no `tauri`, no `keeper-sync`, no new dependency in
  any `Cargo.toml`. Every input arrives through `RenderCtx`.
- Rendering guarantees, unconditionally, for every template that parsed and every title: no `:`
  anywhere; never absolute; no component that is empty, `.` or `..`; no leading/trailing separator;
  no leading/trailing space or `.` in any component; and a title can never introduce more path
  components than the template's own `/` separators.
- **The final component always renders.** A template whose last path component is built
  exclusively from tokens that may collapse (`{title}`, `{slug}`, `{seq}`) is rejected at parse
  with `TemplateError::OptionalLeaf`, naming the component. This is the leaf-scoped sibling of
  `MayRenderEmpty` and exists because the rendered path *is* the session folder: a leaf that
  vanishes would silently promote a parent — the year directory — into a session folder, make the
  collision ordinal rename that parent (`2026`, `2026 (2)`, …), and, with an explicit `{seq}` in a
  collapsible leaf, make session 2 a *child* of session 1. The leaf must therefore contain at
  least one always-rendering token or one literal character. Interior components may still
  collapse and take their separator with them; only the leaf is constrained.
- **The rendered leaf fits the filesystem.** The final component, *including* any collision
  ordinal, is capped at 255 bytes — `NAME_MAX` on APFS, ext4 and exFAT alike. When a title would
  exceed it the title token is truncated at a UTF-8 character boundary (never mid-codepoint) so
  the ordinal always fits; the 80-character cap is a legibility rule, not a byte rule, and cannot
  be one. Without this, the retry loop in 40.3 only ever lengthens a name it can never make legal.
- Validation, not sanitisation: `parse` rejects with a typed reason and never rewrites. `render` is
  infallible — every decision was made at parse time.
- The token table is documented in the module doc, and the doc states in prose that it is the same
  vocabulary `journal_template` publishes (AD-65), naming
  `keeper-core/src/notes/naming.rs::journal_path` as the sibling so the two cannot drift silently.
- Existing `notes` behaviour is unchanged: `slug()`, `note_filename()` and `journal_path()` keep
  their current outputs, guarded by their existing tests.

**Block If:**
- Delivering the rendering guarantees would require a new crate dependency (a slug/unicode/path
  crate). AD-55 and the epic both refuse one.

**Never:**
- No caller: `recording_start`, `SessionParams`, `session_folder_name`, `keeper/src/ipc.rs` and the
  frontend are untouched. Wiring is 40.2 and 40.3.
- No settings key, no IPC surface, no TypeScript bindings, no `ts-rs` derive.
- No migration of existing session folders, and no removal of the shell's current naming path.

## I/O & Edge-Case Matrix

Ctx below = 2026-08-05T14:32:07 unless stated. Template = `DEFAULT_TEMPLATE`
(`{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {slug}`) unless stated.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Default, titled | title `"Standup"`, seq 1 | `2026/2026-08-05 1432 standup` | none |
| Default, untitled | title `None`, seq 1 | `2026/2026-08-05 1432` — collapsed `{slug}` takes the preceding space with it, no trailing space | none |
| Title slugs to nothing | title `"!!!"` | same as untitled: the token collapses with its separator | none |
| Collision, no `{seq}` in template | title `"Standup"`, seq 3 | `2026/2026-08-05 1432 standup (3)` — suffix on the final component only | none |
| Collision, explicit `{seq}` | `{yyyy}-{mm}-{dd} {slug}{seq}`, seq 1 then 2 | `2026-08-05 standup` then `2026-08-05 standup (2)` | none |
| Hostile title | template `{yyyy}/{title} {HH}{MM}`, title `"a/b:c"` | exactly 2 components; separators and `:` are stripped from the title, never rendered | none |
| `{title}` vs `{slug}` | template `{yyyy}-{title}` / `{yyyy}-{slug}`, title `"Café Déjà Vu"` | `2026-Café Déjà Vu` / `2026-cafe-deja-vu` | none |
| Reserved device name | template `{slug}/{yyyy}`, title `"NUL"` | the interior component is suffixed (`nul-rec/2026`), never bare `nul` | none |
| Parse: traversal | `../{yyyy}` | rejected | `TemplateError::ParentComponent` |
| Parse: absolute | `/Users/x/{yyyy}` | rejected | `TemplateError::Absolute` |
| Parse: illegal char | `{HH}:{MM}` | rejected | `TemplateError::IllegalCharacter { ch: ':' }` |
| Parse: unknown token | `{week}` | rejected, reason names the token | `TemplateError::UnknownToken(String)` |
| Parse: unterminated | `{yyyy` | rejected | `TemplateError::Unterminated` |
| Parse: empty / blank | `""`, `"   "` | rejected | `TemplateError::Empty` |
| Parse: nothing guaranteed | `{slug}`, `{slug}/{title}` | rejected — could render to nothing at all | `TemplateError::MayRenderEmpty` |
| Parse: optional leaf | `{yyyy}/{title}`, `{yyyy}/{mm}/{slug}`, `{yyyy}/{slug}{seq}` | rejected — the last component could render to nothing, so the session folder would become its own parent. Precedence: when *every* component is optional the reason stays `MayRenderEmpty`; `OptionalLeaf` names the case where earlier components would have rendered | `TemplateError::OptionalLeaf` |
| Parse: leaf saved by a literal | `{yyyy}/rec-{slug}`, `{yyyy}/{slug} {HH}{MM}` | accepted — the leaf carries something that always renders | none |
| Interior collapse still allowed | `{yyyy}/{slug}/{yyyy}-{mm}-{dd}`, title `None` | `2026/2026-08-05` — an interior component may vanish with its separator | none |
| Long title, byte cap | 80-character Japanese title, seq 9 | the final component is ≤ 255 bytes including ` (9)`, truncated at a character boundary, never mid-codepoint | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/recording/path_template.rs` -- **new**; the whole story.
- `src-tauri/crates/keeper-core/src/recording.rs` -- add `pub mod path_template;` (Rust 2018 puts
  the submodule in the sibling `recording/` directory; the 5 800-line file is **not** moved to
  `mod.rs`). Nothing else in it changes.
- `src-tauri/crates/keeper-core/src/notes/naming.rs` -- `slug()`'s folding loop is extracted to a
  `pub(crate) fn slug_stem(&str) -> String` (no `"untitled"` fallback, no `-note` suffix);
  `slug()` becomes `slug_stem` + those two steps and keeps its exact current behaviour.
  `RESERVED_DEVICE_NAMES` becomes `pub(crate)`. Prior art for the token vocabulary and the
  Windows-legality rules lives here.
- `src-tauri/crates/keeper-core/src/notes/templates.rs` -- read-only precedent: the hand-rolled
  `{{token}}` scanner, and the `Stamp` struct that `RenderCtx`'s civil-datetime fields mirror.
- `keeper/src/ipc.rs:4465-4521` -- read-only: the shell naming this eventually replaces
  (`sanitize_session_title`, the ` (2)` collision loop). Do not edit — that is story 40.3.

## Tasks & Acceptance

**Execution:**
- [x] `keeper-core/src/notes/naming.rs` -- extract `slug_stem` and widen `RESERVED_DEVICE_NAMES` to
      `pub(crate)` -- one slug algorithm in the crate, so recording folders and note filenames can
      never fold a title two different ways.
- [x] `keeper-core/src/recording/path_template.rs` -- module doc with the token table (`{yyyy} {yy}
      {mm} {dd} {HH} {MM} {SS} {title} {slug} {seq}`), stating that `{mm}` is month and `{MM}` is
      minute, and that this is `journal_template`'s vocabulary (AD-65) -- the doc is the contract
      40.2's UI copy is written from.
- [x] `keeper-core/src/recording/path_template.rs` -- `TemplateError` (thiserror, one variant per
      rejection reason in the matrix, lowercase messages naming the offending token/char) and
      `RelativePath` (newtype over `String`; `as_str`, `components()`, `Display`) -- 40.2 renders
      the error text inline, so each reason must stand alone as a sentence. Includes
      `OptionalLeaf`, which names the offending component.
- [x] `keeper-core/src/recording/path_template.rs` -- `PathTemplate::parse`, `DEFAULT_TEMPLATE`,
      `PathTemplate::as_str`, and a `Segment`/`Token` IR -- parse once at edit time, render per
      recording. `parse` enforces the leaf rule: the final component must carry something that
      always renders, with `MayRenderEmpty` taking precedence when *no* component does.
- [x] `keeper-core/src/recording/path_template.rs` -- `RenderCtx` (civil `year/month/day/hour/
      minute/second`, `title: Option<String>`, `seq: u32`) and `PathTemplate::render` implementing
      the collapse, sanitise-title, collision-suffix and 255-byte leaf-cap rules -- the shell owns
      the clock and the retry loop, so both arrive as plain data.
- [x] `keeper-core/src/recording/path_template.rs` -- `parse` refuses `{seq}` anywhere but the final
      component, with its own typed reason naming the offending component -- the ordinal exists to
      make a sibling, and a `{seq}` upstream of the leaf both suppresses the leaf's own ordinal and
      renames an ancestor.
- [x] `keeper-core/src/recording/path_template.rs` -- the 255-byte cap applies to **every** rendered
      component, not only the leaf -- every component becomes a directory that has to be created,
      and 40.3's retry loop only ever varies the leaf.
- [x] `keeper-core/src/recording/path_template.rs` -- `is_illegal` also refuses Unicode format
      characters (category `Cf`), in templates and in titles alike -- a folder whose name is
      invisible, or whose name Finder draws backwards, is not a folder the user can find.
- [x] `keeper-core/src/recording/path_template.rs` -- `parse` refuses a template component that
      render would otherwise silently rewrite: an empty component, and one whose literal text is
      `.` or `..` once surrounding filler is discounted -- validated, never sanitised.
- [x] `keeper-core/src/recording/path_template.rs` -- the collapse rule no longer welds two
      neighbours that both rendered: removal is followed by merging adjacent filler runs into one
      and trimming the component's edges.
- [x] `keeper-core/src/recording/path_template.rs` -- the leaf's collision ordinal is reserved out of
      its byte budget and re-attached after a clamp, whether the ordinal is appended by `render` or
      was rendered in place by an explicit `{seq}` -- a clamp that erases the ordinal makes every
      seq render the same path, and 40.3's retry loop would never terminate.
- [x] `keeper-core/src/recording/path_template.rs` -- the "never rewritten" check applies to a
      component's outer edges wherever they are literal, not only to components holding no token --
      `{yyyy}/..{mm}` and `{yyyy}/{mm} ` are refused, interior separators are untouched.
- [x] `keeper-core/src/recording/path_template.rs` -- a filler run is removed only when a token
      adjacent to it rendered empty, and `renders_something` follows the same exposure rule -- so a
      component's decoration does not depend on whether the recording had a title, and `{yyyy}/-`
      is accepted rather than refused with a reason that is not true of it.
- [x] `keeper-core/src/recording/path_template.rs` -- a component holding no token is decided
      entirely at parse: over 255 bytes, or folding to an MS-DOS device name, each gets its own
      typed reason naming the component -- the clamp and `de_reserve` stay for token-bearing
      components, where the text depends on a title that arrives at record time.
- [x] `keeper-core/src/recording/path_template.rs` -- `(`, `)`, `[` and `]` join the filler set, so a
      parenthesised optional token takes its brackets with it -- an empty `()` is the "Untitled"
      placeholder spelled in punctuation. **Superseded by the bracket-pairing task below**: the
      intent stands, the mechanism does not.
- [x] `keeper-core/src/recording/path_template.rs` -- an optional token counts as collapsed when its
      rendered text is empty once edge noise is discounted -- otherwise a title of `"..."` leaves
      the separator it was meant to take with it.
- [x] `keeper-core/src/recording/path_template.rs` -- brackets are text again; a collapsing token
      takes a *matching pair* with it -- pairing is symmetric, so no rendered name can be unbalanced
      and no neighbour can weld, which a facing rule could not promise.
- [x] `keeper-core/src/recording/path_template.rs` -- the parse-time decision covers a component
      holding no *title-dependent* token (no `{title}`, no `{slug}`), and the leaf's budget reserves
      the widest ordinal -- otherwise `{seq}` and the collision suffix are holes straight through it.
- [x] `keeper-core/src/recording/path_template.rs` -- `#[cfg(test)] mod tests` covering every row of
      the I/O matrix plus the property tests below -- the matrix is the acceptance surface.

**Acceptance Criteria:**
- Given the default template and no `{seq}` in it, when the same title is rendered at seq 1 and
  seq 2, then the two `RelativePath`s differ, differ only in the final component, and both satisfy
  every legality rule.
- Given a deterministic generator (seeded, in-test, no new dependency) of **1 000** titles drawn
  from separators, `:`, control characters, dots, spaces, emoji, CJK, combining marks, reserved
  device names and 300-character strings, when each is rendered against every token-bearing test
  template at seq 1 and seq 7 — and the untitled case (`title: None`) is swept alongside them —
  then every component is non-empty, contains no character from the illegal set, has no
  leading/trailing space or dot, is not `.`/`..`/a reserved device name, the path is relative, the
  final component is at most 255 bytes, and the component count never exceeds the template's own
  separator count + 1.
- Given any template that parses, when it is rendered untitled at seq 1 and again at seq 2, then
  neither rendered path is a path-prefix of the other — a collision ordinal always produces a
  sibling, never a child.
- Given any template that parses, when it is rendered at seq 1 and again at seq 2 for the same
  title, then the two paths have the same number of components and differ only in the last one —
  a collision never changes the depth of the path and never renames a folder above the session.
- Given the property sweep, when each render is inspected, then **every** component — not only the
  final one — is at most 255 bytes.
- Given a template or a title containing a Unicode format character (`U+200B`, `U+FEFF`,
  `U+202E`), when it is parsed or rendered, then the template is refused with
  `TemplateError::IllegalCharacter` and the character never reaches a rendered component.
- Given `{yyyy}//{mm}`, `{yyyy}/  ..  /{mm}`, `{yyyy}/..a`, `{yyyy}/..{mm}` and `{yyyy}/{mm} `, when
  they are parsed, then each is refused with a typed reason rather than silently rendered as
  something else.
- Given a template whose leaf carries an explicit `{seq}` and is long enough to be clamped, when it
  is rendered at seq 1, 2 and 7, then the three paths are pairwise different and each still shows
  its ordinal — the byte cap never erases the only thing that distinguishes two sessions.
- Given a component whose edge filler has no collapsible token beside it, when it is rendered with
  and without a title, then the filler is present in both — a component's decoration does not depend
  on whether the recording was titled.
- Given `{yyyy}/x` followed by 300 literal characters, and given `{yyyy}/nul`, when they are parsed,
  then each is refused with a typed reason naming the component rather than rendered as a name the
  user did not type.
- Given `{yyyy}/{yyyy}-{mm}-{dd} ({title})` and `{yyyy}/{yyyy}-{mm}-{dd} [{slug}]` untitled, when
  they are rendered, then neither leaves an empty bracket pair behind.
- Given any template that parses, when it is rendered with and without a title, then every rendered
  component has balanced `()` and `[]` — a bracket is never removed without its partner.
- Given a template whose collapsible token sits between two components of rendered text, when it is
  rendered untitled, then the two neighbours are still separated — `{yyyy}/{yyyy}-{mm}-{dd} {slug}
  ({HH}{MM})` → `2026/2026-08-05 (1432)` and `{yyyy}/{dd}({slug}){HH}` → `2026/05 14`.
- Given a text-only leaf that parses, when it is rendered at any seq, then it is byte-identical to
  what was typed plus the ordinal — the collision suffix never shortens a folder the user spelled
  out.
- Given `{yyyy}/x` + 300 literal characters + `{seq}`, and given `{yyyy}/nul{seq}`, when they are
  parsed, then each is refused exactly as the same component without `{seq}` is.
- Given a title of `"..."`, when it is rendered against `{yyyy}/{yyyy}-{mm}-{dd}_{title}`, then the
  result is `2026/2026-08-05` — the same as an untitled recording, with no stranded separator.
- Given every component shape the tests construct, when `renders_something` and the render pass are
  compared, then they agree: a component is refused as optional exactly when it renders to nothing
  untitled. The leaf guarantee rests on that agreement, so it is asserted rather than assumed.
- Given `bun run test:rust`, when it runs, then the new tests pass and every pre-existing
  `notes::naming` test still passes unmodified.
- Given `bun run check:core-tauri-free` and `bun run check:core-sync-free`, when they run after this
  change, then both exit 0, and `git diff` shows no `Cargo.toml` or `Cargo.lock` change.
- Given `cargo clippy --workspace --all-targets -- -D warnings`, when it runs, then it is clean with
  no `.unwrap()`/`.expect()` outside `#[cfg(test)]`.

## Design Notes

**Why `seq` is 1-based.** `seq` is the ordinal of this folder among siblings that rendered to the
same path — `1` is the first and adds nothing. It keeps the existing user-visible ` (2)`-for-the-
second-folder convention, and lets 40.3 write `for seq in 1..` with no off-by-one.

**Collapse rule, stated once.** `{title}`, `{slug}` and `{seq}` are *optional* tokens. When one
renders empty it removes itself, and then the separators around it are **normalised**: filler runs
that became adjacent merge into one, and filler at the component's edges is trimmed. Stated as
"eat one adjacent run" it welded neighbours that had both rendered — `{dd} {slug}{title} {HH}`
untitled produced `0514`, not `05 14`, because two collapsing tokens ate both separators and the
day ran into the hour. Merge-and-trim gives the same answers everywhere the earlier rule was
right (`…{HH}{MM} {slug}` untitled is still `2026/2026-08-05 1432`, `rec-{slug}` is still `rec`,
`{yyyy}/{slug} {HH}{MM}` is still `2026/1432`) and the readable answer where it was wrong.
Applying it during segment assembly (rather than trimming the finished string) is what makes
`{yyyy}/{slug}/x` → `2026/x` instead of an empty component. When the two runs that merge are not
identical the *left* one survives (`{dd}-{slug} {HH}` untitled is `05-14`), because it is the
separator the user wrote against the text that is still there. `MayRenderEmpty` at parse time then
closes the only remaining hole: a template whose every component is optional-only has no rendering
left to fall back on, and inventing an "Untitled" placeholder is exactly what the epic refuses.

**Only filler the collapse exposed is trimmed.** "Trim filler at the component's edges", left
unqualified, made a component's own decoration depend on whether it had a title:
`{yyyy}/_{HH}{MM}_{slug}` rendered `2026/_1432_standup` titled but `2026/1432` untitled, so the
underscores the author deliberately typed survived only for titled recordings. Gating the trim on
"something collapsed" is the wrong gate — the question is not *whether* a token collapsed but
*which* filler that collapse exposed. So: a filler run is removed only when it is adjacent to a
token that rendered empty. Filler with no collapsed token beside it is text the user wrote and it
stays. Every pinned answer is unchanged, because in each of them the filler is adjacent to the
collapse: `…{HH}{MM} {slug}` untitled → `2026/2026-08-05 1432`, `rec-{slug}` → `rec`,
`{yyyy}/{slug} {HH}{MM}` → `2026/1432`, `{dd} {slug}{title} {HH}` → `05 14`. What changes is
`_{HH}{MM}_{slug}`, which now renders `_1432` untitled and `_1432_standup` titled — one decoration,
both ways.

This is also what makes `renders_something` agree with what `render` actually does. It called a
literal of pure filler "may render nothing", so `{yyyy}/-` was refused as `OptionalLeaf("-")` — a
false statement, since `{yyyy}/-/{mm}` renders `2026/-/08` with that same `-` intact. Under the
exposure rule the predicate is exact: a literal character survives unless a collapsible token sits
next to it. `{yyyy}/-` is accepted and renders `2026/-`; `{yyyy}/-{slug}` is still `OptionalLeaf`,
because there the `-` really can be eaten.

**`{seq}` belongs to the leaf, and nowhere else.** The contract says the ordinal exists to produce
a *sibling*. A `{seq}` written upstream of the last component breaks that twice over, and the leaf
rule alone does not catch it because the leaf still renders: `{yyyy}{seq}/{yyyy}-{mm}-{dd} {HH}{MM}`
renders `2026/…` then `2026 (2)/…`, `2026 (3)/…` — the year directory renamed once per collision,
which is verbatim the hazard the leaf rule was added to close; and `{yyyy}/{slug}{seq}/{mm}-{dd}`
untitled renders `2026/08-05` at seq 1 but `2026/(2)/08-05` at seq 2, so the retry lands one level
deeper, inside a folder named `(2)`. Both follow from `has_seq` being true for a `{seq}` *anywhere*,
which then suppresses the leaf's own ordinal. Since the contract also fixes the remedy —
"validation, not sanitisation: `parse` rejects with a typed reason and never rewrites" — quietly
ignoring an upstream `{seq}` is not available, and it is refused at parse instead.

**Every component fits the filesystem, not just the leaf.** The contract states the cap on the leaf
because that is where a title normally lands, but every component the template renders becomes a
directory that `mkdir` has to create, and 40.3's retry loop only ever varies the last one. A title
in an interior component reaches the same 255-byte wall with no way back: `{title}/{yyyy}-{mm}-{dd}`
with a 120-emoji title renders a 320-byte first component, and every retry reproduces it byte for
byte. The same re-render-shorter pass therefore runs on each component, with the collision ordinal
subtracted from the leaf's budget only.

**Invisible is not legal.** The illegal set was defined as the characters the three filesystems
refuse, and Unicode *format* characters (category `Cf`) are not among them — so `U+FEFF` alone
parsed as a template and rendered a folder whose entire name is invisible, and the title
`annual\u{202e}gpj.review` rendered verbatim, which Finder draws as `annualweiver.jpg`, the ordinary
right-to-left-override spoof. A title arrives from a bridge, an agent or a paste; a folder the user
can neither see, type, nor recognise defeats the epic's whole premise as thoroughly as an illegal
character does. `Cf` joins `Cc` in `is_illegal`, which puts it on both sides at once: refused in a
template, filtered out of a title.

`std` exposes no general category test — `char::is_control` is `Cc` and stops there — and AD-55
refuses a unicode crate, so `is_format` is a written-out range table, complete for Unicode 16.0 and
documented as such next to the rule it serves. (`char::escape_debug` was considered as a std-only
proxy and rejected: it also escapes `'` and `"`, so it would have made an apostrophe in a title
illegal.)

**A template component is never rewritten either.** `ParentComponent` matched a component that was
exactly `.` or `..`, and everything else fell through to render-time edge trimming — so
`{yyyy}/../{mm}` was a typed error while `{yyyy}/  ..  /{mm}` was silently deleted, `{yyyy}//{mm}`
became `2026/08`, and `{yyyy}/..a` became `2026/a`. Story 40.2 puts a live preview beside the raw
field; a preview that disagrees with what the user typed, with no error to explain the difference,
is worse than a rejection. Parse therefore refuses an empty component and one whose literal text is
`.` or `..` once surrounding filler is discounted. This is about *literals the user typed*, not
about rendered titles — a title is data and stays filtered.

Three reasons come out of it — `ParentComponent` when the text is `.` or `..` net of surrounding
whitespace, `EmptyComponent` when the trim would leave nothing (`//`, a trailing `/`, `   `, `...`),
and `PaddedComponent` naming the folder when the trim would merely shorten it (`..a`, ` x `).

Scoping that check to components holding **no token at all** was too narrow, and left the more
alarming half of the reported case intact: `{yyyy}/..{mm}` still rendered `2026/08`, `{yyyy}/. {mm}`
still rendered `2026/08`, and `{yyyy}/{mm} ` still rendered `2026/08` — so `{yyyy}/  ..  /{mm}` was
a typed error while `{yyyy}/..{mm}` deleted the same two characters without a word. The rule is
about *typed literals*, and a literal does not stop being typed because a token shares its
component. The check therefore applies to a component's **outer edges** wherever they are literal:
if the first segment is a literal beginning with edge noise, or the last segment is a literal ending
with edge noise, the component is refused with `PaddedComponent` naming it. Interior literals are
untouched — they are the separators the collapse rule governs, and `{yyyy}-{mm}-{dd} {HH}{MM} {slug}`
must keep losing its trailing separator when `{slug}` collapses.

The consequence to accept, and to state rather than discover: `  {yyyy}  ` is now **refused** rather
than quietly rendered as `2026`. That reverses an earlier decision here, and it is the coherent one —
"validated, never sanitised" cannot mean that padding is rewritten in one component shape and
rejected in another. `DEFAULT_TEMPLATE` and every template in the matrix are unaffected: none of
them begins or ends a component with a literal.

The trailing separator moves with it: `{yyyy}/{slug}/` is now `EmptyComponent` rather than
`OptionalLeaf("")`. It is still refused — the KEEP list's point — but by the reason that can give
advice a stray `/` can act on, instead of one that quoted an empty string back at the user and told
them to give it a date token.

**Sanitising a title is not sanitising a template.** The template is the user's *specification* and
is rejected when illegal; a title is *data* that arrives from anywhere (a bridge, an agent, a paste)
and is filtered — illegal characters and control codes out, whitespace runs collapsed to one space,
trimmed, capped at 80 characters on a char boundary. Rejecting a recording because its title
contains a slash would be absurd; rejecting a template that contains one is the point.

**Illegal set:** `< > : " / \ | ? *`, `\0`, control characters (`Cc`) and Unicode format characters
(`Cf`) — the union of what APFS, exFAT and NTFS refuse, plus the ones they accept but no human can
read. `/` is legal in a *template* (it is the separator) and never survives inside a rendered
component.

**A token-free component is decided entirely at parse.** "Validated, never sanitised" was applied to
edge padding and stopped there, which left two other silent rewrites and put the spec at odds with
itself — `parse_refuses_a_folder_render_would_have_rewritten` and
`a_leaf_that_is_all_literal_is_clamped_rather_than_refused` pin opposite policies two screens apart.
The rule that resolves it: when a component holds **no token**, its rendered text is exactly the
text that was typed and is therefore knowable at parse, so every rewrite `render` would have applied
is refused there instead. Three cases, beyond the edge padding already handled: `.`/`..`
(`ParentComponent`), nothing left (`EmptyComponent`), and now also a component that exceeds 255
bytes and one that folds to an MS-DOS device name — `{yyyy}/x` + 300 `y`s currently deletes fifty
typed characters in silence, and `{yyyy}/nul` currently renders `2026/nul-rec` in silence, both
beside a 40.2 preview that will disagree with the field and explain nothing. Each gets its own typed
reason naming the component.

The render-time clamp and `de_reserve` stay exactly as they are for components that **do** hold a
token: there the length and the fold depend on a title that arrives at record time, `render` is
infallible, and rewriting *data* is the whole point of the title/template distinction. What is
refused is a rewrite of the user's specification, not a rewrite of a bridge's title.

Two corrections to that boundary, both from cases where "no token" was read too literally:

- **`{seq}` is not a title.** A component whose only token is `{seq}` renders text that is still
  entirely knowable at parse, so the rationale for deferring to render does not reach it — and it
  became a hole straight through both checks: `{yyyy}/x` + 300 `y`s + `{seq}` clamps fifty typed
  characters away in silence while the same component without `{seq}` is refused, and
  `{yyyy}/nul{seq}` renders `2026/nul-rec` in silence while `{yyyy}/nul` is refused. The test is
  therefore "holds no *title-dependent* token" — no `{title}` and no `{slug}` — rather than "holds no
  token at all".
- **The leaf's parse budget must leave room for the ordinal.** Capping a text-only component at
  exactly 255 bytes is right for an interior component but wrong for the leaf, because `render`
  appends the collision ordinal there and then clamps to fit: a 255-byte typed leaf renders as typed
  at seq 1 and silently loses eight characters at seq 2. The leaf's parse budget is 255 bytes **less
  the widest ordinal** (`seq_suffix(u32::MAX)`), so a template that parses is a template whose leaf
  survives every collision unshortened. A fixed reservation is right rather than clever: it is the
  only figure that holds without knowing which seq the retry loop will reach.

The two clamp shapes the sweep depends on move with that line rather than around it.
`{yyyy}/x<400 y's> {slug}` is untouched. `{yyyy}/x<400 y's>{seq}` is now **refused** — which is
precisely the first correction — so the sweep's second clamp shape gains a `{slug}`
(`{yyyy}/x<400 y's>{slug}{seq}`) and goes on proving the same thing: a leaf whose literal overruns
`NAME_MAX`, and whose ordinal is rendered *in place* at its end, still renders three different names
at seq 1, 2 and 7, each still showing its ordinal. What survives from the earlier wording is the
reason the ordinal is a special case at all: it is not text the user typed. It arrives with the
recording, like a title, so the leaf must make room for it — at parse now, by reserving the widest
one, rather than only at render, by clamping a leaf that was already accepted.

**A dangling separator is a placeholder by another name.** `is_filler` covered whitespace, `.`, `-`
and `_`, so a template that parenthesises its title kept the brackets when the title went away:
`{yyyy}/{yyyy}-{mm}-{dd} ({title})` untitled rendered `2026/2026-08-05 ()`, and `[{slug}]` rendered
`[]`. The epic's words are "no dangling separator, no 'Untitled' placeholder", and an empty pair of
brackets is both — it is the word "Untitled" spelled in punctuation. Parenthesising the optional part
is one of the first templates a user writes, so a collapsing token must take its brackets with it.

**Brackets pair; they are not filler.** Putting `(`, `)`, `[` and `]` into the filler set was the
wrong mechanism, and it made things worse than the defect it closed. Filler is directionless, so a
bracket had to be given a *facing* to know which neighbour it belonged to, and a facing rule cannot
keep a pair together: measured, `{yyyy}/{yyyy}-{mm}-{dd} {slug} ({HH}{MM})` untitled lost the space
and rendered `2026/2026-08-05(1432)`; `{yyyy}/{dd}({slug}){HH}` untitled welded its neighbours into
`2026/0514`, which is verbatim the defect loopback 1 fixed; `{yyyy}/({slug} {SS})` rendered
`2026/07)` and `{yyyy}/{slug}(){SS}` rendered `2026/(07`, both unbalanced; and `{yyyy}/{slug}(`
rendered a session folder named `(`.

The mechanism that matches what a bracket *is*: brackets are **text**, and a collapsing token takes
a **matching pair** with it. When an optional token renders empty, and its nearest non-filler
neighbour on the left is `(` or `[` and on the right is the closer that matches it, that pair is
removed together with the token. The existing filler rules then run unchanged — the exposure rule
removes the separators the removal exposed, adjacent runs merge, and the loopback-1 repair puts one
separator back when text survives on both sides with nothing left between them.

This is strictly better than facing, because pairing is symmetric: a bracket is never removed without
its partner, so the collapse can never *unbalance* a name and can never weld two neighbours. It does
not promise a name the user cannot make ugly on purpose — `{yyyy}/{slug}(` renders `2026/(` untitled,
because a lone `(` has no partner to leave with and is text the user typed; that is preservation, not
a rewrite. It also keeps `renders_something` derivable — `{yyyy}/({slug})` is `OptionalLeaf`, because the
pair goes with the token and nothing is left, while `{yyyy}/(x)` and `{yyyy}/()` render as typed,
since a bracket the user wrote around text that always renders is text they asked for.

Worked answers: `{yyyy}-{mm}-{dd} ({title})` untitled → `2026-08-05`, titled → `2026-08-05 (Standup)`.
`{HH}{MM} {slug} ({SS})` untitled → `1432 (07)` — the `(` there has no matching `)` around `{slug}`,
so nothing pairs, and only the exposed space goes. `{dd} ({slug}) {HH}` untitled → `05 14`.
`{dd}({slug}){HH}` untitled → `05 14` as well, by the loopback-1 repair. `{yyyy}/{slug}(` renders
`2026/(` titled and untitled alike.

Two details the pairing rule settles, which the facing rule reached for and missed. First, whatever a
removed bracket was standing in front of goes back through the same splitter a literal goes through,
so a separator the bracket was hiding becomes filler again rather than staying welded to the text:
`x ({title}) y` untitled is `x y`, not `x   y`. Second, the loopback-1 repair — one separator comes
back when the collapse ate everything between two things that both rendered — does **not** fire where
a bracket already stands at the junction, because a bracket reads as an edge by itself:
`({slug} {SS})` untitled is `(07)`, not `( 07)`, and `({SS} {slug})` is `(07)`, not `(07 )`. Where
there was no separator to begin with and a pair left, a single **space** stands in for it
(`{dd}({slug}){HH}` → `05 14`), because half a bracket cannot come back without leaving the name
unbalanced.

An unmatched bracket is simply text and is rendered as typed, titled and untitled alike:
`{yyyy}/{slug}(` renders `2026/(` untitled and `2026/standup(` titled, and `{yyyy}/{slug}(){SS}`
renders `2026/()07`. That is the honest consequence of "brackets are text" — the module preserves the
user's bracket balance rather than inventing one, so a template the user wrote unbalanced renders
unbalanced, and the guarantee is that *no pair is ever half-removed*.

**"Renders empty" means empty after the edge trim.** `{title}` and `{slug}` collapse when they
render nothing, but a title of `"..."` survives `sanitize_title` verbatim — dots are neither
whitespace nor illegal — so no collapse fires, and the component's edge trim then removes the dots
anyway and leaves the separator stranded: `{yyyy}-{mm}-{dd}_{title}` renders `2026-08-05_`. The
token is optional and it contributed nothing; that is the definition the collapse rule needs. An
optional token counts as collapsed when its rendered text is empty **once edge noise is discounted**,
which puts `"..."` where `"!!!"` already was.

**One predicate for `MayRenderEmpty` and `OptionalLeaf`, and it is the renderer.**
`renders_something(&[Segment])` already answered "is this component guaranteed to render something"
for the whole-template check; the leaf rule is the same question asked of `components.last()`. Two
call sites, one definition, so the two errors can never disagree about what "optional" means. But the
definition was a *second walk* of the collapse rule — over `Segment`s, where the renderer walks
`Unit`s — and the leaf guarantee, this story's whole answer to the intent gap, rests on those two
walks agreeing. Nothing tied them; each loopback that touched the collapse had to remember to touch
both, and the brackets are precisely the shape where the two are easy to write differently. The
predicate is therefore defined *by* the renderer: a component is optional exactly when rendering it
untitled produces nothing. The untitled recording is the right question and a sufficient one — a
title only ever turns a collapsed token back into text, and text is what keeps the separators around
it alive, so nothing a title can be makes a component render less. The agreement is then asserted as
well as arranged, over every component shape the tests build combinatorially. Order matters and encodes the matrix's
precedence: the `any()` over all components runs first, so a template where *nothing* is guaranteed
keeps the broader reason. A **trailing separator** (`{yyyy}/{slug}/`) is a last component built from
nothing at all, and is refused the same way rather than silently dropped — dropping it would reopen
exactly the hole the leaf rule closes.

**Where the 255-byte cap is enforced, and why not earlier.** Not in `sanitize_title`: a title does
not know what date prefix, literal or collision ordinal will share its component. Each component is
rendered normally first, then — only if it does not fit — rendered again with a shorter title, in a
loop that asks for the byte deficit in *characters* (a character frees at most four bytes, so it
never cuts more than it must and never fewer than one, and therefore ends). A component can still be
too long with no title in it — a long *literal* beside a token — and `render` is infallible, so that
case is clamped on a character boundary with `RESERVED_SUFFIX`-worth of room left, then run back
through the edge-noise trim and `de_reserve` so the clamped name is still legal. A component that
holds *no* token never reaches the clamp: it was refused at parse, where its length was already
known.

**The ordinal survives the clamp, whichever end it was written at.** Subtracting it from the leaf's
budget "up front" was claimed to cover both orderings; it covers only one. When the template carries
its own `{seq}`, the ordinal is rendered *inside* the component, at its end, and nothing is
subtracted — so the clamp, which returns a *prefix*, cuts the ordinal straight off. Measured on the
committed code: `{yyyy}/x<400 y's>{seq}` renders a byte-identical 251-byte leaf at seq 1, seq 2 and
seq 7, and story 40.3's `for seq in 1..` loop would then ask forever for a path that is already
taken. The rule that holds in both orderings: the leaf's ordinal text is reserved out of the budget
whether it will be appended afterwards or was rendered in place, and when a clamp fires it is
re-attached after the cut. A collision must change the name; a cap that can erase the only thing
distinguishing two sessions is worse than a name the filesystem refuses, because the filesystem at
least says so.

## Verification

**What the loopback-4 tests pin.** Five named tests. Three, one per change:
`brackets_pair_rather_than_face_a_direction`, which walks all four measured facing defects and pins
the worked answers (`2026/2026-08-05 (1432)`, `05 14` with and without typed separators, `(07)`,
`2026/()07`, `2026/(`, `2026/(x)`, `2026/()`, and `({slug})` as `OptionalLeaf`);
`a_text_only_leaf_keeps_room_for_the_collision_ordinal`, which pins the leaf's parse budget at
`255 - seq_suffix(u32::MAX).len()` and an interior folder's at the full 255; and
`a_seq_bearing_folder_is_decided_at_parse_like_a_text_only_one`, which pins `{yyyy}/x…{seq}` and
`{yyyy}/nul{seq}` refused with exactly the reason the same folder without `{seq}` earns. Two are the
properties: `every_rendered_component_has_balanced_brackets` over eleven bracket-bearing templates ×
four titles × three seqs, and `a_text_only_leaf_renders_what_was_typed_plus_the_ordinal` at seq 1, 2,
7, 1 000 and `u32::MAX`.

`assert_legal` now carries the bracket property itself, so all 1 000 swept titles assert it: it
checked legality and never legibility, which is the same blind spot that let `(1432` and `07)` pass
14 000 assertions. `{yyyy}/{yyyy}-{mm}-{dd} {slug} ({HH}{MM})` joins `SWEEP_TEMPLATES` — no template
had the shape `<rendered> <collapsible> (<rendered>)`, which is why nothing in the suite could see
the defect. The sweep's second clamp shape becomes `{yyyy}/x<400 y's>{slug}{seq}`, since the shape
without the `{slug}` is refused at parse now.

**What the loopback-3 tests pin.** Four named tests, one per change:
`a_folder_of_text_alone_that_is_too_long_is_refused_not_clamped`,
`a_folder_of_text_alone_that_names_a_device_is_refused_not_escaped`,
`a_collapsing_token_takes_its_brackets_with_it` and
`a_title_of_nothing_but_edge_noise_collapses_like_an_empty_one`; plus
`renders_something_agrees_with_what_render_does`, which builds every one-, two- and three-atom
component shape over `{yyyy} {slug} {title} x - _ . ( ) [ ]` and asserts, through the public API in
both an interior slot and a leaf slot, that the predicate and the render pass answer the same
question the same way. `{yyyy}/{yyyy}-{mm}-{dd} ({title})` joins the sweep templates, so the bracket
rule is swept against the 1 000 hostile titles as well.

**Commands:**
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-core path_template` -- expected:
  all new tests pass.
- `bun run test:rust` -- expected: whole workspace green, `notes::naming` tests unchanged.
- `bun run check:rust` -- expected: rustfmt clean, clippy clean under `-D warnings`.
- `bun run check:core-tauri-free && bun run check:core-sync-free` -- expected: both exit 0.
- `git status --porcelain -- src-tauri/Cargo.toml src-tauri/Cargo.lock src/` -- expected: empty (no
  dependency change, no frontend change).

## Spec Change Log

### 2026-08-06 — bad_spec loopback 4

**Context.** A second exhaustive pass (229 997 parsing templates × 690 000 renders, plus a 99 180-render
3-atom sweep with the leaf-empty branch instrumented to panic) again found **zero** violations of the
module's hard guarantees: no empty path, no empty/oversize/reserved/illegal component, no depth change
on collision, no parent renamed, no two seqs naming one folder, and the leaf never rendered empty. The
findings below are the *quality of the name* and two holes in loopback 3's own parse-time rule.

**Triggering findings.**

1. `[high]` "Brackets are filler" was the wrong mechanism, and this spec chose it. Filler is
   directionless, so the implementation had to invent a `Facing` (`(`/`[` face right, `)`/`]` face
   left) — and a facing rule cannot keep a pair together. Measured: `{yyyy}/{yyyy}-{mm}-{dd} {slug}
   ({HH}{MM})` untitled → `2026/2026-08-05(1432)`, the space gone; `{yyyy}/{dd}({slug}){HH}` untitled
   → `2026/0514`, verbatim the welding defect loopback 1 fixed; `{yyyy}/({slug} {SS})` → `2026/07)`
   and `{yyyy}/{slug}(){SS}` → `2026/(07`, both unbalanced; `{yyyy}/{slug}(` → a session folder named
   `(`. A 5-atom enumeration finds 159 shapes in the first family alone.
2. `[medium]` The token-free length cap ignores the ordinal `render` appends, so the "decided
   entirely at parse" guarantee breaks on the first collision: a 255-byte typed leaf renders as typed
   at seq 1 and silently loses eight characters at seq 2. Any text-only leaf of 252–255 bytes is
   affected, and 40.2's preview renders seq 1, so it cannot show it.
3. `[medium]` A component whose only token is `{seq}` escapes both new parse checks, because "holds
   no token" was read literally and `{seq}` is not a title. Measured: `{yyyy}/x` + 300 `y`s +
   `{seq}` clamps fifty typed characters in silence while the same component without `{seq}` is
   refused with `OverlongComponent`; `{yyyy}/nul{seq}` renders `2026/nul-rec` in silence while
   `{yyyy}/nul` is refused with `ReservedComponent`.

**What was amended** (all outside `<intent-contract>`): two Execution tasks and five Acceptance
Criteria added; the bracket Design Note replaced — brackets are text and a collapsing token takes a
matching pair with it — and the parse-time boundary corrected on both `{seq}` and the leaf's ordinal
reservation.

**Known-bad state to avoid on re-derivation.** Do not reproduce: brackets in `is_filler`; any
directional/`Facing` treatment of a separator; a parse-time cap that ignores the ordinal the leaf will
gain; a "holds no token" test where "holds no `{title}`/`{slug}`" is meant.

**KEEP.** All three earlier KEEP lists stand, and loopback 3's other work is verified and stays:
`OverlongComponent` and `ReservedComponent` themselves, `reserved_stem()` shared between parse and
render so the two cannot disagree about what a device name is, the "renders empty net of edge noise"
collapse test, and `renders_something` being defined *through* the renderer rather than as a second
independent walk. The `Facing` machinery is the only thing being taken back out.

### 2026-08-06 — bad_spec loopback 3

**Context.** Loopback 2's three fixes were confirmed correct and complete: an adversarial pass swept
461 776 parsing templates × 5 titles × seq {1, 2, 7} against every guarantee in the module doc, plus
`renders_something` agreement in both directions over 39 656 component shapes, and found **zero**
violations. Nothing below reopens them. What is left is scope: rules that were stated generally and
implemented for one case.

**Triggering findings.**

1. `[medium]` The spec contradicts itself. "A folder `render` would have had to rewrite is refused
   here instead of quietly rewritten" was implemented for edge padding only, while the byte-cap
   Design Note endorses silently clamping an over-long literal component. Measured: `{yyyy}/x` + 300
   `y`s renders a 251-byte leaf with fifty typed characters deleted and no error — and
   `a_leaf_that_is_all_literal_is_clamped_rather_than_refused` pins that as intended two screens from
   `parse_refuses_a_folder_render_would_have_rewritten`, which pins the opposite policy.
2. `[medium]` The device-name escape is outside the same rule: `{yyyy}/nul` renders `2026/nul-rec`,
   `{yyyy}/CON/{mm}` renders `2026/CON-rec/08`, silently. A literal-only component's fold is
   statically decidable at parse exactly as `..a` is.
3. `[medium]` `is_filler` covers whitespace, `.`, `-` and `_` only, so
   `{yyyy}/{yyyy}-{mm}-{dd} ({title})` untitled renders `2026/2026-08-05 ()` and `[{slug}]` renders
   `[]`. The epic refuses "a dangling separator" and "an 'Untitled' placeholder"; an empty bracket
   pair is both, reached through punctuation rather than a word.
4. `[low]` A title of `"..."` survives `sanitize_title` verbatim, so no collapse fires, and the edge
   trim then strips it anyway: `{yyyy}-{mm}-{dd}_{title}` renders `2026-08-05_`, stranding exactly
   the separator the collapse rule exists to remove.

**What was amended** (all outside `<intent-contract>`): three Execution tasks and four Acceptance
Criteria added; three Design Notes written — token-free components decided entirely at parse, brackets
joining the filler set, and "renders empty" meaning empty after the edge trim.

**Known-bad state to avoid on re-derivation.** Do not reproduce: a clamp or a `de_reserve` that fires
on a component with no token in it; an `is_filler` that omits brackets; a collapse test that asks
whether the token's text is empty rather than whether it is empty net of edge noise.

**Boundary to hold.** The clamp and `de_reserve` must stay exactly as they are for components that
*do* hold a token. There the text depends on a title arriving at record time, `render` is infallible,
and rewriting *data* is the whole point of the title/template distinction. What is being refused is a
rewrite of the user's specification, never a rewrite of a bridge's title.

**KEEP.** Both earlier KEEP lists still stand. Additionally, loopback 2's work is verified and must
survive intact: the ordinal reserved out of the budget and re-attached after a clamp; the
`Unit { Text | Filler | Gap }` join pass and its exposure rule; `renders_something` following that
same rule; the edge-padding rejection including the deliberate `  {yyyy}  ` reversal; and all 39
tests — the sweep and its clamp shapes especially, since they are what proved fix 1.

### 2026-08-06 — bad_spec loopback 2

**Triggering findings.** Three defects, all measured by executing the loopback-1 code. The first is
a regression the loopback itself introduced; the other two are loopback-1 fixes that were scoped
narrower than the rule they were written to enforce.

1. `[high]` The per-component byte cap can clamp an explicit `{seq}` off the leaf. `fit_component`'s
   all-literal fallback returns a *prefix*, and with `{seq}` in the template the ordinal is rendered
   at the component's end, where the prefix cut removes it — while `has_seq` suppresses the appended
   ordinal that would have replaced it. Measured: `{yyyy}/x<400 y's>{seq}` renders a byte-identical
   251-byte leaf at seq 1, seq 2 **and** seq 7. Story 40.3's `for seq in 1..` loop would ask forever
   for a path that is already occupied, or write a second recording into the first one's folder.
   This is fix #2 of loopback 1 (cap widened to every component) colliding with fix #1 (`{seq}`
   scoped to the leaf), and the Design Note's claim that subtracting the ordinal "covers both
   orderings" is simply false for the clamp path.
2. `[medium]` "A template component is never rewritten" was implemented only for components holding
   no token, so the alarming half of the reported case survived: `{yyyy}/..{mm}` → `2026/08`,
   `{yyyy}/. {mm}` → `2026/08`, `{yyyy}/{mm} ` → `2026/08`. `{yyyy}/  ..  /{mm}` is a typed error
   while `{yyyy}/..{mm}` deletes the same two characters silently.
3. `[medium]` The collapse rule's trim was gated on "something collapsed" rather than on which
   filler the collapse exposed, so a component's decoration became title-dependent:
   `{yyyy}/_{HH}{MM}_{slug}` renders `2026/_1432_standup` titled but `2026/1432` untitled. The same
   gate makes `renders_something` disagree with `render_component` about a filler-only literal:
   `{yyyy}/-` is refused as `OptionalLeaf("-")`, which is untrue — `{yyyy}/-/{mm}` renders
   `2026/-/08` with that `-` intact.

**What was amended** (all outside `<intent-contract>`): three Execution tasks and three Acceptance
Criteria added; the collapse Design Note re-scoped from "trim the edges" to "remove only the filler a
collapse exposed", with `renders_something` following the same rule; the never-rewritten Design Note
widened from token-free components to a component's literal outer edges; and the byte-cap Design
Note corrected on the ordinal.

**Known-bad state to avoid on re-derivation.** Do not reproduce: a clamp that returns a bare prefix
of a leaf whose ordinal was rendered in place; an edge-padding check that inspects only components
with no token; a trim gated on `collapsed_any` rather than on adjacency to the collapsed token; a
`renders_something` that calls a filler-only literal optional when no collapsible token sits beside
it.

**Reversal recorded deliberately.** `  {yyyy}  ` was accepted and rendered `2026` under loopback 1's
Design Note; under this amendment it is refused. Padding cannot be rewritten in one component shape
and rejected in another and still be called "validated, never sanitised". The existing test that
pinned the old behaviour must change with it. `DEFAULT_TEMPLATE` and every matrix template are
unaffected — none begins or ends a component with a literal.

**KEEP — what worked and must survive.** Everything on loopback 1's KEEP list still stands, plus
what loopback 1 itself got right and this pass does not touch:

- `TemplateError::SeqOutsideLeaf` and the first-component-holding-`{seq}` detection.
- The per-component budget loop in `render`, which also retired the untested
  `components.last()`/`rendered.last_mut()` coupling — keep the ordinal appended inside the loop.
- `is_format`'s range table: independently verified against the Unicode 16.0 database as exactly the
  `Cf` set, no misses and no over-inclusions. Do not regenerate or "simplify" it.
- `EmptyComponent`, `PaddedComponent`, and the trailing separator moving from `OptionalLeaf("")` to
  `EmptyComponent` so the advice matches the fault.
- The 36 existing tests and the extended sweep — only the `  {yyyy}  ` assertion changes.

### 2026-08-06 — bad_spec loopback 1

**Triggering findings.** Five spec-level defects, all measured by executing the committed code, none
of them reachable from a defect in `<intent-contract>`:

1. `[high]` `has_seq` is true for a `{seq}` *anywhere*, so an upstream `{seq}` suppresses the leaf's
   ordinal: `{yyyy}{seq}/{yyyy}-{mm}-{dd} {HH}{MM}` renames the year directory once per collision
   (`2026`, `2026 (2)`, `2026 (3)`) — the exact hazard the leaf rule was added to close — and
   `{yyyy}/{slug}{seq}/{mm}-{dd}` untitled renders one level deeper at seq 2 (`2026/(2)/08-05`).
2. `[high]` The 255-byte cap is enforced on the leaf only, though every component becomes a
   directory: `{title}/{yyyy}-{mm}-{dd}` with a 120-emoji title renders a 320-byte first component
   that `mkdir` refuses and 40.3's leaf-only retry loop can never shorten.
3. `[medium]` Unicode format characters (`Cf`) pass both the template validator and the title filter
   (`is_illegal` covers `Cc` only): `U+FEFF` alone parses as a template, and the title
   `annual\u{202e}gpj.review` renders verbatim as the standard RTL-override spoof.
4. `[medium]` `ParentComponent` matches only an exact `.`/`..` component, so `{yyyy}//{mm}`,
   `{yyyy}/  ..  /{mm}` and `{yyyy}/..a` are silently rewritten instead of refused — against
   "validated, never sanitised", and against 40.2's live preview.
5. `[low]` The collapse rule as written ("eat one adjacent run") welds neighbours that both
   rendered: `{dd} {slug}{title} {HH}` untitled renders `0514`, not `05 14`.

**What was amended** (all outside `<intent-contract>`): five Execution tasks added, four Acceptance
Criteria added, and five Design Notes written or rewritten — the collapse rule restated as
merge-and-trim, `{seq}` scoped to the leaf, the byte cap widened to every component, `Cf` added to
the illegal set, and template-literal components brought under "never rewritten".

**Known-bad state to avoid on re-derivation.** Do not reproduce: `has_seq` computed over all
components; a `NAME_MAX` check that reads `components.last()` only; an `is_illegal` that tests
`is_control()` alone; a `.`/`..` check that compares a component to those two strings exactly; a
collapse pass that lets each collapsing token eat a separator independently of its neighbours.

**KEEP — what worked and must survive.** The module's shape is right and was verified end to end;
this loopback adds rules, it does not redesign. Preserve:

- The pure `parse`/`render` split with a `Segment`/`Token` IR, `RelativePath` as the only type
  `render` can produce, and `render` infallible.
- `renders_something` as the *single* predicate behind both `MayRenderEmpty` and `OptionalLeaf`,
  with the whole-template check first so the broader reason wins — the matrix's stated precedence.
- `OptionalLeaf` carrying the offending component reconstructed through `Token::name`, so the error
  quotes the user's own words back at them.
- Rejecting a trailing separator (`{yyyy}/{slug}/`) rather than dropping it.
- `fit_leaf`'s deficit-in-characters loop and its all-literal clamp fallback — the mechanism is
  correct, only its scope was too narrow.
- `slug_stem`'s extraction from `slug`, with `slug`'s 14 tests and outputs untouched.
- The seeded 1 000-title sweep, its untitled case, `assert_legal`, and the sibling-never-a-child
  property — extend these, do not replace them.

## Review Triage Log

### 2026-08-06 — Review pass 5 (bad_spec loopback 4)

- intent_gap: 0
- bad_spec: 3: (high 1, medium 2, low 0)
- patch: 11: (high 0, medium 3, low 8)
- defer: 0
- reject: 3
- addressed_findings:
  - `[high]` `[bad_spec]` "brackets are filler" forced a directional `Facing` rule that cannot keep a
    pair together — measured: a lost separator (`2026-08-05(1432)`), welded neighbours (`0514`, the
    loopback-1 defect returning), unbalanced names (`07)`, `(07`) and a folder named `(` — spec
    amended: brackets are text, and a collapsing token takes a *matching pair* with it.
  - `[medium]` `[bad_spec]` the text-only length cap ignored the ordinal `render` appends, so a
    255-byte typed leaf silently lost eight characters at seq 2 — spec amended: the leaf's parse
    budget reserves the widest ordinal.
  - `[medium]` `[bad_spec]` a component whose only token is `{seq}` escaped both parse checks
    (`{yyyy}/nul{seq}` → `nul-rec`, `{yyyy}/x<300>{seq}` clamped) while the same component without
    `{seq}` was refused — spec amended: the test is "holds no title-dependent token".

A second exhaustive pass (229 997 parsing templates × 690 000 renders, plus a 99 180-render 3-atom
sweep with the leaf-empty branch instrumented) again found zero violations of the module's hard
guarantees, so loopback 2's work and the leaf guarantee are confirmed and not reopened. Cascading
order: bad_spec findings exist, so the 11 patch findings are moot for this pass.
`review_loop_iteration` was incremented to 4 — one below the non-convergence ceiling, and the reason
to expect convergence is that severity has fallen each pass and every remaining item is now a patch.

#### Findings held moot by the cascade (recorded so nothing is lost)

**patch (11)** — `[medium]` the 1 884-shape agreement property is near-circular: `renders_something`
now *calls* the renderer, so the loop is an identity except for the parse↔render wiring; mutation
testing showed a broken separator repair still passes it, while the doc and the test preamble claim
the two "cannot disagree" as though 1 884 shapes had proved it. `[medium]` the leaf-always-renders
guarantee rests on unasserted arithmetic — `render`'s `if text.is_empty() { continue }` applies to the
leaf too, and only the clamp's `room ≥ 238` floor stops it firing; a `debug_assert!` would make it
self-enforcing. `[medium]` (carried) token names bypass `is_illegal`, so an RTL override reaches
`UnknownToken`'s message, which is 40.2's inline copy. `[low]` `OverlongComponent`'s payload is
unbounded and is the one error raised *because* the payload is too long: a 20 001-byte template
yields a 20 001-byte message printed inline in the settings pane. `[low]` `OptionalLeaf`'s advice
("give it some text of its own") contradicted the bracket rule; the amendment makes the advice true
again, but the message is still worth re-reading against the new rule. `[low]` no sweep template has
the shape `<rendered> <collapsible> (<rendered>)`, and `assert_legal` checks legality but never
legibility, so nothing in the suite could see the lost separator. `[low]` (carried) the clamp
relocates an in-place `{seq}` to the end; the ordinal's cost is computed twice; `de_reserve` inspects
the component with its ordinal (`nul-rec` beside `nul (2)`) and rebuilds from the untrimmed stem;
`{slug}` never passes `is_illegal`; a leading space makes an absolute path report `EmptyComponent`;
`seq: 0` renders identically to `seq: 1` and `RenderCtx` is unvalidated; the `{seq}` join overrides
the user's separator, now also inside brackets (`08-05( (2))`); `fit_component` shares one cap across
multiple title tokens; the token table omits the truncation caps and there is no `journal_path` drift
test; `notes/naming.rs:96` still emits one new rustdoc warning.

**reject (3)** — that "renders empty net of edge noise" should be position-dependent so a title of
`"..."` survives in an interior slot (`note{title}v2`): the spec states the rule unconditionally, and
a collapse that depends on where the token sits is harder to predict than the character it costs;
re-raising the `journal_path` grammar divergence, rejected in pass 2; and the already-recorded
`nul-rec`/`nul (2)` sibling item counted a second time under the bracket findings.

### 2026-08-06 — Review pass 4 (bad_spec loopback 3)

- intent_gap: 0
- bad_spec: 4: (high 0, medium 3, low 1)
- patch: 10: (high 0, medium 2, low 8)
- defer: 0
- reject: 2
- addressed_findings:
  - `[medium]` `[bad_spec]` the spec contradicted itself — "never rewritten" for edge padding while
    the byte-cap note endorsed silently clamping an over-long literal component (`{yyyy}/x<300 y's>`
    deletes fifty typed characters) — spec amended: a component holding no token is decided entirely
    at parse, with its own typed reason.
  - `[medium]` `[bad_spec]` the device-name escape sat outside that rule (`{yyyy}/nul` → `2026/nul-rec`
    in silence) — spec amended: refused at parse for a token-free component; `de_reserve` stays for
    token-bearing ones, where the fold depends on a title.
  - `[medium]` `[bad_spec]` `is_filler` omitted brackets, so `({title})` untitled rendered `()` — the
    "Untitled" placeholder spelled in punctuation, which the epic refuses — spec amended: `(`, `)`,
    `[`, `]` join the filler set.
  - `[low]` `[bad_spec]` a title of `"..."` rendered non-empty, so no collapse fired and the edge trim
    stranded the separator (`2026-08-05_`) — spec amended: an optional token counts as collapsed when
    its text is empty once edge noise is discounted.

An adversarial sweep of 461 776 parsing templates × 5 titles × seq {1, 2, 7} found zero violations of
the module's stated guarantees, and `renders_something` was verified to agree with rendering in both
directions over 39 656 component shapes — so loopback 2's three fixes are confirmed correct and are
not reopened. Cascading order: bad_spec findings exist, so the 10 patch findings are moot for this
pass. `review_loop_iteration` was incremented to 3.

#### Findings held moot by the cascade (recorded so nothing is lost)

**patch (10)** — `[medium]` nothing asserts that `renders_something` and the join pass agree, though
the leaf guarantee (this story's resolution of the pass-1 intent gap) rests entirely on it; they are
two independent walks of one rule, one over `Segment`s and one over `Unit`s. `[medium]` (carried)
token names are collected between `{` and `}` without consulting `is_illegal`, so a right-to-left
override lands in `UnknownToken`'s message, which is 40.2's inline UI copy. `[low]` the module doc
promises the ordinal keeps its position "whichever end of the component the template put them at",
but the clamp path always relocates it to the end and can drop trailing literal text
(`{yyyy}/x<400 y's>{seq}z`). `[low]` no sweep template places `{seq}` anywhere but last, so the clamp
branch loopback 2 rewrote is untested for that shape. `[low]` the collapse Design Note's motivating
sentence claims more than the rule it states — decoration adjacent to the collapsed token still goes,
by design. `[low]` `between_text` relies on an undocumented "no empty `Unit::Text`" invariant, and the
`!text.is_empty()` guard beside it is dead. `[low]` the ordinal's cost is computed twice, once as
`seq_suffix` for the reservation and once as `format!("({})")` for the in-place render, with nothing
tying them. `[low]` `de_reserve` inspects the component *with* its in-place ordinal, so one template
yields the siblings `nul-rec` and `nul (2)`. `[low]` a title ending in `.` beside a literal beginning
with `.` renders `run..log`. `[low]` (carried) `de_reserve` rebuilds from the untrimmed stem;
`{slug}` never passes `is_illegal`; a leading space makes an absolute path report `EmptyComponent`;
`seq: 0` renders identically to `seq: 1`; `RenderCtx` is unvalidated; the `{seq}` join overrides the
user's separator; `fit_component` shares one cap across multiple title tokens; the token table omits
the truncation caps and there is no `journal_path` drift test; `notes/naming.rs:96` still emits one
new rustdoc warning.

**reject (2)** — trimming edge noise inside `sanitize_title` wholesale (it would strip trailing dots
from legitimate titles, and the stranded-separator case it aims at is fixed properly by the
collapse-empty refinement above); and re-raising the `journal_path` grammar divergence, rejected in
pass 2 as a difference of grammar rather than of vocabulary.

### 2026-08-06 — Review pass 3 (bad_spec loopback 2)

- intent_gap: 0
- bad_spec: 3: (high 1, medium 2, low 0)
- patch: 9: (high 0, medium 2, low 7)
- defer: 1: (high 0, medium 1, low 0)
- reject: 3
- addressed_findings:
  - `[high]` `[bad_spec]` the per-component byte cap clamps an explicit `{seq}` off the leaf, so
    seq 1, 2 and 7 render one byte-identical path and 40.3's retry loop never terminates — spec
    amended: the leaf's ordinal is reserved out of the budget and re-attached after a clamp in both
    orderings; new Design Note, Execution task and Acceptance Criterion.
  - `[medium]` `[bad_spec]` the never-rewritten check covered only token-free components, leaving
    `{yyyy}/..{mm}`, `{yyyy}/. {mm}` and `{yyyy}/{mm} ` silently rewritten — spec amended: the check
    applies to a component's literal outer edges wherever they are, and `  {yyyy}  ` is now refused
    (a deliberate reversal, recorded in the Spec Change Log).
  - `[medium]` `[bad_spec]` the collapse trim was gated on "something collapsed" rather than on what
    the collapse exposed, making a component's decoration title-dependent
    (`_{HH}{MM}_{slug}` → `_1432_standup` titled, `1432` untitled) and putting `renders_something` at
    odds with `render_component` over a filler-only literal (`{yyyy}/-`) — spec amended: only filler
    adjacent to a token that rendered empty is removed, and the predicate follows the same rule.

Cascading order: bad_spec findings exist, so the 9 patch and 1 defer findings are moot for this pass.
The defer finding was written to the deferred-work ledger. `review_loop_iteration` was incremented
to 2.

#### Findings held moot by the cascade (recorded so nothing is lost)

**patch (9)** — `[medium]` token names are collected between `{` and `}` without consulting
`is_illegal`, so `parse("{\u{202e}gpj}")` puts a right-to-left override into `UnknownToken`'s
message, which is the string 40.2 prints inline: the spoof moved from the folder name to the error
surface. `[medium]` `{slug}` is folded from the *raw* title and never passes the illegal-character
filter — safe today only because `char::is_alphanumeric` happens to exclude `Cc`/`Cf`, with no test
on that side to notice if `slug_stem` ever widens. `[low]` a leading space now defeats the absolute
check into the wrong reason (`"  /Users/x/{yyyy}"` → `EmptyComponent`, whose advice sends the user
looking for a doubled slash). `[low]` the module doc claims the illegal set covers everything "no
human can see" while it covers `Cf` only. `[low]` the `{seq}` join still inserts a space over the
separator the user wrote (`2026-08-05- (2)`). `[low]` `de_reserve` still rebuilds from the untrimmed
stem (`nul -rec.txt`). `[low]` `fit_component` still shares one character cap across multiple title
tokens and over-cuts. `[low]` `RenderCtx` is still unvalidated and `seq: 0` still renders identically
to `seq: 1`. `[low]` doc/test debt: `{slug}`'s 60-character cap lives in the notes domain and is
invisible from this token table, `the_slug_matches_what_the_notes_domain_would_produce` asserts less
than its comment claims (the two folds deliberately diverge for empty and reserved-name titles), no
test ties the shared date tokens to `journal_path`, and `notes/naming.rs:96` still emits one new
rustdoc warning.

**reject (3)** — trimming a leading `-` out of a *title* (it would strip legitimate hyphens from
real titles, and the leading-`-` hazard is already recorded against `RenderCtx`); re-rendering with a
clamped literal so a title survives a component whose literals alone exceed 255 bytes (a template of
300 literal dashes is pathological and the clamp already yields a legal name); and `{slug}`'s
60-character cap counted a second time as a finding separate from the doc-debt item.

### 2026-08-06 — Review pass 2 (bad_spec loopback 1)

- intent_gap: 0
- bad_spec: 5: (high 2, medium 2, low 1)
- patch: 12: (high 0, medium 0, low 12)
- defer: 2: (high 0, medium 1, low 1)
- reject: 4
- addressed_findings:
  - `[high]` `[bad_spec]` `{seq}` outside the leaf suppresses the leaf ordinal and renames an
    ancestor directory (`2026 (2)/…`) or changes the path's depth on collision (`2026/(2)/08-05`) —
    spec amended: `parse` refuses `{seq}` anywhere but the final component, with its own typed
    reason; new Design Note, Execution task and Acceptance Criterion.
  - `[high]` `[bad_spec]` the 255-byte cap covered the leaf only, leaving interior components able
    to render past `NAME_MAX` with no way for 40.3's retry loop to recover — spec amended: the cap
    applies to every rendered component; Design Note, task and Acceptance Criterion updated.
  - `[medium]` `[bad_spec]` Unicode format characters (`Cf`) passed both the template validator and
    the title filter, admitting invisible folder names and the RTL-override spoof — spec amended:
    `Cf` joins `Cc` in the illegal set, on both sides.
  - `[medium]` `[bad_spec]` template components were silently rewritten at render (`{yyyy}//{mm}`,
    `{yyyy}/  ..  /{mm}`, `{yyyy}/..a`) against "validated, never sanitised" — spec amended: parse
    refuses an empty component and one whose literal is `.`/`..` net of filler.
  - `[low]` `[bad_spec]` the collapse rule welded two neighbours that both rendered
    (`{dd} {slug}{title} {HH}` → `0514`) — spec amended: the rule is now remove, merge adjacent
    filler runs, trim edges.

Cascading order: bad_spec findings exist, so the 12 patch and 2 defer findings are moot for this
pass — the code is re-derived against the amended spec and they will be re-found or gone.
The two defer findings were still written to the deferred-work ledger, because both are decisions
for story 40.3 rather than for this module and would otherwise be lost. `review_loop_iteration` was
incremented to 1.

#### Findings held moot by the cascade (recorded so nothing is lost)

**patch (12, all low)** — `seq: 0` renders byte-identical to `seq: 1`; `RenderCtx`'s fields are
`pub` and unvalidated, so `year: -5` renders a component starting with `-` that `rm` and `git add`
read as a flag; the `{seq}` join inserts a space over the separator the user wrote
(`{yyyy}-{mm}-{dd}-{seq}` → `2026-08-05- (2)`); `de_reserve` guards on a trimmed stem but rebuilds
from the untrimmed one (`"nul .txt"` → `nul -rec.txt`); `OptionalLeaf("")` prints advice a trailing
separator cannot follow; the brace grammar is asymmetric and unescapable and its error strings are
40.2's UI copy (`{{yyyy}}`, `{ yyyy }`, a bare `}`); a leading space defeats the absolute check
(`" /Users/x/{yyyy}"` renders `Users/x/2026`); `fit_leaf` over-cuts when the leaf holds more than
one title token; no test ties the four shared tokens to `journal_path` though the doc's stated
purpose is that they cannot drift; the token table omits every truncation rule and the module has no
doctest; `render` pairs `components.last()` with `rendered.last_mut()` on a coupling no test pins;
and `notes/naming.rs:96` emits one new rustdoc warning.

**reject (4)** — the claim that the doc's shared-vocabulary statement is already false (the
counterexamples are grammar differences, not vocabulary: the four shared tokens do mean the same
thing in both); clamping out-of-range civil fields inside `render` (a shell that reports month 13
has a bug that silent normalisation would hide); and two findings that restate others already
counted (`{}`/bare-`}` as separate from the brace-grammar item, and a format-character-only template
as separate from the `Cf` item).

### 2026-08-06 — Review pass 1

- intent_gap: 2: (high 2, medium 0, low 0)
- bad_spec: 5: (high 0, medium 4, low 1)
- patch: 8: (high 0, medium 0, low 8)
- defer: 3: (high 0, medium 1, low 2)
- reject: 6
- addressed_findings:
  - none

Cascading order: an intent gap exists, so every lower finding is moot and none were
addressed. `review_loop_iteration` was not incremented — no bad_spec loopback was
triggered.

#### The intent gap (both findings, one root cause)

The `<intent-contract>` permits a template whose **final** component is built only from
optional tokens, and specifies no behaviour for the case where that component renders to
nothing. Two rows require this to be possible:

- the *Hostile title* row fixes `{yyyy}/{title}` as a template that must parse and render;
- the *Default, untitled* row establishes an untitled recording as a first-class case.

Together they describe a state the contract never resolves. Reproduced by executing the
committed code (ctx = 2026-08-05T14:32:07, `title: None`):

1. **A fully collapsed leaf makes a parent directory the session folder, and the collision
   ordinal renames that parent.** `{yyyy}/{title}` renders `2026`, then `2026 (2)`,
   `2026 (3)`. `{yyyy}/{mm}/{slug}` renders `2026/08`, then `2026/08 (2)`. Under story 40.3
   an untitled recording would write its manifest and media directly into the year
   directory that is meant to *contain* sessions; the next titled recording would then
   create a session folder inside it; and because the year directory always already exists,
   the retry loop fires on every untitled recording and grows `2026 (2)`, `2026 (3)`, …
2. **With an explicit `{seq}` in a collapsible leaf, seq 2 renders a child of seq 1.**
   `{yyyy}/{slug}{seq}` renders `2026` at seq 1 and `2026/(2)` at seq 2 —
   `"2026/(2)".starts_with("2026/")` is `true`. A collision ordinal exists to produce a
   *sibling*; here session 2 is created inside session 1, so their media interleave and
   deleting session 1 deletes session 2. `{yyyy}/{slug} {seq}` behaves identically.

Finding 2 is wrong under every reading of the epic, and neither can be fixed without a
decision that belongs to the intent contract. The three candidate resolutions are mutually
exclusive and the contract supports none of them as written:

- **(a) Reject leaf-optional templates at parse time.** Coherent, and reuses the existing
  `MayRenderEmpty` vocabulary — but it makes `{yyyy}/{title}` invalid, directly
  contradicting the *Hostile title* row.
- **(b) Keep the current behaviour and let story 40.3 refuse the path at pre-flight.**
  Requires the contract to state that a rendered path is not necessarily a session folder,
  which defeats the epic's premise that a recording lands somewhere findable.
- **(c) Substitute a placeholder for a vanished leaf.** Explicitly refused by the epic and
  by this spec's own Design Notes ("inventing an 'Untitled' placeholder is exactly what
  this epic refuses").

Choosing between (a), (b) and (c) — and amending the I/O matrix to match — is a human
decision. It is not inferable: there is more than one defensible reading.

#### Findings held moot by the cascade (recorded so nothing is lost)

Not written to the deferred-work ledger, because the story will be re-derived once the
intent gap is resolved and these would then be duplicates.

**bad_spec (5)**

- `[medium]` The 80-**character** title cap does not bound the rendered component in
  bytes. Measured on `{yyyy}/{yyyy}-{mm}-{dd} {HH}{MM} {title}`: an 80-character Japanese
  title yields a 256-byte leaf, 260 bytes at seq 9, against a 255-byte `NAME_MAX` on APFS,
  ext4 and exFAT. Because the retry loop only *lengthens* the name, 40.3 could never
  recover from the resulting `ENAMETOOLONG`. The Design Notes fix a char cap and the
  guarantee list says nothing about bytes.
- `[medium]` `render` silently rewrites template *literals*, contradicting the module doc's
  "never quietly rewritten into a path the user did not ask for". Measured: `{yyyy}/..a` →
  `2026/a`; `{yyyy}//{mm}` → `2026/08`; `{yyyy}/` → `2026`; `{yyyy}/  ..  ` → `2026`.
  `ParentComponent` is an exact `.`/`..` match, so everything else falls to silent edge
  trimming. Story 40.2 shows a live preview beside the raw field; the two will disagree
  with no error to explain why.
- `[medium]` Unicode **format** characters (`Cf`) pass both the template validator and the
  title filter: `is_illegal` covers `Cc` via `is_control()` only. Measured:
  `PathTemplate::parse("\u{feff}")` succeeds and renders a folder whose entire name is an
  invisible character; the title `annual\u{202e}gpj.review` renders verbatim, which Finder
  displays as `annualweiver.jpg` — the standard RTL-override spoof. The property sweep's
  `FRAGMENTS` already contains U+200B, so the test suite *certifies* the acceptance. The
  contract's illegal set is defined as filesystem-refused characters, which `Cf` are not —
  widening it is a spec decision.
- `[medium]` The property sweep never renders an untitled recording: all 14 000 renders pass
  `Some(title)`, so `title: None` is exercised only by four hand-written examples. And
  `assert_legal` asserts only the *upper* bound on component count, no byte bound, and
  nothing about one render being a path-prefix of another — which is exactly why both
  intent-gap findings survived it. The acceptance criterion enumerates only properties that
  already hold.
- `[low]` A collapsing token can eat a separator run belonging to a neighbouring
  non-collapsing token, and two adjacent optional tokens eat both runs. Measured:
  `{dd} {slug}{title} {HH}` untitled → `0514`, not `05 14`. This follows the Design Notes'
  rule to the letter ("the preceding one if there is one, otherwise the following one"),
  which is what makes it a spec-level issue rather than a coding slip.

**patch (8, all low)**

- `RenderCtx`'s fields are `pub`, unvalidated, with no constructor and no `debug_assert`;
  `year: -5` renders a component starting with `-` (read as a flag by `ls`, `git add`,
  `rsync`), and any year outside `0..=9999` breaks the fixed-width chronological sort that
  justifies the epic.
- `seq: 0` renders byte-identical to `seq: 1`, so a 0-based caller silently reuses the
  first path. `debug_assert!(seq >= 1)` would pin the documented 1-based contract.
- The non-empty-path invariant is accidental: it holds only because `is_edge_noise` is a
  strict subset of `is_filler`, a coupling neither predicate documents and no test pins.
  Adding `'-'` to `is_edge_noise` alone would let `-{slug}` render `""`.
- `de_reserve` guards on a trimmed stem but rebuilds from the untrimmed one, so the title
  `"nul .txt"` renders `nul -rec.txt`.
- No test ties this token vocabulary to `journal_path`, though the doc's stated purpose is
  that the two "cannot drift silently". One assertion that the shared tokens render
  identically in both is the only cheap thing that would actually catch the drift.
- The brace grammar is asymmetric and unescapable, and its error strings are 40.2's inline
  UI copy: `{{yyyy}}` reports `{{yyyy} is not one of the tokens…` (mismatched braces inside
  the message), a bare `}` is a silent literal, `{ yyyy }` is rejected with no hint that
  the spaces are the problem, and there is no escape for a literal `{`.
- No doctest anywhere in the module, so every prose claim in the token table and the
  seven-bullet guarantee list is unverified by `cargo test` — including the two this review
  showed to be false.
- The story introduces one new rustdoc warning: `notes/naming.rs:96` — "public
  documentation for `slug` links to private item `slug_stem`". (The crate already emits 61
  rustdoc warnings, so this is not a gate; it is still new.)

**defer (3)** — pre-existing or downstream, not caused by this story

- `[medium]` `{title}` is neither case-folded nor NFC-folded. On case-insensitive APFS
  `2026/Standup` and `2026/standup` are one directory, so story 40.3's existence probe must
  compare case- and normalisation-insensitively, as `notes::naming::note_filename` already
  does, or it will judge an occupied path free.
- `[low]` Nothing bounds total path length or depth. `{yyyy}/{mm}/{dd}/{HH}/{MM}/{SS}
  {title}` with a long title passes Windows' 260-character `MAX_PATH` without long-path
  opt-in; a template of 5 000 separators is accepted and would have 40.3 create 5 000
  nested directories.
- `[low]` `notes/templates.rs:164` documents `MM` as month and `mm` as minute for its
  `{{date:FMT}}` field — inverted relative to this module, and each doc flags itself as the
  pair to read twice without mentioning the other. They are genuinely different dialects
  (moment-style format codes vs keeper placeholder tokens), so this predates the story, but
  two inverted conventions in one settings surface will be read as a bug.

**reject (6)** — duplicate ordinal on a parent (subsumed by intent-gap finding 2);
duplicated `{seq}` accepted; the title cap yielding 79 characters when the 80th is a space;
two vacuous tests; `'\0'` listed redundantly beside `is_control()`; the claim that
`RESERVED_DEVICE_NAMES` should contain `com0`/`lpt0` (Windows reserves only 1–9).

