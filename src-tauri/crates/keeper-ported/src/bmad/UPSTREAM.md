repository: https://github.com/bmad-code-org/BMAD-METHOD
commit: 05bfbd46d00766ec88eb9b42e76be2c575d64d7b (tag v6.12.0)
licence: MIT
copyright: Copyright (c) 2025 BMad Code, LLC
files read: src/scripts/{config_utils,resolve_config,resolve_customization,render_skill,memlog}.py, src/scripts/tests/{test_config_utils,test_resolve_config,test_resolve_customization,test_memlog}.py, src/core-skills/bmad-party-mode/scripts/resolve_party.py, src/core-skills/bmad-party-mode/scripts/tests/test_resolve_party.py, LICENSE, TRADEMARK.md
ported: ConfigError, load_toml, _detect_keyed_merge_field, _merge_arrays, structural_merge, merge_layers, load_central_config, load_customization, and the resolvers' extract_key and --key output (`config.rs`); render_skill's four token grammars, _lookup, _require_*, _resolve_config_value, _find_config_values/_resolve_short_config, _format_markdown_list, _format_review_layers, _resolve_customization_value, _resolve_replacements, _render_sources, render's identity, generation hash and manifest, _verify_existing (`render.rs`); memlog's split, render, touch, entry_count, ack, cmd_init, cmd_append, cmd_set and resolve (`memlog.rs`); resolve_party's _alias, build_collective, resolve_members, group_menu, find_group, group_detail and main's three projections (`party.rs`)
not ported: resolve_customization's project-root discovery (find_project_root, script_project_root, candidate_project_roots, has_override, warn_on_masked_override: keeper never walks for a root, it reads the drive); every CLI's argparse and stdout reconfiguration; the file I/O of render_skill (_load_sources' reading and escape check, _publish's staging and rename), memlog (write_atomic, the fsync) and resolve_party (_run_json's subprocesses, load_workflow's fallback to the bare customize.toml) — each is the caller's, through keeper's own reader and session runtime; bmad-help.csv's catalogue (lands with its first consumer, the workflows rung)
changed: Python dict/list became `toml::Value`/`toml::Table` with `toml`'s `preserve_order`, so tables keep document order as dicts do (R98); errors are `ConfigError`/`RenderError`/`MemlogError`/`PartyError` whose `Display` is upstream's message; `load_toml` takes `required` positionally; a layer arrives as a `Layer` (a name and what the caller read), so keeper names drive-relative paths; a parse error's detail is the `toml` crate's wording rather than `tomllib`'s, and a layer that is not UTF-8 is a "failed to parse" error rather than an uncaught `UnicodeDecodeError`; upstream's "did not parse to a table" cannot occur; a corrupt existing manifest's detail is `serde_json`'s wording; `render`'s project root is a `ProjectRoot` (BMAD's absolute directory, or keeper's session, R96) and its renderer identity is a parameter (`renderer_sha256()` hashes `render.rs` as upstream hashes its own file); the clock is a parameter of the memlog commands; where resolve_party crashes on a value (a member code or name that is not text, `agents` or a member list that is a number) the port refuses with a sentence naming the field
revisit: main@4f61d4e769e50bc11d0d5d724f48942aac699679 moved the scripts to `skills/bmad/scripts/` and dropped `_bmad/config.user.toml` from the central layers; this port does not follow. `structural_merge` is unchanged there. Re-port when keeper's `_bmad/` moves past v6.12.0 (C7; Epic 94).

# bmad

