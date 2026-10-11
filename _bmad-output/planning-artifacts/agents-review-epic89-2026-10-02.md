# Epic 89 — adversarial review (read-only; nothing run, nothing edited)

Base: `origin/main` (bec12596) via `git diff origin/main` and `/tmp/agents-salvage/epic89.diff`.
(Anything read earlier through the stale local `main` was re-derived; the one wrong conclusion — a
voices row in `folder.rs` — is dropped.) No cargo/bun/vitest was run; every "test passes" claim below
is by reading, not by execution. Shell-crate edits were checked by inspection only, as the repo rules
require.

## Findings

### MAJOR

**R-1 · major · redaction misses model-written fields other than `assistant.text`**
`src-tauri/crates/keeper-core/src/agents/log/writer.rs:355-385` (`redacted`)
Only `assistant.text`, `tool_result.content`, `user.text`, `peer.text` are passed through
`redact_secrets`. Not scanned: `tool_call.args` (`ToolCallBody.args`, the model's own output — a
model that read a `ghp_…` token in a tool result and then calls `drive_write`/`drive_grep` with it
puts the token into the chunk verbatim), `compact.summary` (a model-written summary of a
conversation that contained the raw secret — the stub test itself proves the model *sees* the raw
token), `peer.ask.question`, `error.sentence`, `approval.result`, `delegate.reason`, `run.detail`,
`surface.outcome`. Acceptance 12 cannot catch this: the stub's tool calls carry no secret.
Consequence: the exact S-17 outcome (a credential in a file that syncs to every host) through the
two fields a model most plausibly echoes into.
Fix: redact every string leaf of the body generically (serialize body → `Value`, walk strings,
redact, `LineBody::decode` back), or at minimum add `ToolCall.args`, `Compact.summary`,
`PeerAsk.question`, `ErrorBody.sentence`. Extend acceptance-12's stub so round 2's tool call
echoes the token in its arguments, and assert the chunk is clean.

**R-2 · major · the home fence does not cover what the soul reads, so a tool can rewrite slot 1**
`src-tauri/crates/keeper-sync/src/files_write.rs:447-470` (`in_agent_home`);
`src-tauri/crates/keeper-core/src/agents/prompt.rs:196-235` (`soul_slot`);
fixture `zone-ok/nixi/SOUL.md` (`persistent_facts: ["file:notes/standing-orders.md"]`),
golden `nixi-told.md` ("## Persistent facts / - From notes/standing-orders.md: …" inside
"# Who you are").
The fence refuses `agent.toml`, `SOUL.md`, `USER.md`, `MEMORY.md`, `journal/`, `proposals/` and
`_*`; the test explicitly routes `80-agents/nixi/notes.md` as writable. But a `file:` persistent
fact pulls `80-agents/nixi/notes/standing-orders.md` into **slot 1 of the system prompt as
instructions** (not under `FILE_CONTENT_IS_DATA`). Any bot with a write grant on the drive (today's
Epic 61 `drive_write`/`drive_edit`, or an agent whose `allow` includes them) can rewrite Nixi's
standing orders for every later turn — the exact thing `WriteRefusal::AgentHome`'s doc comment
says the fence exists to prevent, and a direct hole in FR-770 ("no tool can change an agent's
soul").
Fix (either, preferably both): (a) fence the whole home — `[_home, ..] => true` — the home is the
person's folder by AD-362, and amend the spec's "`80-agents/nixi/notes.md` is routed" row; (b)
render `RenderedFact::File` under the data sentence (slot 6 style) or refuse `file:` facts whose
target is tool-writable. Add a test: a `file:` fact's path is refused by `classify`.

**R-3 · major · a PEM block whose END marker was truncated away is written in full**
`src-tauri/crates/keeper-core/src/agents/redact.rs:78-81`
`-----BEGIN … PRIVATE KEY-----(?s:.*?)-----END … PRIVATE KEY-----` needs both markers.
`tool_result` bodies are truncated (`Truncated { shown, total }`), so a key file cut after its
header lands with all visible key material unredacted; the same for a `.pem` pasted without its
footer. Fix: `(?:-----END [A-Z0-9 ]*PRIVATE KEY-----|\z)` as the terminator, and/or a second
pattern for a BEGIN header followed by base64 lines. Add the truncated case to
`every_pattern_in_the_set_is_replaced_by_its_marker`.

### MINOR

**R-4 · minor · torn-tail repair only reaches today's chunk**
`writer.rs:98-120` (`open` → `newest(today)`). A host that died mid-line yesterday reopens today,
starts `<today>.<host>.1.jsonl` and never truncates yesterday's torn chunk: it stays torn in the
synced drive and every `read_session` on every host reports the problem forever. Fix: on open,
repair this host's newest chunk whatever its date (it is the host's own), then rotate.

