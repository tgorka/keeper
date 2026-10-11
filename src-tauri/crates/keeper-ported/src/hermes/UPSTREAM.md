repository: https://github.com/NousResearch/hermes-agent
commit: bfc7152687277dd877734e8e33ad0dd6bbbfa07d
licence: MIT
copyright: Copyright (c) 2025 Nous Research
files read: LICENSE, tools/memory_tool_store.py, tools/threat_patterns.py, tests/tools/test_memory_tool.py, agent/background_review.py (the three review prompts and the blocks they are built from), agent/curator.py (DEFAULT_STALE_AFTER_DAYS/DEFAULT_ARCHIVE_AFTER_DAYS :33, apply_automatic_transitions :209-263), tests/agent/test_curator.py, tests/agent/test_curator_activity.py, tools/skill_usage.py (the three states :35)
ported: ENTRY_DELIMITER, _parse_entries (with _read_raw_checked's BOM strip), the load's dedupe, _char_count, _find_unique_match, _pinned_index, _stale_entry_message, MemoryStore's add, replace, remove, _locate, resolve_entry, _edit, _apply_batch_op, apply_batch, resolve_batch_entries, _batch, _consolidation_failure and the per-turn cap, _failure_with_entries, _batch_failure, _success_response's fields, _usage and _usage_pct, _detect_external_drift's test, load_from_disk's _sanitize (`memory.rs`); _PATTERNS with their scopes, INVISIBLE_CHARS, MAX_SCAN_CHARS, scan_for_threats, first_threat_message (`threats.rs`); _MEMORY_REVIEW_PROMPT, _SKILL_REVIEW_PROMPT, _COMBINED_REVIEW_PROMPT and their blocks, adapted (`review.rs`); DEFAULT_STALE_AFTER_DAYS, DEFAULT_ARCHIVE_AFTER_DAYS, the active/stale/archived states and apply_automatic_transitions' rule and counts (`curator.rs`)
not ported: _file_lock (keeper's one writer holds a memory file by construction: the consolidator under its lease, AD-401), the .bak drift snapshot (git keeps every version), _read_raw_checked's I/O and _write_file's atomic rename (the caller's, through keeper's own reader and session runtime), format_for_system_prompt and _render_block (keeper's prompt slot 2 renders memory, AD-363), the memory_tool JSON dispatcher and its aliases, the background-review write-origin gate (keeper stages every write; a review-origin replace or remove always waits for a person at consolidation, R127), and the load-time dedupe and over-cap acceptance (keeper refuses such a file whole, AD-364 and R123); the curator's scheduler and state file, its config getters, the first-sight seeding and bundled re-anchoring, the usage ledger, cron-reference resolution, archive_skill's I/O and the LLM consolidation fork (keeper's curator is a weekly host job under a lease that reads a skill's last change from git; the skills it manages are never offered, so no model reviews them)
changed: Python to Rust. A Store holds one file's text instead of a path, so upstream's re-read under the lock is a parse of that text; failures are a `Failure` whose fields are upstream's extra keys; budgets count Unicode scalars (Python's `len` on `str`); `\s` in every pattern reads as Python's (`str.isspace()`, U+001C–U+001F included); hardcoded_secret's lookahead is a case-sensitive check on each match, retried one character later when it rejects one; the drift sentence drops its `.bak` clause; the `[BLOCKED: …]` placeholder's last clause names `memory_propose` with op remove, or an edit of the file, in place of `memory(action=remove)`; `_sanitize` passes only an entry that is exactly that placeholder for the file with known finding ids, where upstream passes any entry starting with `[BLOCKED:` — a file could forge the prefix and append a payload (R204); the per-turn failure cap is one counter keeper's caller carries between its two per-file stores (`Store::set_consolidation_failures`), as upstream's one store counts both files (R204); the review prompts name keeper's tools and rules (list below); the curator's rows are handed in and its new states handed back, times in whole seconds since the epoch, and upstream's cron-referenced set is the caller's `protected` set
revisit: when hermes-agent changes tools/threat_patterns.py or tools/memory_tool_store.py, re-run generate.py at the new commit and diff the fixture and the sentences; when it changes agent/curator.py's windows or apply_automatic_transitions, diff `curator.rs` against it

# hermes

`memory.rs`, `threats.rs`, `review.rs` and `curator.rs` are modified files of hermes-agent; each says
so in its header with upstream's copyright. No code from any other project was read for this port.

## Licence

MIT License

Copyright (c) 2025 Nous Research

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Upstream tests, case by case

