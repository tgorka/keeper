# Spec 52.9 — Fractions, and an icon at the root

story: 52.9
status: review
branch: `work/epic-52-fractions-and-icon` (on top of `work/epic-52-file-row-menu`)
baseline_revision: c873fa6
final_revision: ''
binds: FR-313; AD-34-8, AD-34-9
sentinel: `MUT52-9`

<intent-contract>

**Two asks, verbatim.** *"w sync files nie mozna dac przy track files at or above
ulamkowych wartosci"* and *"w glownym folderze w repo stworz favicon.ico (ico
format z tego co jest w svg)"*.

**The threshold.** `add-folder-form.tsx:1271-1282` is a `type="number" min={0}`
input with NO `step`. HTML's implicit step is 1, so `1.5` is a stepMismatch, and
because the control sits in a real `<form>` with a native submit and no
`noValidate`, WKWebView blocks the submit outright. Everything downstream already
handles fractions: `Number.parseFloat` at `:518-521`, and
`Math.round(mb * 1024 * 1024)` at `:862-865` keeps the `u64` integral. The same
bug has a worse face: a profile whose threshold is not a whole MB — the docs' own
256 KiB example, `docs/sync.md:371` — pre-fills as `0.25` and makes the form
unsaveable for ANY unrelated edit.

**The favicon.** The root has `favicon.svg` (the finished coloured tile) and
`favicon.png`, and `index.html:13-14` references both, relatively. There is no
`.ico` at the root. `src-tauri/crates/keeper/icons/icon.ico` exists for the
Windows bundle and already carries six sizes — 16/24/32/48/64/256 — produced by
`tauri icon`, which is the ONLY svg→ico converter present in this container
(ImageMagick, rsvg-convert, inkscape, resvg, icotool, cairosvg, Pillow and sharp
are all absent; probed).

**Always**
- The threshold accepts a fractional MB and stores the exact byte count. One
  rounding, where it already is.
- A profile with a sub-MB threshold is editable.
- The root `favicon.ico` is produced by the generator that owns the other root
  icons, from `favicon.svg`, and is bit-identical to the bundled one — zero extra
  rasterisation.
- `index.html` references it, and the generator's gate asserts both the file's six
  entries and that reference, because "present but unreferenced" is the exact
  failure that gate was written to catch.

**Block if**
- Nothing new. `AD-34-8/9`'s empty-means-keeper-picks semantics are untouched.

**Never**
- Never hand-run the conversion as the deliverable: the file must be reproducible
  from the script, or the gate and the generator disagree.
- Never convert from `mark.svg` — it is transparent and `currentColor`-driven, and
  would yield an invisible-on-dark icon, which is what the generator's own
  green-pixel check exists to catch.

</intent-contract>

## Code Map

| where | change |
|---|---|
| `src/components/sync/add-folder-form.tsx:1271-1282` | `step="any"` + `inputMode="decimal"`, the idiom `session-space-editor.tsx:566-568` already uses |
| `scripts/gen-mark-icons.ts:235-243,326-333,356-378` | a third root artefact: copy `icon.ico` out of the desktop set, plus an ICONDIR check |
| `index.html:13-15` | the `.ico` link, relative, ordered so the vector still wins where understood |
| `scripts/gen-mark-icons.test.ts:206-270` | the gate: six entries, and the reference |

## Tasks & Acceptance

| # | acceptance |
|---|---|
| 1 | the threshold input carries `step="any"`; a test asserts the attribute, not just a whole-number round-trip, because jsdom implements no constraint validation |
| 2 | `1.5` saves `1572864` bytes |
| 3 | a profile at 262144 bytes pre-fills as `0.25` and the form saves an unrelated change |
| 4 | `favicon.ico` exists at the repo root and its ICONDIR holds 16/32/48/256 |
| 5 | it is byte-identical to `src-tauri/crates/keeper/icons/icon.ico` |
| 6 | `index.html` references it relatively, and the generator's gate fails if either the entries or the reference goes missing |
| 7 | the settle and poll inputs get the same `step` treatment, named here rather than left as a silent widening |

## Design Notes

_(filled at review)_

## Verification

_(filled at review)_