**R-5 · minor · a newly created chunk's directory entry is never fsynced**
`writer.rs:154-162` (`open_chunk`, `!existing`), `sync()` at `:330-342` syncs file data only.
The blob path fsyncs `blobs/` (`:321-323`); the chunk path does not fsync `log/`. On a crash the
"fsynced" turn's chunk can be absent from the directory on filesystems that do not order the
create with the data (NFR-117). Fix: fsync `log_dir` after `create` in `open_chunk`, or in
`sync()` when a chunk was created since the last sync.

**R-6 · minor · symlink refusal is check-then-open (TOCTOU)**
`writer.rs:72-93` (`refuse_symlink`/`real_dir`), `:144-158` (`open_chunk`), `:306-311`
(blob dedupe). `symlink_metadata` then `open`/`create_dir`. The blob temp uses `create_new` (safe);
chunks and `log/` do not. Fix: after open, compare `file.metadata()` (dev, ino) with
`symlink_metadata(path)` and refuse on mismatch, or `OpenOptionsExt::custom_flags(O_NOFOLLOW)`.
Risk is low (needs a local writer racing keeper) — noted because the spec asks for it.

**R-7 · minor · `BLOB_OVER_BYTES` is not tied to `rotate_at`**
`writer.rs:28-30`, `:232-240`. `limit = min(64 KiB, rotate_at − 1)`. With `lfs_threshold_bytes`
below ~22 KiB a body ≤ 16 KiB (which the spec says is written inline) is refused `LineTooLong`;
`rotate_at(0)` refuses every line. Fix: refuse `rotate_at < 2 × BLOB_OVER_BYTES` in `open` with a
sentence, or blob at `min(16 KiB, rotate_at / 4)`.

**R-8 · minor · `append_line` never rotates on a date change**
`writer.rs:252-259`. It dates the line by the current chunk's date (else `today` from `open`),
so Epic 95's journal lines appended after midnight keep landing in yesterday's chunk, against
"a new chunk at a UTC date change". Fix: take a `NaiveDate` (or `DateTime<Utc>`) parameter.

**R-9 · minor · the epoch fence drops a legitimate `claim lost/released` line and cannot locate what it dropped**
`reader.rs:168-199` (`fence`). A loser's `claim lost` at epoch E−1 is necessarily written after
the winner's `acquired` at E, so it is dropped with "written after a newer epoch was acquired",
losing the one record that the loser noticed. Problems from the fence carry `chunk: "log/"`,
`line: None`. Fix: exempt `LineBody::Claim` from the fence; carry chunk and line number through
the merge so a dropped line is nameable.

