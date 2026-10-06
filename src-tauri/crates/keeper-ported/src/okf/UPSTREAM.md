repository: tgdrive (the owner's drive), `.okf/bin/` — okf, okf_lib.py, okf_index.py, okf_links.py
commit: 97ec9c38bd4b70022f46a1b9b35bbf556a3ef0d4 (drive HEAD; `.okf/` last changed at c656fc0c7f19fa95ecb3f1333ad4f39e319457b6, 2026-09-11)
licence: written from documentation; no upstream code is copied
copyright: the drive's scripts carry no licence and no notice (no LICENSE in the drive's root or in `.okf/`, no header in the scripts); the owner's statement is pending (OA-95-2), so ruling R137 option (b) applies: nothing of their code was read
files read: `.okf/OKF-0.2-digest.md`, `.okf/config.yaml` (its comments), the `okf` command's own usage text, generated `index.md` listings in the drive; the scripts were only run — their functions' names and signatures taken from `inspect`, their answers recorded by `tests/fixtures/okf/generate.py`
ported: the YAML subset of the config and of frontmatter (`yaml.rs`); load_config with bundles, exclude, guides, no_index and listed and the defaults of a key left out (`config.rs`); _match, is_excluded with guides winning, Config.bundle_for with the innermost bundle (`matcher.rs`); split_frontmatter, Doc.title, Doc.description, the reserved names (`doc.rs`); a reader of okf_index.render's listing grammar (`index.rs`); okf_links.resolve (`links.rs`) — each as behaviour, from the digest and the fixtures
not ported: okf validate, okf links' graph and RDF output, okf migrate, okf_index's walk and renderer (keeper reads listings, it never writes them: D-21), the predicate and annotation readers, IRIs and CURIEs
changed: Python to Rust, written without the source. Where the drive's fallback YAML parser and PyYAML disagree (a single-quoted `''`, a trailing ` # comment` on a plain value — `yaml.json`), this follows PyYAML, which the drive's tools use whenever it is installed; a timestamp stays a string where PyYAML builds a date; a non-string scalar in a config field is its text where Python keeps the number
revisit: when the owner states a licence (OA-95-2) — then (a): read and port the scripts, commit the drive's config as a fixture — or when `.okf/bin/` changes: re-run `generate.py` and diff the fixtures

# okf

`keeper-ported::okf` is how keeper reads an OKF drive the way the drive's own tools read it, for
`drive_search` (story 95.4, AD-403). The tools in `tgdrive:.okf/bin/` carry no licence, so under
ruling R137 (b) this module was written from the drive's documentation — the OKF digest and the
config's comments — and its every undocumented rule was settled by running the tools and
recording their answers. Not one line of the scripts was read.

## The fixtures

`tests/fixtures/okf/generate.py` writes inputs, runs the drive's tools over them and records the
answers; no test runs Python. Regenerate with:

    KEEPER_TGDRIVE=/path/to/tgdrive python3 src-tauri/crates/keeper-ported/tests/fixtures/okf/generate.py

| file | what the drive's tools answered |
| --- | --- |
| `config.yaml` → `config.json` | `load_config` over a config written for the fixture (none of the drive's content) |
| `yaml.json` | PyYAML and the fallback parser over that config, where they disagree |
| `match.jsonl` | `_match(pattern, path)` over 60 patterns × 71 paths |
| `paths.jsonl` | `is_excluded` and `bundle_for` over the fixture config |
| `drive-paths.jsonl` | the same over the drive's own config, for acceptance 1's 40-path table |
| `docs.jsonl` | `read_doc`'s meta, title, description and reserved flag |
| `links.jsonl` | `okf_links.resolve(base, target)` |
| `index.jsonl` | the listings `okf index` wrote for a written drive, and the documents in each folder |

The drive's own config is not committed (OA-95-2): `okf_matching_matches_the_drives_own_tools`
is `#[ignore]` and reads it from `$KEEPER_TGDRIVE`.

## What running the tools found

- **Q9 (epic 95): a `/**` pattern is a literal prefix.** `60-sessions/**/workspace/**` excludes
  `60-sessions/**/workspace/a.md` and no real session's workspace, as the config's own comment
  warns. keeper's own rule keeps every session workspace out of `drive_search` (OA-95-3).
- A pattern ending in `/` is a literal prefix too; a pattern without a wildcard is the whole path;
  `*` crosses `/` in a pattern holding a `/`; a wildcard pattern without one matches the file name;
  `[`, `]` and `\` are text.
- The fallback YAML parser keeps a trailing ` # comment` in a plain value and reads `''` in a
  single-quoted string as two quotes; the drive's config uses neither.
- `okf index` writes a listing for a bundle whose config says `index: false`.