The tag `v6.12.0` is an annotated tag object (`b28fef56…`) pointing at the commit above, which is
what this repository's `_bmad/` runs: `_bmad/scripts/{config_utils,resolve_config,
resolve_customization,render_skill,memlog}.py` are byte-identical to the tag's `src/scripts/`, and
the installed `bmad-party-mode` skill's `scripts/resolve_party.py` and its test to the tag's
`src/core-skills/bmad-party-mode/scripts/`. No code from any other project, and none under AGPL or
GPL, was read for this port.

## Upstream tests, case by case

Each ported case is a Rust test named as upstream minus `test_`, every assertion kept, inline in
the module that ports the function it calls (89.1's convention).

- `test_config_utils.py` (5) → `config::tests`: `structural_merge_recurses_appends_and_replaces_keyed_tables`,
  `non_string_keyed_identifier_is_rejected`, `present_malformed_optional_layer_is_rejected`,
  `missing_optional_layer_is_empty`, `filesystem_layer_precedence` (all four central layers, the
  custom user layer wins, and the three customization layers).
- `test_resolve_config.py` (4) → `config::tests::resolve_config`, as `load_central_config`,
  `extract_keys` and `to_json`, the functions the CLI calls:
  `full_and_repeated_key_output_follow_layer_precedence`, `malformed_present_layer_fails`,
  `writes_emoji_json_when_stdout_encoding_is_cp1252` (the dump keeps the emoji as itself). Not
  ported: `missing_tomllib_exits_with_actionable_version_error` (a Python interpreter check).
- `test_resolve_customization.py` (7) → `config::tests::resolve_customization`:
  `writes_emoji_json_when_stdout_encoding_is_cp1252`, `explicit_project_root_wins_and_stays_quiet`
  (the explicit root's overlay is read; the port never warns). Not ported:
  `missing_tomllib_exits_with_actionable_version_error` (interpreter), and the four cases of
  root discovery from the working directory or the script's path —
  `home_installed_skill_reads_project_override_not_home_bmad`, `project_installed_skill_still_resolves`,
  `walk_prefers_bmad_over_a_nearer_git_directory`, `notes_when_a_rejected_root_holds_the_only_override`
  — because keeper is always given the drive as the root.
- `test_memlog.py` (30) → `memlog::tests`, over an in-memory disk standing in for the files the
  CLI touches, with upstream's exit codes: 28 cases by name. Not ported: `test_target_is_required`
  (argparse refusing a command with neither `--workspace` nor `--path`; `Target` cannot be built
  without one) and `test_init_creates_missing_workspace` (directory creation is the caller's).
- `test_resolve_party.py` (17) → `party::tests`: 16 cases by name. Not ported:
  `test_passes_project_root_to_the_customization_resolver` (the subprocess argv; the port takes the
  merged table instead of running the resolver).

## Parity goldens

`tests/fixtures/bmad/expected/` holds what BMAD's own scripts printed (fixture README), compared
byte for byte, so key order is checked as well as values (R98): the central merge and its
`--key agents`, Winston's `--key agent`, `bmad-architecture`'s `--key workflow` with a real
overlay, the party projections, and the render of `bmad-build` and of a synthetic skill using
every token kind (replacements in first-seen order, every rendered file, `manifest.json`), plus
the render refusals' sentences.

## Deviations

- **`{project-root}` (R96).** Upstream binds every `{project-root}` to one absolute directory.
  keeper renders inside a session: `ProjectRoot::Session(<write root>)` binds a path to the
  session's write location (its `artifacts/`), except `{project-root}/_bmad/**`, which always means
  the drive's install, read where it is (`session_locations` states both for a path value). Both
  the value and the session root must be plain relative segments — no empty, `.` or `..`
  segment, no backslash, no drive letter, `{project-root}` only at the start — or the value is
  refused with "`config.<path>` must resolve inside this session: <value>". That is a grammar
  check; the caller still resolves each location through the drive's containment
  (`browse::resolve`) before reading or writing. Format-B skills therefore read their inputs only
  through the inputs a workflow declares.
- **Overlays (R97).** Upstream reads `_bmad/custom/` wherever the filesystem leads, symlinks
  included. keeper reads only what the drive holds: the caller builds each `Layer` through its
  own containment, an absent `_bmad/custom/` is two empty layers (`Source::Absent`), and a symlink
  out of the drive is never followed. `Layer::read`, the filesystem reader kept for 89.1's
  consumers (`agents new --from-bmad`, a person's act), follows upstream.
- **Key order (R98, R170).** `toml` is built with `preserve_order` so a merged table iterates as
  the Python dict does: the party's default room is the `[agents.*]` order, an ambiguity sentence
  lists paths in merged order, and a dump prints keys as `resolve_config.py` does. Cargo turns the
  feature on in every crate built with this one; keeper's own files are read in key order
  regardless (`keeper_core::toml_order`, R170), so only this module sees document order.
- **Python's own spellings.** `to_json` writes `json.dumps(…, indent=2, ensure_ascii=False)`'s
  bytes itself — a float as Python's `repr`, `NaN`/`Infinity`/`-Infinity` included — and refuses
  a TOML date or time with the `TypeError` the resolver dies of ("Object of type date is not
  JSON serializable"). The memlog's ack escapes as `json.dumps` does (DEL included), a refused
  `--field` is quoted by Python's `repr` (every character `str.isprintable()` refuses escaped;
  the table `py_printable.rs` is generated from Python's Unicode 16.0.0 database), and a memlog
  target is spelled as `pathlib` spells it. Party ids compare by Python's exact numeric `==`.
- **Renderer identity.** The manifest's `renderer_sha256` is the SHA-256 of `render.rs`, not of
  `render_skill.py`, so keeper's and BMAD's generations never collide; the parity goldens pass
  the Python file's hash explicitly.
- **Atomicity.** `memlog.py` writes a temp file, fsyncs and renames; the port returns the new
  text and the caller's session runtime owns the write and the fsync.

## Fixtures

Copied verbatim under the MIT licence above, from the tag (each was compared with the tag's file):
`bmad-agent-architect/customize.toml` (`src/bmm-skills/agents/`), `bmad-architecture/customize.toml`
(`src/bmm-skills/plan/`), `bmad-build/**` (`src/bmm-skills/ship/`), `bmad-party-mode/customize.toml`
(`src/core-skills/`). From this repository: `_bmad/config.toml` and `_bmad/config.user.toml`
(written by BMAD's installer), `render/_bmad/` (the same two with `[modules.gds]`'s three
duplicated keys removed), and `architecture.memlog.md` (the architecture's memlog). The operator's
makistack fleet layer, which tgdrive links as `_bmad/custom/`, supplied `_bmad/custom/config.toml`
and `_bmad/custom/bmad-architecture.toml`; they are makistack's files, not BMAD's. Written for these
tests: `_bmad/custom/bmad-agent-architect.toml`, `all-tokens/**` and
`render/_bmad/custom/all-tokens.toml`.

## Trademark

BMad™ is a trademark of BMad Code, LLC, not licensed under MIT. keeper uses the name only to
describe compatibility ('compatible with BMad Method'), never as a product, feature or UI name.