**R-10 · minor · untrusted-zone globs are case-sensitive while the write fence folds case**
`label.rs:303-319` (`in_untrusted_zone`), vs `files_write.rs:447` ("compared folded … on the
case-insensitive volume keeper ships on"). `00-Inbox/x.md` reads `owner`. Fix:
`GlobBuilder::case_insensitive(true)` (and say so in `docs/agents.md`).

**R-11 · minor · `[[gate]]` (agent.toml) and `delegates` (session agent.toml) are not in the grammars**
`home.rs:326-338` (`ROOT_KEYS`), `session.rs:33-51`. The architecture's *Data formats* tables
list `[[gate]]` (kind `gate` only; `system`, `peer`, `audience`, `delegates`, `deadline_ms_max`,
`modes`, `max_tickets_per_hour`) and the session's `delegates`. A file written to the architecture
is refused "agent.toml has a key keeper does not know: gate". No gate runs before Epic 96, so no
user is hurt today, but the grammar contract and `docs/agents.md` should either parse it or say
plainly that `[[gate]]` is not read yet.

**R-12 · minor · reader skips a symlinked chunk silently; the secret test does not look in `blobs/`**
`reader.rs:88-92` (`file_type().is_file()` without a problem entry);
`tests/agents_log.rs:1048-1056` (iterates `log/*` files only). A blobbed body holding a secret
would pass acceptance 12. Fix: push a problem for a non-regular entry; recurse into `log/blobs/`
in the test.

**R-13 · minor · `quirks(OpenAi).done_sentinel = Yes` generalises one gateway to every OpenAI-compatible endpoint**
`bots/quirks.rs` (OpenAi row). `chat.rs` treats a stream ending without `[DONE]` as truncated;
the evidence is one CLIProxyAPI run. A compatible endpoint that omits the sentinel renders every
answer as cut off. Consider `Unknown` for the generic kind, or recording per-provider from the
*Test* probe.

**R-14 · minor · docs forecast a release number and cite a machine-local path**
`docs/agents.md:60-63`: "Requires keeper ≥ 0.9.1" — the current release is 0.9.0 and this rung is
unreleased; if the next release is 0.10.0 the sentence is false. The same paragraph cites
`/workspace/tgdrive/.keeper/keeper.toml:6-10`, a path that exists only on the operator's hosts.
Fix: fill the floor at release time (the release-notes step acceptance 14 already requires) and
cite the behaviour, not the path.

**R-15 · minor · `docs/sessions.md` says "rotated before 192 KiB"**
True only at the default LFS threshold; the rule is `min(192 KiB, 3/4 × lfs_threshold_bytes)`
(`docs/agents.md` has it right).

**R-16 · minor · `redact_secrets`' compile-failure fallback mislabels its finding**
`redact.rs:108-116`: `kind: SecretKind::PrivateKey` for the whole-text marker. Dead code today
(const patterns), but a reader of `found` would be told a private key was seen. Add a
`SecretKind::Withheld` or similar.

**R-17 · minor · agentskills port trims the name before validating; upstream not confirmed to**
`keeper-ported/src/agentskills/mod.rs:96` (`raw.trim().nfkc()`). WebFetch would not reproduce
upstream `validator.py` verbatim, so I could not confirm whether upstream strips; a name
`" my-skill "` may be valid here and refused there (or the reverse). Verify against the pinned
commit and record the answer in `UPSTREAM.md` `changed:`.

**R-18 · minor (forward) · replay drops `user.attachments` and `peer.ask`**
`replay.rs:69-71`. Fine for the text turns 89.5 proves; once 90.5 logs an attachment, the
replayed user message will differ from the sent one (image parts). Note it in `message_for` so
90.5 extends both sides together.

**R-19 · minor · a drive-committed symlink can redirect a tool write past the fence**
`files_write.rs:586` classifies the *requested* subpath after `resolve_existing` canonicalises;
`engine.rs:2420` stages symlinks, so a reader of a shared drive can commit `10-notes/x.md →
../80-agents/nixi/SOUL.md` and a bot's vault write to `x.md` follows it. A reader could edit
`SOUL.md` directly anyway, so this adds little power; recorded because the brief asks. Fix:
classify the canonical path made root-relative, not the requested one.

## Checked and found clean

- **label.rs**: exhaustive 72-label lattice (commutative/associative/idempotent, identity,
  absorbing), join = readers ∩ / integrity min / `local_only` OR, `may_reach` = audience ⊆
  readers, `may_use_model`, OKF `human_reviewed`/external-source rules, serde shapes (sorted
  readers, `"*"`, `local_only` omitted when false; `"all"`, bad id, duplicate, unknown word refused).
- **drive.rs**: every key typed and bounded; `version > 1` → "written by a newer keeper"; id/
  principal patterns; owner ∈ readers; sorted, duplicate-free readers as Matrix ids; `[integrity]`
  replaces the default whole, bare table/empty list = none, bad glob refused naming it, unknown
  integrity key refused. **zone.rs**: `_*`, `README.md`, `AGENTS.md`, dotfiles never homes;
  sentences match the spec.
- **home.rs**: every `[limits]`/`[memory]` bound equals the architecture table (1–8, ≥0, ≥1000,
  0–3, 1–3, 1–16; 0|5–50, 0|5–100); `human` rules; `[model].bot` through `parse_base_url` (no
  userinfo), empty target refused, `openai` accepted; `local_only` forcing names the drive's key;
  `mcp:` sentence; closed vocabulary incl. `kvm_*`; steward = specialist + `delegate` (C8/F18);
  menu rules; `id == folder`, `_` reserved; shared `matrix_user` refused naming both.
- **soul.rs**: all bounds; 16 KiB with size; block scalar refused naming key and fix; `file:` facts
  refuse `..`, `/`, `{}`, `:`, `\`; `soul_from_bmad` lists everything not imported; both goldens
  (`nixi-told.md`, `winston-SOUL.md`) are compared to committed files, not regenerated.
- **memory.rs**: independently recounted the fixtures (USER 1375 / MEMORY 2200, 14 / 22 entries);
  `§` line trimmed; count = entries + `\n§\n`; invisible-format set and duplicate rule; C3 (left
  out whole, listed, never truncated); digest over canonical join, host-independent.
- **prompt.rs**: slots 1–6 in order; `FILE_CONTENT_IS_DATA` closes slot 5 before the context
  files; `told` text == prompt text; digests as documented.
- **session.rs**: kind incl. `conversation`, title ≤120, hop 0–3, parent shape, room/user parsing,
  label with `local_only` only when true, unknown keys named, compose→parse round-trip.
- **log/mod.rs**: key order `v,id,parent,ts,host,epoch,claim,kind,matrix_event,body`;
  `ts` millis `Z`; `deny_unknown_fields` on every body; blob ref recognised only as the exact
  2-key object; `ChunkName` refuses padded `n`; v2/unknown kind are problems, not panics.
- **writer.rs** (beyond R-4…R-8): one `write_all` per line; blob written to a `create_new` temp,
  fsynced, renamed, dir fsynced **before** the line; chunk stays strictly under `rotate_at`;
  `ForeignHost` refused; redaction before blobbing; receipt carries the redacted inline line.
- **reader.rs/replay.rs**: merge by (ts, host, id); double-acquire → `conflicted` and replay
  refuses; same event twice is not a conflict; `hydrate_blob` validates name and hash; `compact`
  substitution.
- **index.rs**: `<zone>/.keeper/agents.db`, WAL, `user_version` mismatch drops and rebuilds;
  `**/.keeper/**` is excluded by `exclude.rs:166` at any depth, so the db and its WAL never sync;
  rebuild/apply/cards/seen as specified; test really renames `log/` away.
- **files_write.rs** (beyond R-2/R-19): `in_agent_home` folds ASCII case, ignores empty segments,
  covers `_*`, the four files, `journal/**`, `proposals/**`; `..`/absolute/platform separators are
  refused upstream by `resolve_existing`/`plain_segments`; unaware scope refuses nothing new;
  armed in the one bot write path (`bots_tools.rs:434`, after `with_sessions`), and deliberately
  not in the person's Files pane (`sync_ipc.rs:4764`).
- **profile/mod.rs, folder.rs**: `AgentsConfig` mirrors voices; escape refusals name the field;
  overlap refused in both directions against all five zones; `AGENTS_NEED_SESSIONS` enforced in
  `validate` (so a save that turns sessions off is refused); folder-file row + tests; old rows load.
- **keeper-ported**: compared `bmad/config.rs` line by line with the installed
  `_bmad/scripts/config_utils.py` (byte-identical to the tag per `UPSTREAM.md`): keyed detection
  over `base + override`, `code` then `id`, string/empty checks in upstream's order and wording,
  in-place replace/append, table recursion, scalar override, `load_toml` messages,
  `load_customization` layers — faithful. Both `UPSTREAM.md` carry all nine key lines, licences
  on `deny.toml`'s allow list, the §4(b) header, the trademark paragraph verbatim;
  `tests/upstream.rs` reads `deny.toml` and refuses `GPL-3.0`; `check:ported-pure` grep would
  catch `tokio`; crate deps are `toml` + `unicode-normalization` only. agentskills messages match
  upstream's as summarised (see R-17 for the one unverified point).
- **89.6 bots**: every `match` on `ProviderKind` decides `OpenAi` (`mod.rs`, `quirks.rs`,
  `discover.rs` ×4, `grant.rs`, `commands.rs`, `voice_target.rs`, `home.rs::serves_local_models`);
  `openai_models` → `hermes_vm(id, None)` leaves vision/tools/reasoning `None` (never
  `Some(false)`); `health` sends the bearer to `/v1/models`; probe is a membership test with no
  `/p/` prefix (test asserts all requests are `GET /v1/models`); 401 sentence; token asserted
  absent from the error; egress test: exactly one "AI provider" row for `provider.example`; no
  tailnet host in `src-tauri/crates`, `src`, `dev`, `docs` (the only `siren-alsephina` hit is the
  pre-existing `setup_qr.rs` fixture, unrelated); account round trip (device file → `openai`
  provider; `omp` still refused); live test `#[ignore]` reason exact, endpoint/token from env,
  token searched for in captured tracing.
- **Shell crate (by inspection)**: `bot_task.rs` drops the `ProviderKind` import and has no
  remaining non-test use; `bots_ipc.rs` already imports `discover`; both swaps have the right
  polarity (`!probes_model_capabilities`); every `SyncProfileVm`/`SyncProfileReq`/
  `FilesFolderRoles` literal (forge_ipc:480, sync_ipc:5661/3753, vm.rs tests) carries the new
  fields; `SessionsConfig::default()`/`AgentsConfig::default()` exist; `account_restore` literal
  uses `..original`; `DriveRecord.agents` mapped; generated TS bindings updated.
- **UI**: the agents switch is inside `{form.sessions && (` (absent, not disabled); the request
  carries `agents` even while hidden so Rust's sentence appears; folder-file lock; `bot-grant-bar`
  is a `switch` with `satisfies never`; `KINDS` has three; copy names three kinds truthfully;
  mock-shell openai fixture uses an `example` host.
- **Docs**: `WriteRefusal::AgentHome`, `AGENTS_NEED_SESSIONS`, the torn-tail sentence and the
  label sentences in `docs/agents.md` are byte-identical to Rust; measurement numbers are
  recorded and the re-run command names an existing `#[ignore]` test; S-31 sentence present;
  "control metadata is not labelled" present; redaction chapter lists the set and DW-430.
- **Tests**: acceptance-7 is a real end-to-end — `run_tool_loop_reporting` against a local stub
  HTTP server, lines written through `ChunkWriter`, fresh `read_session`+`replay`, `build_body`
  compared byte for byte; no new `unwrap`/`expect` in production paths of the new modules.

Not verified: nothing was compiled or executed on this host; the Rust test files were read, not run.
