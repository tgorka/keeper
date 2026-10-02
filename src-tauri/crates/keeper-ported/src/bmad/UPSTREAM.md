repository: https://github.com/bmad-code-org/BMAD-METHOD
commit: 05bfbd46d00766ec88eb9b42e76be2c575d64d7b (tag v6.12.0)
licence: MIT
copyright: Copyright (c) 2025 BMad Code, LLC
files read: src/scripts/config_utils.py, src/scripts/tests/test_config_utils.py, LICENSE, TRADEMARK.md
ported: ConfigError, load_toml, _detect_keyed_merge_field, _merge_arrays, structural_merge, merge_layers, load_customization (`config.rs`)
not ported: load_central_config (94.1, which owns the four central layers)
changed: Python dict/list became `toml::Value`/`toml::Table`; errors are `ConfigError` whose `Display` is upstream's message; `load_toml` takes `required` positionally; a parse error's detail is the `toml` crate's wording rather than `tomllib`'s, and a layer that is not UTF-8 is a "failed to parse" error rather than an uncaught `UnicodeDecodeError`; upstream's "did not parse to a table" cannot occur, because the parse target is a table.
revisit: main@4f61d4e769e50bc11d0d5d724f48942aac699679 moved the scripts to `skills/bmad/scripts/` and dropped `_bmad/config.user.toml` from the central layers. `structural_merge` is unchanged there. Re-port when keeper's `_bmad/` moves past v6.12.0 (C7; Epic 94, story 94.1).

# bmad

The tag `v6.12.0` is an annotated tag object (`b28fef56…`) pointing at the commit above, which is
what this repository's `_bmad/` runs: `_bmad/scripts/config_utils.py` is byte-identical to the
tag's `src/scripts/config_utils.py`.

The test fixture `tests/fixtures/bmad/bmad-agent-architect/customize.toml` is BMAD's own file,
copied verbatim from the installed v6.12.0 release under the MIT licence above. Its golden,
`winston-resolved.json`, was printed by BMAD's `resolve_customization.py` (see the fixture's
README); the script is not kept in this repository.

## Trademark

BMad™ is a trademark of BMad Code, LLC, not licensed under MIT. keeper uses the name only to
describe compatibility ('compatible with BMad Method'), never as a product, feature or UI name.
