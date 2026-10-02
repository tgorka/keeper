# Findings — keeper agents planning set (epics 89–99), adversarial review

Scope: consistency, feasibility, build order. Code citations were checked against this worktree (`src-tauri/crates/**`, `/workspace/tgdrive`, matrix-sdk 0.18 sources under `/usr/local/cargo/registry`). Side note, not a finding: while this review ran, another session began stubbing `keeper-core/src/agents/*` (1-line files) and `crates/keeper-ported/` in this same worktree, uncommitted; the plan's "no `agents` module" was true at `origin/main`.

---

## Blockers

**F1 — blocker — The proxy DM's power levels lock the person out.**
`ARCHITECTURE-AGENTS.md:649-651`; `epic-90:383` (90.4 create-room row), `:402-405`; `epic-91:258` (91.5 makes the DM "typed `dev.keeper.agent.session`").
Every session room — the proxy DM included — is created with "creator 100, other agents 50, **people 0**; `events_default` is 50; `…approval.decision`, `…heard`, `…surface.result` … allowed at 0". The person must send `m.room.message` (every question to Nixi) and `dev.keeper.agent.scope` ("sent by the person's device (proxy DM)", `:662`), both at the default level 50. Epic 99 Q5 (`epic-99:85-86`) noticed the same trap for gate peers and wrote a different PL scheme for gate rooms; nothing does so for `main` sessions.
Fix: in *Matrix events* and 90.4's row, add per-type `events` entries `m.room.message: 0` and `dev.keeper.agent.scope: 0` for rooms whose session `kind` is `main` (or give the `human` level 50 there and keep observers at 0 elsewhere); 91.5 acceptance 5 must assert the person can post in the DM; 90.4 acceptance 2 must cover both shapes.

**F2 — blocker — 90.5's turn loop replays the log on every turn, which NFR-116/AD-365 forbid.**
`epic-90:469-470` ("**History:** `replay`"), `:445` (`run_agent_turn` "replays the log (89.5)"); vs `ARCHITECTURE-AGENTS.md:753` ("the turn loop and the board read the index and the writer's in-memory tail, and never re-read a log on the hot path") and NFR-116 (`:1191`, "a turn read the index and the in-memory tail only").
No story defines the in-memory history a served session keeps, when it is loaded (claim acquire / first turn), how `SessionWriter` appends to it, or a test that a second turn opens no chunk. 89.5 acceptance 9 proves only the index half.
Fix: 90.5 Files add `keeper_agent::agent::SessionContext { messages, memory_snapshot, label, … }` loaded once per (session, claim) via `replay`, appended by the writer; acceptance: "the second turn of a served session opens no file under `log/`" (fs-watch or injected reader counter). Replay stays the cold path (open, takeover, restart).

**F3 — blocker — Trust pinning: epics say "never trusted until a person pins", ruling R25 says TOFU.**
`agents-coordinator-decisions-2026-10-02.md` (R25, as it read before R28) ("`[[trust]].master_key` is optional (TOFU on first verified decision, then pinned)") vs `epic-90:314` ("when absent the user is 'not pinned yet' and is never trusted"), `epic-93:175` ("A pin is written into `agentd.toml` by a person, never by keeper"), `epic-93:185` (acceptance 7: "`not pinned` … a `differs` host accepts no decision"), `epic-93:264` ("Until a person is pinned, that host accepts no decision from them").
A binding ruling and the two stories that implement it disagree on whether a fresh agentd can ever accept an approval without an operator editing a file. 93.3's acceptance 7 would have to be rewritten under TOFU (first verified decision writes `master_key` — which also contradicts "never by keeper" and `agentd.toml`'s "nothing in the file is written by keeper" except `control_room` via `toml_edit`).
Decision needed: coordinator either rescinds R25's TOFU clause (then R25 is corrected) or 90.3/93.3 adopt it (then `init`/`status` write the pin with `toml_edit` on first verified decision, the record says `pinned_by: tofu`, and the operator action at `epic-93:264` goes).

