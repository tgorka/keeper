repository: https://github.com/agentskills/agentskills
commit: 69ef37e9424c0a7ea9dd2293b559e43ec8176379
licence: Apache-2.0
copyright: none stated; skills-ref/LICENSE is the Apache-2.0 text with its appendix placeholder unfilled, and skills-ref/pyproject.toml names the author Keith Lazuka
files read: skills-ref/src/skills_ref/validator.py, skills-ref/tests/test_validator.py, skills-ref/LICENSE
ported: MAX_SKILL_NAME_LENGTH, MAX_DESCRIPTION_LENGTH, MAX_COMPATIBILITY_LENGTH, ALLOWED_FIELDS, validate_metadata with _validate_name, _validate_description, _validate_compatibility and _validate_metadata_fields (`mod.rs`)
not ported: validate (the directory walk) and parser.py (YAML frontmatter); keeper reads SKILL.md with its own frontmatter subset in keeper_core::agents::skills, which reproduces the three directory sentences ("Path does not exist", "Not a directory", "Missing required file: SKILL.md")
changed: Python to Rust. Fields arrive as `(key, MetaValue)` pairs rather than a dict, and the skill directory as its name rather than a Path; lengths count Unicode scalar values (Python `len`); case is `str::to_lowercase` (Python `lower`); NFKC comes from `unicode-normalization`; `isalnum` is `char::is_alphanumeric`. Every error sentence is upstream's, verbatim.
revisit: when the Agent Skills spec changes its allowed fields or limits, re-read validator.py at the new commit and update this record.

# agentskills

Apache-2.0 §4(b): `mod.rs` is a modified file. Its header says so, names the upstream file it was
modified from and how it was changed. No NOTICE file exists upstream, so §4(d) carries nothing.

The name is stripped, then NFKC-normalised, before it is checked, as upstream does it
(`validator.py:37`, `name = unicodedata.normalize("NFKC", name.strip())`); Rust's `str::trim`
and Python's `str.strip()` both remove Unicode whitespace.