`tests/tools/test_memory_tool.py` has 57 cases in 18 classes. Each ported case is a Rust test
named `hermes_upstream_` + upstream's name minus `test_`, every assertion kept, inline in the
module that ports the function it calls.

- `TestScanMemoryContent` (5) → `threats::tests`: `clean_content_and_false_positives_pass`,
  `injection_and_override_blocked`, `exfiltration_and_secrets_blocked`,
  `persistence_patterns_blocked`, `invisible_unicode_blocked`.
- `TestMemoryStoreAdd` (3), `TestMemoryStoreReplace` (5), `TestMemoryStoreRemove` (2),
  `TestExactWholeEntryMatchPriority` (2), `TestMemoryConsolidationGracefulDegrade` (4) →
  `memory::tests`, all ported. `replace_same_across_single_batch_and_approval_replay`'s third
  surface (`apply_memory_pending`) is the batch with `matched_entry` set, which is what that
  replay calls.
- `TestMemoryStorePersistence` (2): `save_and_load_roundtrip` ported (text written, then read
  as a new store). Not ported: `deduplication_on_load` — keeper refuses a file holding a
  duplicate whole (R123; `keeper_core::agents::memory`'s
  `a_duplicate_or_an_invisible_character_is_refused_naming_the_entry`).
- `TestMemoryStoreCharLimitOnLoad` (1, two parameters): not ported — keeper leaves an over-cap
  file out of the snapshot whole and lists its entries (R123;
  `one_scalar_over_the_cap_leaves_the_file_out_and_lists_its_entries`).
- `TestMemoryStoreSnapshot` (1): mapped — the frozen snapshot is keeper's session context,
  pinned by `agent_turns::the_memory_snapshot_does_not_move_during_a_session` and
  `a_session_never_sees_its_own_proposals_in_memory`.