**F4 — blocker — Gate proposals have two incompatible fates.**
`epic-95:356` (95.2 acceptance 1: a proposal with `origin` `gate` "gets `verdict = "rejected"`" on the first night) vs `epic-99:137` ("proposals whose `origin` is `gate` expire unread after 30 days (`verdict = "expired"`)") and `:146` (acceptance 6 `gate_proposals_expire_unread`); `ARCHITECTURE-AGENTS.md:1123` sides with 99.2.
The 30-day state is unreachable if 95.2 rejects on night one; `gate_proposals_expire_unread` cannot pass against 95.2's consolidator. Also `epic-99:145` asserts a gate has no `memory_propose`, but 95.1's nudge review pass (`epic-95:300-302`) is offered `memory_propose` in every session and is not switched off for `kind = gate`.
Fix: pick one — recommended: 95.2's structural gate *skips* (leaves pending) `gate`-origin proposals and the curator sweep expires them at 30 d; or 99.2 drops acceptance 6 and AD-416's "expire unread after 30 days". Either way 95.1 must disable the nudge pass (or its `memory_propose`) for gate sessions.

**F5 — blocker — Delegation cannot be read before joining, and 90.5 forbids joining.**
`epic-90:462` ("Join only the rooms named by a session `agent.toml` of an agent hosted here. An invite to any other room is left pending, never joined."); `epic-92:142` (target side: "an invite plus a delegate event to one of this principal's agents is placed … the winner creates the session"); `epic-91:178` (only the human's invite to a *proxy* conversation is accepted).
matrix-sdk 0.18: an invited client receives only `invite_state` (stripped state) — `matrix-sdk-base-0.18.0/src/response_processors/room/sync_v2.rs:226`; `ruma-client-api-0.24.0 sync/sync_events/v3.rs:591-595`. The brief, label, hop and limits live in the timeline event `dev.keeper.agent.delegate`, so the target host cannot read them, place the session or create `agent.toml` (which is what 90.5 requires before joining). Chicken-and-egg; the live test `a_delegation_round_trip_on_a_real_homeserver` (`epic-92:155`) cannot pass as specified.
Fix: amend 90.5's join rule: join a `dev.keeper.agent.session` room on invite when the inviter is an agent user of a drive this host knows (`_drive.toml` audience ⊇ … per `check_sink`) or the proxy's `human`; 92.1 orders the target side as join → read delegate event → place → create (idempotent on the event id); the delegator sends the brief only after seeing the target's `m.room.member` join (keys are shared to invited devices under `shared` visibility — `matrix-sdk-base client.rs:1013-1021` — but sending before join still risks an untracked device). Add a negative test: an invite from an unknown user is left pending.

---

## Major

**F6 — major — `ARCHITECTURE-AGENTS.md` was not updated for R24 (tool names, config grammars, sandbox, sinks, crates).**
- `:288`, `:974`, `:1039`: MCP tools `mcp:<server>/<tool>`; R24(7) wire name `mcp__<server>__<tool>` appears nowhere in the architecture (`epic-96:124` has it; `epic-94:130` still only the old form).
- `:974`: AD-397's closed vocabulary has no `kvm_snapshot`/`kvm_act` (R24(3); `epic-96:114`, `:263`), yet `epic-94:373` tests "every tool it names is in AD-397's vocabulary".
- `:623-627`: `[[mcp]]` has only `name|url|credential`; R24(4)'s `command|readers|role|fingerprint`, `[[kvm]]` (`epic-96:263`) and `[sandbox] read_exec` (`epic-96:120`) are absent; `:278-304` `agent.toml` has no `[[gate]]`, `:425-443` session `agent.toml` has no `delegates` (`epic-99:78,82`).
- `:923` (AD-391): MCP/KVM/`run` sink audience "`*` (anyone)" — R24(1) says configured `readers`; `epic-92:140` ships `Sink::External` as `*` and 96.2 must then change 92.1's shipped arm.
- `:1023` (AD-405): landlock only — R24(5) landlock + `seccompiler`; `:155` guard row lists no `seccompiler`; `:1023` "label must allow `*`" for network vs R24(15)/`epic-96:128`.
- `:1039`: "unknown or destructive ⇒ T3" makes Paseo's reads T3 against AD-407's T0; R24(2)'s four `role = "paseo"` rows missing.
- `:1060`, `:1065`: "screen hash" precondition vs R24(6) accessibility path / difference hash (`epic-96:122`).
- `:94-108`, `:162-169`, `:147-155`: no `keeper-nse` (R24(14)); no Android CI job (R24(13)).
- `:1216`: epic map 96 "none in the shell crate" vs R24(8) and `epic-96:206`.
- `:1081`: one mixed backchannel list vs R24(10) per language (`epic-97:93`); `:1070`: no hydration by role (R24(11); `epic-97:101`).
Fix: apply R24 item by item to the Data formats tables, AD-391/397/405/406/407/408/409/410/411, the crate topology, the compiles-where table, the guards table and the epic map; add a `> Coordinator note (R24)` under each AD changed.

**F7 — major — Architecture/D-entries not updated for R25–R27 (`run: waiting`, session `kind`, claim-epoch precondition, ports, `agents init`, AD-404, AD-401, AD-364, AD-367, AD-375, FR-781).**
- `run:` gains `waiting` (R25): `ARCHITECTURE-AGENTS.md:585` (card fields), `:502` (log `run` line), `:87`; `epic-92:167` (92.2 grammar), `:111` (Q7 "no new card key"), `deferred-work.md:7146` (DW-376), `sprint-status.yaml:123`, `research:1702` all still `queued|running|blocked|review|failed`. One grammar, two rulings' worth of readers (91.1 header, 92.2 badge, 98.2 phone) — must agree.
- Session `kind` gains a value for a person-started proxy conversation and the status schema names the DM (R25): `:431` enum and `:661` status content unchanged; `epic-91:116-117` (Q3/Q4) say the opposite ("No schema change", "`main` means …"). Decide which reading R25 accepted and fix the other.
- "a claim epoch is not an approval precondition" (R25): approval record still has `"claim_epoch": 4` under `preconditions` (`:539`) and AD-394 re-checks "the claim epoch" (`:938`); `epic-93:87` (Q1) reads it as "current claim, epoch ≥ record's" — write that into the record schema (drop `claim_epoch`, or rename `written_at_epoch` outside `preconditions`).
- Ports/TurnOrigin (R27): `:124` and `:774` list four ports and `TurnOrigin{…, Agent{session}}` in 90.1; `GrantSource` (fifth) and `Agent` land in 90.5 (`epic-90:105-116`).
- `agents init` is 91.5's (R25/R27): FR-781 (`:1145`), AD-375 (`:826`) still put it in 90.5; FR table column should read "90.5, 91.5".
- AD-404 (`:1018`): FR-229/235/236/241 (R26 → FR-243/FR-244), `generated: {by: …}` (R26 → `agent:<agent>@<host>`), writes into the closed session's `artifacts/knowledge/` (R25/R26 → steward's own harvest session); `:1017` cites `docs/sessions.md:1084-1089` (now `:1070-1073`, `epic-95:147`); research `:1287` still FR-229.
- AD-401 (`:996`) "03:00 in the principal's time zone" vs R26 host local offset; FR-805 (`:1169`) "the owner approves first" vs R26/`epic-95:404` requester approves `USER.md`.
- AD-364 (`:748`) "enforced by `keeper-ported::hermes`" in 89.3 breaks AD-396's first-consumer rule (`:1224`: hermes → 95.1) and R27.
- `:955-959`, Ambiguity 9 (`:1317`): "BMAD-METHOD's licence is not stated locally" — R27/`epic-89:71`: MIT, v6.12.0 @05bfbd46.
- Epic map (`:1209`, `:1212`): 89's shell column lacks 89.3 (R27, `epic-89:159`); 92 has two rungs with 92.6/92.5 in `epic92-delegate` vs R25/`epic-92:105` three rungs.
Fix: one editing pass over the architecture, each change under a coordinator note; the stale FR table rows (FR-781, FR-784 "approval cards" → 91.1, 93.3; FR-795 lift → 92.6, 93.3; FR-794 "propose what the drive should learn" → 92.5, 95.5; FR-772 write-time refusal → 89.3, 95.1; FR-808 "any artifact" vs `epic-95:650` notes vault only).