- `TestMemoryToolDispatcher` (5): `replace_missing_content_still_distinct_error` ported as the
  batch op it reaches. Not ported: `no_store_returns_error`, the two `new_text` alias cases and
  `content_wins_when_both_content_and_new_text_set` (the JSON dispatcher; keeper's tool
  arguments are `memory_propose`'s own).
- `TestMemoryBatch` (4): `batch_add_and_remove_atomic`, `batch_duplicate_add_is_noop_not_failure`,
  `batch_injection_blocked_rejects_whole_batch` ported. Not ported: `batch_new_text_alias_for_content`
  (dispatcher alias).
- `TestExternalDriftGuard` (4): all ported; the backup assertions became "the text is untouched"
  and the remediation field, as no `.bak` is written.
- `TestUnreadableFileDoesNotWipeMemory` (2): not ported — file I/O; keeper's reader refuses an
  unreadable or non-UTF-8 file before any store is built.
- `TestLoadTimeSnapshotSanitization` (3): all ported over `sanitize_for_snapshot`.
- `TestBomToleranceInMemoryFiles` (3): `bom_is_stripped_from_first_entry` and
  `bom_file_add_keeps_existing_entry_intact` ported; `invalid_utf8_still_reports_unreadable` not
  (file I/O, as above).
- `TestBatchRefusesToEmptyNonEmptyStore` (2, two parameters each): both ported.
- `TestMemoryFileLockPermissions` (3): not ported — no lock file.
- `TestBackgroundReviewDeleteGate` (6): not ported — keeper never writes memory from a
  session; every write is a proposal, and a review-origin replace or remove always takes the
  review path (R127).

Ported: 37 of 57 cases (5 + 3 + 5 + 2 + 2 + 4 + 1 + 1 + 3 + 4 + 3 + 2 + 2); mapped: 1; not
ported: 19 (1 + 1 + 4 + 1 + 3 + 2 + 1 + 6), each named above with its reason.

## Parity fixture

`tests/fixtures/hermes/scan.jsonl` is upstream's own `scan_for_threats` over a corpus (the
upstream test strings, one example per pattern boundary, full-width and compatibility forms, the
secret post-check's cases, U+001C between words), at `strict` and at `all`. It was generated
once, at the commit above, by

    python3 src-tauri/crates/keeper-ported/tests/fixtures/hermes/generate.py <hermes-agent checkout> src-tauri/crates/keeper-ported/tests/fixtures/hermes/scan.jsonl

and is read by `threats::tests::hermes_upstream_scan_matches_the_fixture`; no test runs Python.
The fixture is what found that `regex`'s `\s` leaves out U+001C–U+001F.

Not measured: Python's `\w` (`str.isalnum()` or `_`) and `regex`'s (`Alphabetic`, marks,
`Nd`, `Pc`, `Join_Control`) differ on marks, other numerics and connector punctuation; NFKC
folds most of the numerics first. Case folding is both engines' simple folding.

## Review prompts

`review.rs` keeps upstream's three prompts and their blocks sentence by sentence, except:

- **Tools.** `the memory tool with the matching target` → `memory_propose with the matching
  target`; `memory tool, target='user'|'memory'` → `memory_propose, target='user'|'memory'`;
  skill writes are `skill_propose` with op `create` or `patch` and the whole SKILL.md as body.
- **Staging.** The routing block gains "A proposal is staged, not saved: keeper applies it later,
  or a person does, and this session keeps the memory it opened with."; it drops "If the tool
  schema lists only one target …" (`memory_propose` always takes both).
- **Support files.** Keeper proposes SKILL.md only (support files are DW-560): option 3 "ADD A
  SUPPORT FILE …" is removed and option 4 becomes 3; "small `references/` set of topical depth"
  and the umbrella-hoarding clause leave the target shape; the lesson block's references/ bullet
  becomes "keeper proposes SKILL.md only, so depth that is only needed sometimes goes in a short
  topical section of it, never a '<date>-<incident>' section."; "search the skill (and its
  references/)" → "search the skill".
- **Loaded skills.** "loaded via /skill-name or you read via skill_view" → "you read via
  skill_view" (keeper has no slash command); the curator-managed clauses of option 1 go.
- **Read-before-write and protected skills.** Upstream's enforced `skill_manage` guard and its
  list of protected skills (bundled, hub, external, pinned, user-owned, `hermes curator adopt`)
  become keeper's two paragraphs: read-before-write through `skill_view`, the patch pinned to the
  SKILL.md it was proposed against (R132), and a person's skill (no `metadata.keeper_proposal`)
  changed only after that person's review. "If the only skills that need updating are protected,
  say 'Nothing to save.' and stop." goes with the list.

## Curator

`curator.rs` ports `apply_automatic_transitions` (`agent/curator.py:209-263`) and its two windows
(`:33`) as `transition` over one row and `apply_automatic_transitions` over many, with upstream's
counts. `tests/agent/test_curator.py` has 35 cases; `tests/agent/test_curator_activity.py` has 1.

- Ported (4): `pinned_skill_is_never_touched`, `cron_referenced_skill_by_name_survives_inactivity`
  (the cron set is the caller's `protected` set), `unreferenced_skill_is_still_archived`, and
  `test_curator_activity.py`'s `recent_view_activity_prevents_false_stale_transition` (upstream's
  ledger folds `last_viewed_at` into `last_activity_at`; the row is handed that value).
- Not ported (32): the config gates and bounded getters (`first_run_defers`, `set_paused_roundtrip`,
  the three `non_positive_*` cases, `bad_bounded_value_warns_once_per_distinct_value`: keeper's
  windows are upstream's defaults, not configurable); bundled, hub and built-in handling
  (`bundled_skills_are_off_limits_unless_opted_in`, `llm_candidate_list_omits_bundled_and_disabled_skills`,
  `llm_prompt_does_not_invite_bundled_writes_when_prune_builtins_on`,
  `prune_builtins_still_archives_bundled_via_deterministic_pass`,
  `protected_builtin_never_archived_even_when_stale`,
  `preseeded_never_used_builtin_is_reanchored_not_staled`, `prune_builtins_never_touches_hub_skills`,
  `cli_pin_refuses_bundled_skill`: keeper's curator manages only an agent's unadopted skills);
  cron-path resolution (`cron_referenced_skill_by_absolute_path_survives_inactivity`,
  `unresolvable_reference_is_kept_verbatim`: keeper names a skill by its folder name); the state
  file (`state_atomic_write_no_tmp_leftovers`, `run_review_records_state`); and the LLM review
  fork (`dry_run_injects_report_only_banner`, `run_review_synchronous_invokes_llm_stub`, the five
  `review_runtime_*`/`review_model_*` cases, the six `review_fork_*` cases,
  `threaded_llm_pass_keeps_callers_profile_scope`).

`curator_state_machine` pins keeper's reading of the rule at its boundaries (13, 14, 29, 30 days,
from active and from stale, a stale skill used again), for a skill used and one never used.
Keeper's adapter — which skills are managed, what counts as a use, what is protected, the stale
mark and the move — is `keeper_core::agents::curate`.