**F8 — major — D-34 was not corrected as R25 says it was.**
`docs/decisions.md:1769-1771`: "once Nixi has read tgdrive in a session, handing that session's work to the shared steward is refused until tgorka allows it". R25: "**Every** Nixi→shared-agent hand-off is a declassification … (stricter than the D-34 draft wording, which is corrected to match)". Epics 91 (Q12, `:125`) and 92 (Q4, `:108`) build to R25; the D-entry a reader cites still says the weaker rule, and its status line (`:1784-1785`) does not cite R25.
Fix: replace with "A proxy's context carries its person's core memory, so every hand-off from a proxy session to an agent with a wider audience is a declassification: the brief is shown to the person and released with one tap; the release is the recorded declassification."

**F9 — major — D-33/D-35/D-36 contradict the rulings or each other.**
- `docs/decisions.md:1705-1706` D-33 "sandboxed by landlock on Linux" — no seccomp (R24(5)); `:1715-1716` no "a networked `run` is approved per run and that approval is the configuration, with a `docs/egress.md` row" (R24(15)); its own revisit trigger (`:1731-1732`, "landlock lacking a restriction") is already met by `epic-96:117-118`.
- `:1808-1810` D-35 promises Android voice "with echo cancellation and the same turn models"; `:1849`/`:1853-1855` D-36 runs `ort` on "the Mac and the iPhone" only; `deferred-work.md:7504-7508` (DW-413) plans a no-models Android fallback. D-35 promises more than any story delivers (`epic-98:262` accepts the fallback).
- D-36 (`:1846-1849`) lacks R24(11) hydration by role; D-31 (`:1637-1639`) credits "keeper-agent's session writer" against R27 (file-level writer in core).
Fix: amend D-33 with seccomp + per-run network approval; make D-35 say "turn models where `ort` runs (Mac, iPhone); Android uses the platform recogniser, a documented limitation (R20)"; add role hydration to D-36.

**F10 — major — Perf budgets whose owning stories never assert them.**
- NFR-113 (`ARCHITECTURE-AGENTS.md:1188`, "p95 on tuwunel: the anchor appears within 1 s …"): 90.5 acceptance 13 only "records" the anchor delay on **Synapse** (`epic-90:512`); 91.1 acceptance 8 is "owed, operator measurement" (`epic-91:165`). No test asserts ≤ 1 s anywhere; the NFR has no failing condition.
- NFR-114 (`:1189`): 97.2 measures "to `FinishRecognition`", not turn end (`epic-97:97`), and the send may follow up to 600 ms later (`:162`); 97.3 measures "by the voice log's timings" (`:200`) but adds no pause/continue fields to that log (`:180-186`); the 300 ms resume budget is contradicted by the epic's own numbers (96 ms onset `:105` + recogniser lag "a few hundred milliseconds" `:96`).
- NFR-112 (`:1187`) and NFR-122 (`:1197`) are measured only through operator actions (`epic-90:418-420`, `epic-92:250`) — acceptable if named, but the numbers' *publication* is the only gate.
Fix: NFR-113 gets a Synapse-harness assertion in 90.5 acceptance 13 (anchor ≤ 1 s, final edit ≤ 1 s after stream end, p95 over ≥ 50 turns) with tuwunel as the published figure; NFR-114 names the clock points (VAD speech-end timestamp → `UtteranceEnd` → send) and the log fields 97.3 writes, with sample size and percentile; or the budgets are restated to what can be measured.

**F11 — major — Canonical-JSON spec disagrees with the ruling that accepted it.**
R25: "canonical JSON (RFC 8785 subset: sorted keys, serde_json numbers, **no floats in digested fields**) is implemented in core". `epic-93:92` (Q6): "UTF-16 key order, ECMAScript number form, the RFC's string escapes"; acceptance 4 (`:129`) tests the RFC's float samples (`1e+21`, `1e-7`, `-0 → 0`, `333333333.3333333`) and "a non-finite number is refused". If digested fields hold no floats, the float tests exercise code with no caller; if they do, R25's subset is wrong. Two hosts (desktop, agentd) computing `binding_digest` differently is a consume-twice bug.
Fix: state one grammar in *Data formats* under `binding_digest`: keys sorted by UTF-16 code units, integers only in digested fields (a float in `args` is refused at record creation with its path), strings escaped per RFC 8785 §3.2.2.2; drop the float samples from acceptance 4 and add "a float anywhere in `{tool,args,exec_binding,preconditions}` refuses the record".

**F12 — major — The "unattended sentence" that four stories assert does not exist in the code they cite.**
`epic-90:117` (C4 "the unattended sentence (`bots_tools.rs:109-114`'s behaviour)"), `:500`; `epic-92:133`, `:154`; `epic-93:51`, `:158`, `:208`; `ARCHITECTURE-AGENTS.md:888`, `:923`. `keeper/src/bots_tools.rs:109-114` is `fn ask`: `self.approve.as_ref().is_some_and(|approve| approve(call, reason))` — a bare `false`, no sentence; the only words are the module doc at `:43-45` ("A host built with no approver declines every ask…"). Tests "refused with the unattended sentence" cannot be written until someone defines the sentence and where the model sees it.
Fix: 90.1 (the move) defines `pub const UNATTENDED_REFUSAL: &str` in `keeper_agent::host` with the exact text, returned as the tool result; 90.5/92.1/92.3/93.2/93.4 cite that constant.

**F13 — major — Research states superseded versions of R14–R27 as open or decided otherwise.**
`research-agents-2026-10-02.md`: `:1801` (#21 answer over 64 KiB "not pinned" — R23); `:1812` (#32 KVM raise "is open" — R22); `:1821` (#41 Android voice "not pinned" — R20); `:1014` and `:1817` (#37 backchannel rule, stop-first vs R14 pause-first); `:1829` (#49 "the operator decides" — R17 requires agentd's own push); `:174`/`:1831` (#51 spelling undecided — R19); `:177`/`:479` ("Epic 22's refusals are reversed" — R16: scoped, not reversed); `:1789` (#9 "consumption recorded with the claim epoch" — R25); `:1642`, `:1647`, `:1666` (§12.4: `TurnOrigin` without `Agent`, four ports, a blocking `MatrixApprover` — R27/AD-394); `:1702` (§12.6 `run` without `waiting` — R25); `:1811`/`:1076` (landlock only — R24(5)); `:1287` (FR-229 — R26); `:1823`, `:1916`, `:1858` (BMAD/OpenClaw licences open — R27/`epic-89:71` says settled); `:1792` (#12 cites `/workspace/tgdrive/README.md:31-33` for the 256 KiB threshold — that is prose about backup priority; the threshold is `/workspace/tgdrive/.keeper/keeper.toml:33`).
Fix: a "superseded by ruling" column or inline strike-through per row; §14 rows for BMAD licence, `tokio::spawn` and Linux build marked "settles in 89.1 / 90.1" rather than open.

**F14 — major — `[[trust]]`/approver aside, 93.3's "every device" and 98.x FRs rest on operator-only proofs.**
FR-815 "on the Mac and on the phone" (`ARCHITECTURE-AGENTS.md:1179`): 97.2's phone proof is an iOS compile (`epic-97:165`) and an operator checklist (`:168`). FR-819 (`:1183`) sideload/sign-in/rooms/board: operator-only (`epic-98:233-236`). FR-820 "the same turn models": 98.4 acceptance 7 accepts the fallback without them (`epic-98:262`), echo cancellation only "recorded" (`:273`). FR-817: wholly gated on Apple enrolment (`epic-98:160-171`). FR-808 "any artifact" vs 95.5 notes-vault-only (`epic-95:650`, DW-390).
Fix: either reword the FRs to what the repo proves (e.g. FR-815 "…and on the phone once the device run in 97.2's checklist is recorded") or add device-run acceptances with a recorded artifact (`docs/agents.md § Measured`) as the gate.

**F15 — major — R24 fallout inside the epics themselves.**
- `epic-98:156` (98.1 #9) quotes the iOS check as `cargo check --workspace --target aarch64-apple-ios` — 90.5 changes it to `--exclude keeper-agentd` (`epic-90:451`); `keeper-nse` must be a member that check compiles, and the Android job is described two ways (`epic-98:113` `-p keeper` vs `:221` four crates).
- `epic-96:107` ("the plan needs the coordinator's nod before 96.2 lands") and `epic-99:75` still await decisions R24 took; `epic-96:112` vs `:263` configure a NanoKVM-Go in two tables (`[[mcp]] role = "kvm:<id>"` and `[[kvm]] kind = "nanokvm-go"`) — which holds `readers`/`fingerprint`/credential is undefined.
- `epic-96:190` (96.2 shell) names commands but no `keeper/src/*.rs`, `lib.rs` registration, front component or mock-shell file (compare `epic-95:606-614`).
- 96.1 has no `cargo deny` acceptance for `landlock`/`seccompiler` (96.2 #4 has one for `rmcp`); Gradle deps (`epic-98:248`) are outside `cargo deny` and no story says how NFR-119 covers them.
- `deferred-work.md:3958` DW-237 still says `ipc.rs:1456 (sessions: notes_available(&state))` — R24(12) moved it to `:1481`, and `:1481` is now `sessions: mac_folder_capability_of(...)`, so the quoted code is stale too.
Fix: as listed; 98.1 #9 → "the iOS check (`--workspace --exclude keeper-agentd`) compiles `keeper-nse`"; one `[[kvm]]` table owns every KVM's `readers`/`fingerprint`/credential and `[[mcp]] role = "kvm:<id>"` references it.

---

## Minor

**F16 — minor — Code citations that are wrong or drifted (epics 89–90, architecture).**
- `SessionTaskVm` is `keeper-core/src/sessions/vm.rs:388`, not `vm.rs:388` (`ARCHITECTURE-AGENTS.md:581`, `:893`).
- `tasks.rs:9-49` is constants (`MIN_SCHEDULE_INTERVAL_MS` `:31`); the parser is `TaskSchedule::parse` at `:695` (`ARCHITECTURE-AGENTS.md:589`, `:898`; `epic-92:64` has it right).
- `frontmatter.rs:725-749` is `unescape_double`; a scalar is "single-line by construction" (`:54-55`) — 89.3's "double-quoted multi-line `identity` reads back" holds only via `\n` escapes, which is what DW-357 says; cite `quote_double` (`:1018`) for the write side.
- `sessions/files.rs:92-119` is `NewFileKind::parse`; the `.jsonl` refusal path is `check_rel` (`:209-212`), `compile_new` is `:671`.
- `bots/mod.rs:355-358` is the prefix arm of `Endpoint::url`; `Endpoint::new` is `:328`.
- `engine.rs:1919` → `recover_running` call is at `:1925`; `release.yml:241+`/`:229-330` — the file has 322 lines.
- `org_account/auth.rs`, `org_account/account.rs` → `keeper-core/src/auth.rs`, `keeper-core/src/account.rs` (`epic-90:60`, `:380-382`, `:393`; `ARCHITECTURE-AGENTS.md:805`); `tauri.conf.json:5` is `crates/keeper/tauri.conf.json:5`.
- `arm_drive(profiles, grants, offered)` (`epic-90:189`) is `arm_drive(state: &AppState, grants, offered)` (`bots_drive_ipc.rs:89`); profiles are read inside.
- `sessions_root.rs:316-321` is `rescan`; the 400 ms coalescing loop is `:180-188`.
- `vm.rs:4200`/`:4238`/`:10301` point at the `voices` field/arm/test line, not `FilesFolderRoles` (`:4192`), `FilesFolderRoleVm` (`:4170`) or the role tests (`:10379`, `:10421`); `FolderTier::new` is `:354` not `:787`.
- `keeper-core` already depends on `sha2`, `hex`, `toml`, `unicode-normalization`, `rusqlite` (`keeper-core/Cargo.toml:45,76,95,103-104`); `epic-89:334`'s "plus `sha2` and `hex`" is a no-op.
- `notes/search_index.rs` only defines `SEARCH_DB_FILE` (`:21`); the `.keeper/search.db` path is built in the shell (`keeper/src/notes_vault.rs:1139`) — 89.5 C1's "core already opens files under `.keeper/`" is weaker than stated (AD-24 tension; R27 accepted the placement, so cite `org_account/descriptor.rs:1251-1255` only).
Fix: correct in place; the epics' "line numbers are in the current worktree" promise is otherwise good.

**F17 — minor — 89.3's fixture diverges from the architecture's own `agent.toml` example.** `ARCHITECTURE-AGENTS.md:242` uses `bot:openai:…#<model>`; `epic-89:375` says "the architecture's Nixi example parses with `bot:ollama:…`" because `openai` arrives in 89.6 (same epic, later rung). Either the example in *Data formats* shows `ollama` with a note, or 89.3's fixture is named as a variant, so "the architecture's example parses" is literally true.

**F18 — minor — Steward default set stated three ways.** `ARCHITECTURE-AGENTS.md:913` (AD-389: specialist + `card_update`, `delegate`, `workflow_start`), `:974` (AD-397: proxy's set + …; R27 says AD-389 wins — AD-397's sentence still uncorrected), `epic-89:368` (specialist + the three), `epic-92:262,267` (specialist + `card_update`, `delegate`; "`workflow_start` joins it in 94.3"). 89.3's specialist set already contains `card_update` and `workflow_start` (`epic-89:369`), so all three reduce to specialist ∪ {`delegate`}; say so once and correct AD-397.

**F19 — minor — Cross-principal doorbells for a shared drive have nowhere to ring.** `epic-92:239`: zone-wide changes (`80-agents/**` → `memory`) ring "the principal's control room"; neuradrive is mounted by `agentd-tgorka` *and* `agentd-neuraffica` (AD-377), which have different control rooms. A consolidation pushed by one is seen by the other only at the 5-minute poll. Fix: for a drive with more than one mounting principal, ring every control room whose principal's `agentd.toml` lists the drive — or state that the poll is the fallback and NFR-122 excludes this path.

**F20 — minor — 95.5 allocates UX-DR137 for a panel already specified as UX-DR90.** `epics-sessions-phase7.md:120-124` ("promote-table parser and panel … Binds FR-243, FR-244, … UX-DR90"); `epic-95:676` opens UX-DR137 without citing UX-DR90. Fix: reuse UX-DR90 or state what DR137 adds (knowledge notes, staleness badge).

**F21 — minor — 90.5's unit "has syncd's hardening".** `epic-90:530`: `keeper-syncd/packaging/keeper-syncd.service` is a *user* unit whose hardening is only `NoNewPrivileges=yes` (`:67`) and `PrivateTmp=yes` (`:72`), with `ProtectHome`/`ProtectSystem` deliberately omitted (`:74-82`). agentd's is a system template unit with `User=agentd-%i` and `LoadCredential=`; it should say which directives it adds (`ProtectSystem=strict`, `ReadWritePaths=` its XDG dirs, `ProtectHome=yes` is impossible if `HOME` is its data dir — decide).

**F22 — minor — Sprint-status and epic headers point at files that are not in the repo.** `sprint-status.yaml:53,61,72,81` "(drafted in agents-decisions-draft.md)" — D-31…D-36 now live at `docs/decisions.md:1584-1877`; `:111,121,133,144,156` and every epic header cite `local://coordinator-decisions.md` and `local://program-map.md`, which exist only in `/tmp`. Epic headers also say both "allocates no number" and "deferred items start at DW-382" (`epic-94:22` vs `:27`, same pattern 95–99). Fix: cite `docs/decisions.md` lines; check the coordinator file in under `_bmad-output/planning-artifacts/` or name it as external in each header.

**F23 — minor — 89.5's drive-cost measurement is unrepeatable by design.** `epic-89:557-560`: "The measuring script is not kept", yet DW-359's revisit trigger (`:704`) depends on re-measuring. Keep the script under `keeper-core/tests/` as an `#[ignore]` test, as 89.6/90.4 do for their live tests.

**F24 — minor — DW-366 and DW-395 overlap; DW-391 records the uncorrected AD-404 as current.** `deferred-work.md:7073` vs `:7378` (both "no Linux CI job", one says the release job is planned, the other "revisit when … gets its Linux release job"); `:7338` "The agents program cites those ids for the panel alone (AD-404)" describes the FR-229 wording R26 corrected. Merge the two DWs; reword DW-391.

**F25 — minor — Two architecture-vs-epic drift points inside epic 90.** C9 (`epic-90:128`) says the coordinator's scope mis-numbered Ambiguity 5 as 7 — fine, but AD-371 (`ARCHITECTURE-AGENTS.md:806`) still lists `get_state_event_static` as the read path with no "except the claim read-back" clause; and 90.4's `[UNVERIFIED]` on `RequestConfig` (`epic-90:380`) is settled: `RequestConfig::disable_retry` (`matrix-sdk-0.18.0/src/config/request.rs:117`), applied per send via `.with_request_config(..)` (`room/futures.rs:169`). Also `send_raw`/`Client::send` are builder-returning, not `async fn` (`room/mod.rs:2621`, `client/mod.rs:1960`) — harmless, but the API table says "call".

---

**Verified and holding (for the record):** the `voices` recipe lines in `profile/mod.rs`/`folder.rs`/`sync_ipc.rs`/`add-folder-form.tsx` (89.2); `ProviderKind` arms, `quirks`, `discover`, `grant`, `commands`, `voice_target`, store/egress sites (89.6); `run_tool_loop_reporting:1215`, `ToolCallReporter:1204`, `FILE_CONTENT_IS_DATA:153-155`, `arguments_raw` replay (`chat.rs:348`), `UNTRUSTED_PREAMBLE:97-102`; `shape.rs:356-416`/`STATUSES:370`; `bots_ipc.rs:1132-1984`, `:1150`, `:1173`, `:1177-1186`, `:1275-1284`; `bot_task.rs:109,134`; `sessions_exec.rs:4-10`'s two false promises are indeed unfulfilled (no `Mutex`, no `resume` at start); `sessions_ipc.rs:873`; `keeper-syncd/src/platform.rs` secret helpers; `wake_now:13966`, `remote_poll_due:16845`, `REMOTE_POLL_MS:570`, no `pull_now`; `browse::resolve:746`; `device_slug:484-491`; `ci.yml:90`; Synapse `v1.156.0`; `docs/ios.md:737`; `cargo deny` in CI (`ci.yml:109`); tgdrive zones (`README.md:9-24`, 80 free), `lfsThresholdBytes = 262144`, `lfsNever = ["*.md","*.txt"]`, `[folder.sessions]` present; matrix-sdk 0.18 encryption APIs at the exact lines 93.3 cites; `default_event_filter:254-332` with `_ => false` at `:321`; edits aggregated only for `RoomMessage`/polls (`event_handler.rs:389-402`); `create_room` accepts a custom `type` (`CreationContent.room_type`, `_Custom`); 50 story ids present once in sprint-status; DW-355…417 contiguous with R26's `DW-E94-3/E95-7/E95-8` mapping to DW-383/391/392.
