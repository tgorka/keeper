# Agents program retrospective — epics 89–94, and the program 89–99 as a whole (2026-10-13)

Six epics and 31 stories were built as 37 rungs. The plan named 14. All 37 PRs merged on 2026-10-11 at 01:27 UTC together with the plan PR #432, through the stack merge that also landed epic 95's first four rungs (#477, main `6744d468`). None is open. Claude Opus 5.5 wrote the code (coordinator's record).

Claude Fable 5.1 reviewed epic 89, epic 90 rungs 1, 2 and 7, and epic 91 rung 2. Fable is from the same vendor as the author, so those reviews are **not fully model-independent**. GPT-6 Astra reviewed epic 91 rung 5 and every rung of epics 92–94. No artifact names the reviewer of #440–#443, #445, #447, #448 or the epic 94 prep rung.

Every story is done in the repo. What is still owed is 24 device runs and the production rollout on electra. No agent has yet run on the owner's real drives.

Sources: the epic docs in `_bmad-output/planning-artifacts/epic-89-*.md` … `epic-94-*.md` (*As built*, *Owed when built*, *Operator actions*); `/tmp/agents-salvage/review-epic89.md`, `review-epic90-*`, `review-epic91-*`, `review-epic92-*`, `review-epic93-*`, `review-epic94-prep.md` and `review-94*.md`; the bodies of PRs #432–#473 (GitHub); `agents-coordinator-decisions-2026-10-02.md`; `deferred-work.md`; `/tmp/agents-salvage/ctx/stack-notes.md`; and the `ctx/brief-*.md` files named below. The final shas are the merged commits (`git log bec12596..6744d468` in this worktree), and the branch names are the branch heads.

## What shipped

Each rung below gives its branch, PR and final sha. **What you get** is the person-facing change. **Proved** is what an artifact records as run. **Owed** is what is still to do on a device or by the operator (each item is in § *Owed device runs*). Unless a rung says otherwise, its PR body records two more things, and its lines do not repeat them:

- A Linux gate for the rung alone: fmt, clippy `-D warnings` and the crate tests.
- Mutation-proved new tests.

The exceptions: the UI-only rungs #454 and #466 ran biome, tsc and vitest with UI mutants instead of the Rust gate, and #468's body records no mutation run.

### Epic 89 — An agent is a file in your drive

There are four rungs (the plan had two). There was one review round.

#### 89.1 keeper-ported crate and 89.2 the agents zone
- **Rung `agents-89-zone` — #434, merged, final `8dc88052`.**
  - **What you get:** a drive can name an agents zone (`[folder.agents]`, default `80-agents`; it needs a sessions zone). A `_drive.toml` in it says who owns the zone, who may read it and which folders are untrusted. keeper also carries BMAD's customization merge and the agentskills validator as pure Rust in `keeper-ported`, each with an `UPSTREAM.md`.
  - **Proved:** the `check:ported-pure` guard, wired into `bun run check` and lefthook (PR #434). The shell edits were checked by inspection only.
  - **Owed:** 89.2 #13, the folder-flag smoke on hesperia, and `check:rust:macos`.

#### 89.3 an agent's home and 89.4 labels
- **Rung `agents-89-home` — #435, merged, final `84246ead`.**
  - **What you get:**
    - keeper reads an agent's home: `agent.toml`, `SOUL.md`, `USER.md` and `MEMORY.md` within their caps, and its skills. It composes the system prompt in a fixed slot order.
    - Every input carries a label saying who may read it and how far it is trusted.
    - A tool write anywhere in an agent's home is refused, case-folded, and also where a link lands.
  - **Proved:** the golden prompt `nixi-told.md`, the label lattice and the write fence (PR #435). The one shell edit (`.with_agents`) was checked by inspection.
  - **Owed:** 89.3 #12 on hesperia (a ⌘9 bot is refused when it tries to edit `SOUL.md`), and `check:rust:macos`.

#### 89.5 a session an agent works in
- **Rung `agents-89-log` — #436, merged, final `4ca5b36c`.**
  - **What you get:** an agent session keeps its whole record in the drive:
    - a chunked JSONL log that is never large enough to become an LFS object;
    - blobs for large bodies;
    - a byte-for-byte replay;
    - a local index that is never synced.
    - Secrets in the nine known shapes are redacted before anything is written.
  - **Proved:**
    - Replay runs through the real tool loop, and every string leaf is redacted (PR #436, review R-1 and R-3).
    - A 10 000-line session costs 14.80 MiB loose and 528.62 KiB packed (`docs/agents.md` § *What a session costs the drive*).
    - The rung touches no shell file.
  - **Owed:** nothing on a device.

#### 89.6 a generic OpenAI-compatible provider
- **Rung `agents-89-openai` — #433, merged, final `7cff6c9e`.**
  - **What you get:** a third provider kind, `openai`, for any OpenAI-compatible endpoint such as CLIProxyAPI. Its models come from `/v1/models`, and tools are offered with the unknown-capability warning.
  - **Proved:**
    - The live smoke against CLIProxyAPI passed: health, model list, a three-round drive tool loop, `[DONE]` plus the usage frame, and the 401 sentence. The key never appeared in the output (PR #433).
    - The golden prompt measured `prompt_tokens = 1915` against the 16 384-token context Ollama runs with (`docs/agents.md` § *What an agent is told*).
  - **Owed:** 89.6 #10 on hesperia. Add the provider in Settings › Bots, see *Test* read Online, and run a ⌘9 chat that uses `drive_read`.

### Epic 90 — One turn loop on the Mac and on the server

There are seven rungs (the plan had three).

#### 90.1 extract keeper-agent
- **Rung `agents-90-extract` — #438, merged, final `f502594f`** (its first commit, `f142cf6f`, captured the goldens).
  - **What you get:** the turn loop, the drive tool host and the task runner moved into the tauri-free crate `keeper-agent`, which both the Mac and a Linux server link. Only one thing changed for a person: the scheduled task runner answers an ask with `UNATTENDED_REFUSAL`.
  - **Proved:**
    - Characterisation goldens were captured on hesperia from the code before the move (`f142cf6f`), and `the_extracted_turn_reproduces_the_goldens` holds the moved code to them.
    - The `check:agent-tauri-free` guard (PR #438).
  - **Owed:**
    - `check:rust:macos`, which was not run because the Mac was busy (PR #438).
    - 90.1 #9, the before/after smoke on the installed build.

#### 90.2 the sessions runtime, made safe
- **Rung `agents-90-sessions` — #439, merged, final `3e8af7b4`.**
  - **What you get:** sessions can be created, archived and written safely when several things do so at once. There is a zone lock plus a lock file, a pending plan is resumed at start, a create carries a caller-supplied id, and containment is checked canonically.
  - **Proved:** the zone-lock, resume and caller-id tests (epic 90 § 90.2's *As built* notes).
  - **Owed:** `check:rust:macos`, which was not run (PR #439), and 90.2 #10 (`kill -9` mid-create, then relaunch).

#### 90.3 keeper_sync::xdg, headless platforms, agentd.toml
- **Rung `agents-90-xdg` — #440, merged, final `10afae28`.**
  - **What you get:** a Linux host's configuration. It covers `agentd.toml`, secrets from the environment, `LoadCredential` or `0600` files, and the mount rule (a principal never mounts a drive outside its readers).
  - **Proved:** the Linux gate (PR #440).
  - **Owed:** the operator's setup on electra: OS users, Forgejo tokens and pins.

#### 90.4 a lean Matrix client for agents
- **Rung `agents-90-matrix` — #441, merged, final `945b9394`.**
  - **What you get:** the agents' own Matrix client, their event contents and the session-room power levels.
  - **Proved:**
    - The live run against Synapse on delectra passed 8/8.
    - It found that a limited timeline lost 12 % of events between syncs, fixed by `SYNC_TIMELINE_LIMIT = 500`.
    - p95 delivery between two copies was measured (PR #441; `docs/agents.md` § *Measured on Synapse*).
  - **Owed:** NFR-112 on tuwunel (PR #441: "Awaiting the operator").

#### 90.5 keeper-agentd init, login, run
- **Rung `agents-90-agentd` — #442, merged, final `e32964e7`.**
  - **What you get:** `keeper-agentd`, the agents' Linux host. It has `init`, `login`, `agents list`, `run` and `status`. It answers in an agent's rooms with streamed edits and keeps the session log.
  - **Proved:**
    - Live on Synapse with CLIProxyAPI, over 50 turns: p95 to the anchor 47 ms, p95 to the final edit 287 ms.
    - `FINAL_CUT_BYTES` was measured at 45 KiB.
    - 20 mutations were killed, and `check:agentd-lean` passes (PR #442; epic 90 § 90.5 *As built*).
  - **Owed:**
    - NFR-113 on tuwunel.
    - The `workflow_dispatch` release dry run with the real signing key.
    - The production rollout on electra (90.5 #15).

#### 90.6 hosts, claims and placement
- **Rung `agents-90-claims` — #443, merged, final `428fdb92`** (the pure half and agentd).
  - **What you get:** each session has exactly one owning host, held by an epoch claim. Another host takes over after the claim expires, and a session nobody can serve says why.
  - **Proved:**
    - `live_claims` 4/4 on Synapse: a start race has one winner, a takeover follows `kill -9` about 180 s later, and a clean handover takes 0.2–1.7 s.
    - NFR-113 still holds with claims in the path (p95 43 ms and 292 ms) (PR #443).
  - **Owed:** the claim race rerun on tuwunel (90.6 #15).
- **Rung `agents-90-hosts` — #444, merged, final `fe7ddf50`** (the Mac).
  - **What you get:** the Mac hosts agents under its own pins. Settings › Agents signs copies in, pinning exactly the owner, readers and local-only setting the person was shown.
  - **Proved:** the macOS gate passed on hesperia at this sha, and the browser proof passed at 1280 px and 420 px (PR #444).
  - **Owed:** 90.6 #14, the Mac taking over and handing back against electra's daemon.

### Epic 91 — Nixi: one door, and the note you both see

There are five rungs (the plan had two).

#### 91.1 agent rooms in keeper
- **Rung `agents-91-rooms` — #446, merged, final `332d98ff`.**
  - **What you get:** an agent's room reads as an agent's room. It has a header with the run state, scope and label chips, one growing answer that is never marked edited, and its own Agents window.
  - **Proved:** live, the event cache holds a decrypted status after a restart (R33). The macOS gate passed on hesperia (PR #446).
  - **Owed:** a real-WKWebView proof on hesperia, and NFR-113 on tuwunel.

#### 91.2 the assistant beside your notes
- **Rung `agents-91-dock` — #447, merged, final `550b9a7d`.**
  - **What you get:** Nixi is docked beside the note you are reading. The dock shows Nixi's rooms, lets you choose the drives in scope, sends your place in the note as focus and starts new conversations.
  - **Proved:**
    - Live, a scope reaches the next turn, and with two copies running only the claim holder makes a new conversation.
    - The macOS gate passed, and 33 mutations were caught (PR #447).
  - **Owed:** the dock beside two open notes on hesperia.

#### 91.3 presence and surface tools
- **Rung `agents-91-surface` — #448, merged, final `b6e52050`.**
  - **What you get:** Nixi can open, highlight, point at and scroll to part of your note on the device you are using, and can propose an edit that you apply or decline.
  - **Proved:** live, a surface request reaches the device and its answer releases the turn. The macOS gate passed, and 35 mutations were caught (PR #448).
  - **Owed:** acceptance 9 on hesperia and kalypso.

#### 91.4 talk to your main agent
- **Rung `agents-91-voice` — #449, merged, final `8458f900`.**
  - **What you get:** a spoken question can go to your assistant's room, and its answer is spoken as it streams.
  - **Proved:**
    - Live, one message is sent and its answer followed.
    - On hesperia: clippy over the workspace, and the shell lib's 536 tests. The run stopped on an unrelated perf test under load (PR #449).
  - **Owed:** acceptance 7 on hesperia and kalypso.

#### 91.5 the agents zone seeded
- **Rung `agents-91-seed` — #445, merged, final `e4468df9`.**
  - **What you get:** `keeper-agentd agents init` and `agents new`, and *Set up agents* on the Mac. They seed a drive's agents zone with Nixi, Dr Tola Grey and Dr Lucyna Novak, never write over a file, and make the proxy's DM once.
  - **Proved:** live, the DM is made once, and the person's state write is refused with `M_FORBIDDEN`. The macOS gate passed (PR #445).
  - **Owed:** operator actions 0–4: the provider record, the tuwunel users, upgrading every machine, and seeding tgdrive and neuradrive.

### Epic 92 — Stewards that hand work to each other

There are nine rungs (the plan had three).

#### 92.1 delegation opens a session
- **Rung `agents-92-delegate` — #451, merged, final `8b591339`.**
  - **What you get:** an agent can hand work to another agent. The hand-off opens a session in the target's home, and the reply comes back with its label.
  - **Proved:** live, `a_delegation_round_trip_on_a_real_homeserver` with two agentd processes. 26 + 20 mutations were caught (PR #451).
  - **Owed:** nothing on a device by name. The real hand-off refusal is 92.6's.
- **Rung `agents-92-brief` — #452, merged, final `967a3f8e`.**
  - **What you get:** the room shows a delegation's brief: who it is for, its title and its drives. It shows nothing that a narrowed label hides.
  - **Proved:** the browser proof at 1280 px and 420 px, and 9 + 12 mutations caught. The shell was checked by inspection (PR #452).

#### 92.2 the board shows who works and where
- **Rung `agents-92-cards` — #453, merged, final `0ddfbb3c`** (the Rust half).
  - **What you get:** card keys say which agent works a card and where. The run state comes from the log, and the session tools are the only door for writes.
  - **Proved:** `check-macos.sh` on hesperia passed after it caught a Mac-only case-folding path (PR #453).
- **Rung `agents-92-board` — #454, merged, final `eccb0b08`** (the UI).
  - **What you get:** the board draws who works a card and where, and a person can *Allow* a schedule an agent wrote.
  - **Proved:** the browser proof at 1280 px and 1920 px, with the unreadable badge measured (PR #454).
  - **Owed:** a real agent card on hesperia's board while `nixi@electra` runs it (DW-443).

#### 92.3 scheduled cards
- **Rung `agents-92-schedule` — #456, merged, final `3a7cd0dc`.**
  - **What you get:** a card runs on its schedule once, on one host, in its own session.
  - **Proved:** live, a claim race between two agentd processes gave one turn on one host (PR #456).
  - **Owed:** a real `@daily` card run by electra while hesperia is away, then by hesperia (DW-454).

#### 92.4 the doorbell and the quiet-folder pull fix
- **Rung `agents-92-quiet-pull` — #450, merged, final `f76f5e6a`.**
  - **What you get:** a quiet folder pulls every five minutes, not once an hour.
  - **Proved:** a failing-first run, with 12 mutations caught (PR #450).
- **Rung `agents-92-doorbell` — #455, merged, final `1dbf11fa`.**
  - **What you get:** a host that pushed a shared drive rings every control room listing it, and a listening host pulls at once.
  - **Proved:**
    - Live, a shared drive rings every control room that lists it.
    - On hesperia every crate passed except one keeper-sync sweep test that this rung does not touch (DW-453) (PR #455).
  - **Owed:** NFR-122 on the tailnet (DW-452).

#### 92.5 Dr Tola Grey and Dr Lucyna Novak
- **Rung `agents-92-stewards` — #457, merged, final `23f59067`.**
  - **What you get:** the stewards keep their own triage and harvest sessions. Daily cards come from their menu, and a closed session wakes the harvest once.
  - **Proved:** live triage with a stub model. With a real model (claude-haiku-4-5 through CLIProxyAPI) it passed only 1 run of 3 (DW-472) (PR #457).
  - **Owed:** a real `@daily` triage of tgdrive.

#### 92.6 labels enforced at every send and delegation
- **Rung `agents-92-labels` — #458, merged, final `928ff190`.**
  - **What you get:**
    - Every sink checks the label, and a block is audited and refused.
    - Untrusted content cannot pick a recipient.
    - A narrowed session's room hears only one fixed sentence.
  - **Proved:** live, a label-refused invite leaves the room as it was. All mutations were killed (PR #458).
  - **Owed:** Nixi's hand-off to Dr Lucyna Novak seen refused, with Marta named.

### Epic 93 — An action waits for you

There are six rungs (the plan had two). The PR numbers skip #461 and #462, which are epic 97's drafts.

#### 93.1 tiers and the approval record
- **Rung `agents-93-tiers` — #460, merged, final `085c0d9e`.**
  - **What you get:** every agent call is classified into a tier from T0 to T5 by one pure table. A call is one tier stricter when nobody is watching, every call is audited with its tier before it runs, and T5 is never done.
  - **Proved:** 22 mutations killed (PR #460).
- The record half is in `agents-93-park` (below).

#### 93.2 a run that parks and resumes
- **Rung `agents-93-park` — #463, merged, final `14331aa8`.**
  - **What you get:** an action that needs a person waits. Its approval record is bound by a digest and consumed exactly once through a state event that the homeserver orders. The round's remaining calls run in order after approval.
  - **Proved:** live, `consume_once_on_a_real_homeserver` and `two_copies_racing_run_the_effect_once` (five approvals, one effect each). 42 mutations were killed (PR #463).
  - **Owed:** a takeover of a parked session between electra and hesperia (DW-487 and DW-503 cover the harness gap).

#### 93.3 the approval card on every device
- **Rung `agents-93-trust` — #464, merged, final `d4f52189`.**
  - **What you get:** only a reader's own sealed device, cross-signed by a pinned identity, can approve. At T4 only the requester can, from another device.
  - **Proved:** live, `live_trust` (PR #464).
- **Rung `agents-93-card-vm` — #465, merged, final `9fb6ebe2`.**
  - **What you get:** each device draws the agent's sealed request as a card bound by its digest.
  - **Proved:** live, `live_card` drives the real command. The shell was checked by inspection (PR #465).
- **Rung `agents-93-card` — #466, merged, final `2081f294`.**
  - **What you get:** the card shows the tier as a word and a weight and the exact action whole. Decide buttons appear only where this device decides.
  - **Proved:** 16 card scenarios in a real browser at 1280 px and 420 px (PR #466).
- **Rung `agents-93-decide` — #467, merged, final `4b1e03e9`.**
  - **What you get:** decisions are live. agentd decides under its `[[trust]]` pins and the Mac under its own verified accounts. A flow beyond the label parks as a *declassify* card in the approvers' proxy DMs.
  - **Proved:** live AC6 end to end on delectra. The shell was checked by inspection, because `ssh mac` timed out (PR #467).
  - **Owed:** 93.3 AC8, the card decided on the iPhone.

#### 93.4 unattended and delegated runs
- This story was built in `agents-93-tiers` (#460: the raise and the audit) and went live in `agents-93-decide` (#467) (epic 93 § 93.4 *As built*).
  - **Owed:** a real scheduled run on electra that parks at night and is resumed by a morning approval.

### Epic 94 — Workflows the way BMAD writes them

There are six rungs (the plan had two).

#### 94.1 keeper-ported BMAD
- **Rung `agents-94-ported-prep` — #468, merged, final `7349f274`.**
  - **What you get:** BMAD's config merge, party, render and memlog, ported into `keeper-ported::bmad`. Their upstream tests are ported by name, with parity goldens printed by BMAD v6.12.0's own scripts.
  - **Proved:** keeper-ported and keeper-syncd are green. 37 keeper-sync tests failed only because the container had run out of inotify instances (PR #468).

#### 94.2 the workflow tool surface
- **Rung `agents-94-config` — #469, merged, final `44b78640`** (acceptance 1, 2, 5 and 8).
  - **What you get:** an agent can run a BMAD skill as written. Every capability BMAD assumes is answered by a tool or a sentence.
- **Rung `agents-94-render` — #470, merged, final `a429bf53`** (acceptance 3 and 4).
  - **What you get:** `bmad_render` and `bmad_memlog` write only inside the session. 14/14 mutations were killed.
- **Rung `agents-94-ask` — #471, merged, final `266e0b09`** (acceptance 6 and 7).
  - **What you get:** `ask_human` asks the person through their proxy. The proxy relays the person's own words, never the model's.
  - **Proved:** live, `ask_human_round_trip_on_synapse` passed, which proves OA-94-3. 37/37 mutations were killed (PR #471).
  - **Owed:** a phone checkpoint relayed through Nixi.

#### 94.3 workflow.toml and workflow cards
- **Rung `agents-94-workflows` — #472, merged, final `8671d08e`.**
  - **What you get:** an agent runs a BMAD workflow in a session of its own, with checkpoints that go to the person. The stewards' triage and dispatch workflows are seeded.
  - **Proved:**
    - AC8 is a takeover between two git checkouts synced through agentd's engine.
    - AC5 is a due workflow card across two host runtimes.
    - 62 mutations were killed (PR #472).
  - **Owed:** OA-94-2, the phone checkpoint, and the resume on electra.

#### 94.4 helper sessions and review layers
- **Rung `agents-94-helpers` — #473, merged, final `83a2d3a1`.**
  - **What you get:** an agent can hand a piece of work to a helper: a context-free, read-only model call. A round's helpers run side by side, and every helper counts against the turn's tokens.
  - **Proved:** 31/31 mutations were killed (PR #473).

## What the independent reviews caught

Severity is the reviewer's own, with these mappings:

| Reviewer's word | Severity here |
| --- | --- |
| blocker | HIGH |
| major | MEDIUM |
| minor, nit, info | LOW |
| untriaged (reviewers who left triage to the coordinator) | each finding listed one line apiece |

"Fixed" means fixed in place on the rung, as its PR body and the epic's *As built* say.

### Epic 89 — Fable 5.1

- **#433–#436** (`review-epic89.md`, committed as `agents-review-epic89-2026-10-02.md`). One round with 3 MEDIUM and 16 LOW findings. All were "fixed or forwarded" (PR bodies). No ruling number was given.
  - R-1 (MEDIUM): redaction missed model-written fields → every string leaf is redacted (#436).
  - R-2 (MEDIUM): the home fence did not cover what the soul reads → the whole home is fenced, and `file:` facts are data in slot 5 (#435).
  - R-3 (MEDIUM): a PEM block cut before its END marker was written in full → fixed (#436).
  - LOW: R-4…R-19 were fixed or forwarded (PR bodies). R-19 was fixed with R-2 (#435), R-10 gave case-insensitive untrusted globs (#435), R-13 gave `done_sentinel = Unknown` (#433), and R-18 was forwarded and fixed in #442 (replay carries attachments and asks).

### Epic 90

- **#438, #439** (`review-epic90-r12.md`, Fable 5.1). One round with 3 MEDIUM and 11 LOW findings, all fixed (PR bodies).
  - R1-1 (MEDIUM): a spoken turn's approval was refused instead of asked → fixed with `SinkApprover` (epic 90 § 90.1).
  - R2-1 (MEDIUM): `resume_all` ran on the main thread under the registry mutex → it now runs on a blocking task outside the lock.
  - R2-2 (MEDIUM): a verb read the zone before finishing a crashed plan → `exec::hold` finishes it first.
  - LOW: R2-3…R2-10 and R1-2…R1-4 were fixed. They covered test strength, lock files and stale references.
- **#440, #441** (`review-epic90-r34.md`, reviewer not recorded). One round with 4 MEDIUM, 9 LOW and 2 nit findings. Most dispositions are not recorded per finding.
  - R34-01 (MEDIUM): stale profiles escape the mount rule → disposition not recorded.
  - R34-02 (MEDIUM): three acceptance-5 tests passed vacuously → disposition not recorded.
  - R34-03 (MEDIUM): `local_only` in `_drive.toml` could be edited by a reader and was not pinned → `local_only` is now part of the pin (`docs/agents.md:136–140`).
  - R34-04 (MEDIUM): R30 let an observer send any encrypted event type → fixed in 90.5 (`classify`; epic 90 § 90.5 *As built*).
  - LOW: R34-06 was closed in #442 (`review-epic90-r5.md`). The rest are not recorded.
- **#442** (`review-epic90-r5.md`, reviewer not recorded). One round with 7 MEDIUM and 11 LOW findings. All 18 were fixed (epic 90 § 90.5 *As built*: "The review of rung 5 (R5-01…R5-18), as fixed"). R5-04, R5-14 and R5-17 were decided by the coordinator without a number.
  - R5-01 (MEDIUM): a message that arrived before its worker existed was dropped → the worker reads back up to 20 pages.
  - R5-02 (MEDIUM): a room was joined because a drive file named it → only the invite rule joins.
  - R5-03 (MEDIUM): `recover` did nothing after a crash mid tool loop → it edits the anchor.
  - R5-04 (MEDIUM): the answer went to the room whatever the label said → `NARROWER_THAN_ROOM`.
  - R5-05 (MEDIUM): the release job ran an unpinned `bunx` with the signing key → the distribution's `minisign`.
  - R5-06 (MEDIUM): shutdown started queued turns → fixed.
  - R5-07 (MEDIUM): a worker that ended kept its room → fixed.
  - LOW: R5-08…R5-18 were fixed. They covered exit codes, the envelope, logging, blocking work, the unit and live-test edges.
- **#443** (`review-epic90-r6.md`, reviewer not recorded). Verdict: "no blocker; three majors". R6-01…R6-14 were fixed (epic 90 § 90.6 *As built*), and R6-15 (info) stays a known gap.
  - R6-01 (MEDIUM): placement could name a host that does not host the agent → fixed.
  - R6-02 (MEDIUM): `Lease` trusted a monotonic clock that stops during sleep → it counts on both clocks.
  - R6-03 (MEDIUM): a hand-back awaited the worker inside the tick → fixed.
  - LOW: R6-04…R6-14 were fixed.
- **#444** (`review-epic90-r7.md`, Fable 5.1). Verdict: "no blocker; 2 major, 9 minor". Dispositions are not recorded per finding.
  - R7-01 (MEDIUM): the pin's `local_only` came from the file, not from what the person was shown → visible as fixed in `docs/agents.md` § *This Mac as a host*.
  - R7-02 (MEDIUM): a failed first sign-in stranded the copy's store → disposition not recorded.
  - LOW: R7-08 (rows re-read every 5 s) is visible in the docs. The rest are not recorded.

### Epic 91

- **#445** (`review-epic91-r1.md`, reviewer not recorded). 3 MEDIUM and 8 LOW findings, all fixed (epic 91 § 91.5 *As built*, "review of rung 1, F1–F11").
  - F1 (MEDIUM): a `local_only` drive could not be seeded → fixed.
  - F2 (MEDIUM): the DM was idempotent only on the local folder → `main_dm` adopts the existing DM.
  - F3 (MEDIUM): `agents init` opened the live copy's crypto store beside `run` → refused while `run` holds `agentd.lock`.
  - LOW: F4–F11 were fixed.
- **#446** (`review-epic91-r2.md`, Fable 5.1). Verdict: "no blocker. Two majors". All findings were fixed (PR #446).
  - F1 (MEDIUM): any sender could switch off keeper's notification for a message → `is_trusted_turn`.
  - F2 (MEDIUM): a person's message carrying the marker was drawn as the agent's answer → fixed.
  - LOW: F3–F9 were fixed.
- **#447** (`review-epic91-r3.md`, reviewer not recorded). Verdict: "should not ship as-is". 9 MEDIUM and 13 LOW findings, all fixed (epic 91 § 91.2, "Review fixes (PR #447 review)" and *As built (UI)*).
  - F1 (MEDIUM): a room counted as the proxy's conversation on any member's word → `proxy::admits` requires encryption and the sole creator.
  - F2 (MEDIUM): a fold right after a caret move left the host holding a closed note → `FocusLane`.
  - F3 (MEDIUM): `send_agent_event` was a public door for any `dev.keeper.agent.*` event → removed.
  - F4 (MEDIUM): no tests on the new `account.rs` surface → tests added.
  - U1 (MEDIUM): Undo-Send's ⌘⇧Z stole redo inside a note → scoped to its conversation.
  - D1 (MEDIUM): the heading was `None` on the last line of a note → fixed.
  - D2 (MEDIUM): the heading was named from the note on disk, not the buffer → fixed.
  - D3 (MEDIUM): the clear was never delivered on quit or sign-out → fixed.
  - D4 (MEDIUM): "You have no assistant yet" stuck → the dock relists on its own.
  - LOW: F5–F10, U2–U4 and D5–D8 were fixed.
- **#448** (`review-epic91-r4.md`, reviewer not recorded). Verdict: "not mergeable as is — one major". All findings were fixed (epic 91 § 91.3, "Review fixes (PR #448 review, F1–F11)").
  - F1 (MEDIUM): the device acted for any agent of any session room → it admits only the room's own proxy.
  - LOW: F2–F11 were fixed.
- **#449** (`review-epic91-r5.md`, GPT-6 Astra). Verdict: "Changes required". All six were fixed, under R72 for R5-F1, leaving DW-420, DW-421 and DW-422.
  - R5-F1 (untriaged): the unheld send did not establish whose proxy the room belongs to → R72, proxy admission fails closed.
  - R5-F2 (untriaged): a nonterminal status completed the spoken answer → fixed.
  - R5-F3 (untriaged): the follower could speak an older question's answer → fixed.
  - R5-F4 (untriaged): answers decrypted late were invisible → the follower reads the event cache. DW-422 is the residue.
  - R5-F5 (untriaged): a delayed old send could install its follower → question generations.
  - R5-F6 (untriaged): a dropped status board could announce idle → fixed.

### Epic 92 — GPT-6 Astra on every rung (PR bodies)

- **#450** (`review-epic92-r1.md`). "Changes requested". All four were fixed in this revision.
  - R1 (untriaged): a permanently failed pull was retried every five minutes → fixed.
  - R2 (untriaged): a paced pull could duplicate one already running → fixed.
  - R3 (untriaged): the test did not pin the five-minute cadence → fixed.
  - R4 (untriaged): the docs promised delivery, not eligibility → reworded.
- **#451** (`review-epic92-r2.md`). "CHANGES REQUIRED": 3 HIGH, 14 MEDIUM and 1 LOW. All were fixed under R93–R95.
  - R2-01 (HIGH): a person could manufacture a delegated session → R93.
  - R2-02 (MEDIUM): restart recovery admitted events that live intake rejects → R93.
  - R2-03 (HIGH): the label was checked when the room opened, not at each send → R94.
  - R2-04 (HIGH): the parent lost the child's `local_only` and untrusted restrictions → R94, and DW-433 is closed.
  - R2-05 (MEDIUM): a later round could replace the opening brief → R93.
  - R2-06 (MEDIUM): the model was not given the id needed to continue → fixed.
  - R2-07 (MEDIUM): artifact references were left out of the parent's model input → fixed.
  - R2-08 (MEDIUM): continuing a replied child after a restart lost its route → fixed.
  - R2-09 (MEDIUM): a crash between `delegate replied` and `peer` consumed the reply → fixed.
  - R2-10 (MEDIUM): crossing the budget on the final completion did not notify → fixed.
  - R2-11 (MEDIUM): a refused reply reset the exchange counter → fixed.
  - R2-12 (MEDIUM): the two-host idempotency test shared a filesystem → R95. DW-435 is the residue.
  - R2-13 (MEDIUM): a transient history failure was recorded as recovery → fixed.
  - R2-14 (MEDIUM): a failed brief send left the target waiting → fixed.
  - R2-15 (MEDIUM): the pending-delegation map bypassed the router's bound → fixed.
  - R2-16 (MEDIUM): reply recovery read only the newest 50 events → fixed.
  - R2-17 (MEDIUM): the card stamp let a mark's value be replaced → fixed.
  - LOW: R2-18 (artifact validation followed an escaping symlink) was fixed.
- **#452** (`review-epic92-r3.md`). "REQUEST CHANGES": 8 MEDIUM.
  - R3-01 (MEDIUM): rendered briefs kept obsolete trust → R117. DW-445 is the residue.
  - R3-02 (MEDIUM): a partial member cache counted as complete → R117.
  - R3-03 (MEDIUM): a human creator could self-certify a forged brief → R114. DW-444 is the residue.
  - R3-04 (MEDIUM): the brief text and its delegation could differ → R114.
  - R3-05 (MEDIUM): non-brief message types got the brief UI → R114.
  - R3-06 (MEDIUM): hiding metadata did not hide the body → R115 rules that the body is not masked and changes the sentence instead.
  - R3-07 (MEDIUM): cached conversations waited for a member fetch → R117.
  - R3-08 (MEDIUM): the dock promise could not be met → R116 removes the promise.
- **#453** (`review-epic92-r4.md`). "request changes": 5 HIGH, 9 MEDIUM and 1 LOW. Fixed under R118–R122.
  - R4-01 (HIGH): a directory link defeated the session tools' fences → R119.
  - R4-02 (HIGH): protected names were case-sensitive on the Mac → R119.
  - R4-03 (HIGH): `session_write` could forge `run` and `last_run` → R119.
  - R4-04 (HIGH): retagging a file laundered schedule approval → R119.
  - R4-05 (HIGH): a forged duplicate `allowed_by` survived → R119.
  - R4-06 (MEDIUM): keep rung 2's R2-17 repair → kept.
  - R4-07 (MEDIUM): unreadable agent keys were dropped → R119.
  - R4-08 (MEDIUM): a body-only read bypassed the untrusted read → R119.
  - R4-09 (MEDIUM): a length-only guard lost a person's edit → R120.
  - R4-10 (MEDIUM): file writers never rechecked the claim → R120.
  - R4-11 (MEDIUM): incremental replay could regress state → R121.
  - R4-12 (MEDIUM): the refresh raced the live writer → R121.
  - R4-13 (MEDIUM): an unbounded scan ran under the index transaction → R121. DW-447 is the residue.
  - R4-14 (MEDIUM): host keys could sit beside a move's lines → R122.
  - LOW: R4-15 (the AC7 spike) → R122 and R159 recorded it. DW-441 stays open.
- **#454** (`review-epic92-r5.md`). "changes required": 1 HIGH, 1 MEDIUM and 1 LOW.
  - R5-01 (HIGH): the front removed an IPC argument before Rust did → resolved by #453's R118.
  - R5-02 (MEDIUM): an unreadable run value escaped its badge → fixed.
  - LOW: R5-03 was fixed.
- **#455** (`review-epic92-db.md`). "changes required": 2 HIGH and 8 MEDIUM. All fixed under R160–R162.
  - DB-01 (HIGH): a level-0 visiting steward blocked its own bell → R160.
  - DB-02 (HIGH): power 50 removed an outsider from the disclosure check → R160.
  - DB-03 (MEDIUM): bells in the sync's state section went unanswered → R161.
  - DB-04 (MEDIUM): the desktop could not trust the shared-drive steward → R162.
  - DB-05 (MEDIUM): an untrusted declaration could replace drive routing → R162.
  - DB-06 (MEDIUM): duplicate and parked checks raced → R161.
  - DB-07 (MEDIUM): a sender could trigger unbounded fetches → R161.
  - DB-08 (MEDIUM): ringing could starve claim renewal → R161.
  - DB-09 (MEDIUM): the session lookup bypassed containment → R162.
  - DB-10 (MEDIUM): the last push at shutdown was never rung → R161.
- **#456** (`review-epic92-sch.md`). "changes required": 3 HIGH, 5 MEDIUM and 3 LOW. All fixed under R163–R164.
  - SCH-01 (HIGH): a takeover erased the record of the consumed window → R163.
  - SCH-02 (HIGH): a never-run interval card could run twice → R163.
  - SCH-03 (HIGH): a failed settlement was forgotten → R163.
  - SCH-04 (MEDIUM): the reread did not enforce the current schedule or pin → R163.
  - SCH-05 (MEDIUM): a holder ran due cards with no eligible placement → R164.
  - SCH-06 (MEDIUM): an interrupted run stayed `running` → R164.
  - SCH-07 (MEDIUM): the card scan dropped its byte budget → bounded.
  - SCH-08 (MEDIUM): an incomplete scan certified "alone" → fixed.
  - LOW: SCH-09 (leap day) was fixed, SCH-10 widened DW-455, and SCH-11 was fixed.
- **#457** (`review-epic92-stw.md`). "REQUEST CHANGES": 2 HIGH and 9 MEDIUM. All fixed under R165–R166.
  - STW-01 (HIGH): two checkouts could create different rooms for one duty → R165. DW-470 is the residue.
  - STW-02 (MEDIUM): a crash between room and folder left an orphan invitation → R165.
  - STW-03 (HIGH): harvest dropped the closed session's labels → R166.
  - STW-04 (MEDIUM): the synthetic event id did not stop repeats on another host → fixed.
  - STW-05 (MEDIUM): opening-date filtering dropped new work → fixed.
  - STW-06 (MEDIUM): dispatch never marked the source card → fixed.
  - STW-07 (MEDIUM): a partly synced archive got an unstable key → fixed.
  - STW-08 (MEDIUM): enqueueing counted as delivery → fixed.
  - STW-09 (MEDIUM): archive discovery was unbounded → fixed. DW-471 is the residue.
  - STW-10 (MEDIUM): room creation retries blocked every lease tick → fixed.
  - STW-11 (MEDIUM): the live smoke bypassed the data-dependent triage → real-model run added, which passed 1 run of 3 (DW-472).
- **#458** (`review-epic92-lb.md`, and `review-epic92-lb-on-stw.md` for the combination). "changes required" and "REQUEST CHANGES": 2 HIGH, 9 MEDIUM, 1 LOW and 5 untriaged. All fixed under R167–R169.
  - LB-01 (HIGH): auto-included drive context bypassed the model label → R168.
  - LB-02 (HIGH): room checks used the opening label instead of the actual audience → R168.
  - LB-03 (MEDIUM): a next round validated one recipient and sent to another → R167.
  - LB-04 (MEDIUM): the steward dispatch was unreachable after an inbox read → R167.
  - LB-05 (MEDIUM): the budget reply disclosed the narrowed label → R169.
  - LB-06 (MEDIUM): retries reused an old public title → R169.
  - LB-07 (MEDIUM): placement status undid R64 → R169. DW-462 is closed and DW-463 is the residue.
  - LB-08 (MEDIUM): the narrowed detail was not delivered for the steward case → R169. DW-460 is restated.
  - LB-09 (MEDIUM): suppressed flows had no refusal record → R169.
  - LB-10 (MEDIUM): the every-sink test was an inventory → made behavioural.
  - LB-12 (MEDIUM): keep the lower rung's R160 fix on restack → R168.
  - LB-STW-01 (untriaged): harvest published the closed session's identity before the room gate → fixed.
  - LB-STW-02 (untriaged): harvest refusals bypassed the R65 audit → fixed.
  - LB-STW-03 (untriaged): placement discarded the known-agent roster → fixed.
  - LB-STW-04 (untriaged): placement suppressed a status without recording it → fixed.
  - LB-STW-05 (untriaged): a room that became too broad never triggered its proxy notice → fixed.
  - LOW: LB-11 (declassification requests now bind the blocked effect) was fixed.

### Epic 93 — GPT-6 Astra on every rung (PR bodies; `brief-Fix93r6.md` for rung 6)

- **#460** (`review-epic93-r1.md`). "REQUEST CHANGES": 3 MEDIUM and 3 LOW. All fixed (epic 93 § 93.4 *As built*, "review fixes R93-01…06"). No ruling number was given.
  - R93-01 (MEDIUM): protected targets were classified by spelling, not by where the write lands → fixed.
  - R93-02 (MEDIUM): a classified call did not have exactly one audit row → fixed.
  - R93-05 (MEDIUM): the audit could name a different file from the one changed → fixed.
  - LOW: R93-03, R93-04 and R93-06 were fixed.
- **#463** (`review-epic93-r2.md`). "changes required": 3 HIGH and 13 MEDIUM. All fixed under R174–R181.
  - R93P-01 (HIGH): an approval checked one payload and ran a different one → R174.
  - R93P-02 (HIGH): file preconditions bound the alias, not the destination → R174.
  - R93P-03 (HIGH): a retried request could disclose after the room widened → R175.
  - R93P-04 (MEDIUM): a non-holder could write the decision file → R176.
  - R93P-05 (MEDIUM): a crash between the record and the line lost the action → R176.
  - R93P-06 (MEDIUM): recovery forgot the rest of the round → R176. DW-501 is the residue.
  - R93P-07 (MEDIUM): terminal refusals left pending audit rows → R176. DW-500 is the residue.
  - R93P-08 (MEDIUM): a parked card was still due → R177.
  - R93P-09 (MEDIUM): parking a scheduled card update broke its own precondition → R178.
  - R93P-10 (MEDIUM): a finished continuation left the card blocked → R178.
  - R93P-11 (MEDIUM): a transient failure stranded an accepted approval → R179. DW-502 is the residue.
  - R93P-12 (MEDIUM): continuations ran while the host thought the worker idle → R177.
  - R93P-13 (MEDIUM): a failed upload still published an approvable card → R180.
  - R93P-14 (MEDIUM): history truncation made old approvals unusable → R179.
  - R93P-15 (MEDIUM): the approval writer followed a symlinked folder → R180.
  - R93P-16 (MEDIUM): the live test did not prove the claimed race → R181. DW-503 is the residue.
- **#464** (`review-epic93-r3.md`). "changes required": 1 HIGH, 5 MEDIUM and 2 LOW. All fixed under R182–R184.
  - R3-01 (HIGH): an identity reset could pair the pinned master key with a device the new identity signed → R182.
  - R3-02 (MEDIUM): stale trust after a failed `/keys/query` → R182. DW-512 is the residue.
  - R3-03 (MEDIUM): the claim could be lost during the trust lookup → R183.
  - R3-04 (MEDIUM): a proxy DM could forward a decision to itself forever → R184. DW-511 is the residue.
  - R3-05 (MEDIUM): malformed payloads were copied into the trace → R182.
  - R3-06 (MEDIUM): a file I/O error was recorded as an accepted decision → R183.
  - LOW: R3-07 and R3-08 were fixed (R183, R182).
- **#465** (`review-epic93-r4.md`). "changes requested": 9 untriaged. All fixed under R185–R187.
  - R4-01 (untriaged): the displayed action was never checked against the digest → R185.
  - R4-02 (untriaged): SDK provenance failures counted as seals → R185. DW-516 and DW-518 are the residue.
  - R4-03 (untriaged): a rejected decision disabled every client → R187.
  - R4-04 (untriaged): large actions could be approved unseen → R186.
  - R4-05 (untriaged): the fold followed arrival order, not room order → R187. DW-517 is the residue.
  - R4-06 (untriaged): any agent at power 50 could mark another's action consumed → R185. DW-515 is the residue.
  - R4-07 (untriaged): T4 gating checked the card's room, not the session's → R187.
  - R4-08 (untriaged): a session approval did not say its scope → R186.
  - R4-09 (untriaged): the live test bypassed the production path → `live_card`.
- **#466** (`review-epic93-r5.md`). "changes requested": 5 untriaged. All fixed under R188.
  - R5-01 (untriaged): a second coalesced action reset the first's state → R188.
  - R5-02 (untriaged): a scope-detail shim restored under-disclosure → R188.
  - R5-03 (untriaged): an expired card said nobody decided → R188.
  - R5-04 (untriaged): "consumed" claimed execution → R188.
  - R5-05 (untriaged): the fingerprint went stale → R188.
- **#467** (`review-epic93-r6.md`, GPT-6 Astra per `brief-Fix93r6.md`). "Changes required": 9 untriaged, all under R194.
  - R6-01 (untriaged): a declassification left an approval for a later call → fixed.
  - R6-02 (untriaged): matching bytes released a widened audience → fixed.
  - R6-03 (untriaged): the delayed brief trusted modified arguments → fixed. DW-524 is the residue.
  - R6-04 (untriaged): R193 did not apply to a declassified next round → fixed.
  - R6-05 (untriaged): a blocked surface request could not reproduce its bytes → narrowed to fail closed, and DW-521 rewritten.
  - R6-06 (untriaged): a replacement park left an extra audit row → fixed.
  - R6-07 (untriaged): a restart lost the return route for approvals → fixed.
  - R6-08 (untriaged): an approver whose proxy is on another host could not be asked → narrowed to fail closed. DW-485 was rewritten and DW-525 opened.
  - R6-09 (untriaged): a desktop scan failure kept a revoked trust anchor → fixed.

### Epic 94 — GPT-6 Astra (PR bodies) except the prep rung, whose reviewer is not recorded

- **#468** (`review-epic94-prep.md`). "changes requested": 7 untriaged. R170 covers 01 and 03. The dispositions of 02 and 04–07 are recorded nowhere.
  - E94-PREP-01 (untriaged): `preserve_order` changed which setting wins → R170, which sorts every keeper-owned TOML read and write.
  - E94-PREP-02 (untriaged): the render guard accepted non-contained paths → not recorded.
  - E94-PREP-03 (untriaged): the audit left order-dependent diagnostics → R170.
  - E94-PREP-04 (untriaged): `to_json` changed TOML scalars → not recorded.
  - E94-PREP-05 (untriaged): the equality shim could pick the wrong party group → not recorded.
  - E94-PREP-06 (untriaged): Python escaping missed control characters → not recorded.
  - E94-PREP-07 (untriaged): memlog paths did not keep `pathlib` spelling → not recorded.
- **#469** (`review-94config.md`). "Findings remain": 8 untriaged. All fixed under R195, and DW-530 is closed.
  - R94C-01 (untriaged): read provenance recorded the alias → the landing is recorded.
  - R94C-02 (untriaged): the tools bypassed the drive grant → fixed.
  - R94C-03 (untriaged): a skill-only agent got no frame → fixed.
  - R94C-04 (untriaged): fallback sentences named absent tools → fixed.
  - R94C-05 (untriaged): any MCP tool counted as web search → fixed.
  - R94C-06 (untriaged): `bmad_party` hid unread overlays → fixed.
  - R94C-07 (untriaged): truncated invalid UTF-8 counted as text → fixed.
  - R94C-08 (untriaged): tests pinned prose → pins removed.
- **#470** (`review-94render.md`). "changes requested": 8 untriaged. All accepted under R196.
  - R94R-01 (untriaged): the staging folder bypassed containment → its own staging folder. DW-552 is the residue.
  - R94R-02 (untriaged): verification accepted a symlink → fixed.
  - R94R-03 (untriaged): the journal ran ahead of directory durability → fixed. DW-554 is the residue.
  - R94R-04 (untriaged): an unreadable memlog was overwritten → fixed.
  - R94R-05 (untriaged): render paths did not round-trip → fixed.
  - R94R-06 (untriaged): read failures published partial renders → fixed.
  - R94R-07 (untriaged): declassification bound the arguments, not the generation → fixed.
  - R94R-08 (untriaged): no turns-level coverage → added. DW-553 is the residue.
- **#471** (`review-94ask.md`). "changes required": 13 untriaged. All accepted under R199.
  - R94A-01 (untriaged): the model could write the person's answer → the relay carries the person's own message only.
  - R94A-02 (untriaged): a pinned proxy was then refused by the invite path → fixed.
  - R94A-03 (untriaged): a child's question could be routed into a session that rejects asks → fixed.
  - R94A-04 (untriaged): an ask was sent before its record existed → fixed.
  - R94A-05 (untriaged): an intercepted question had no durable recovery → fixed. DW-546 is the residue.
  - R94A-06 (untriaged): an open ask let a scheduled run start another window → fixed.
  - R94A-07 (untriaged): a scheduled ask's answer did not finish its card → fixed.
  - R94A-08 (untriaged): an answer could start a request over an unresolved parked call → fixed. DW-547 is the residue.
  - R94A-09 (untriaged): a failed leave counted as done → fixed. DW-548 is the residue.
  - R94A-10 (untriaged): answering the first of two open questions made the proxy leave the room the second needed → fixed.
  - R94A-11 (untriaged): a refused send left the run blocked → fixed.
  - R94A-12 (untriaged): permanent send errors retried forever → fixed.
  - R94A-13 (untriaged): the immediate send skipped the room recheck → fixed.
- **#472** (`review-94wf.md`). "Changes are needed": 15 untriaged. All accepted under R202.
  - R94W-01 (untriaged): workflow creation bypassed the label → fixed.
  - R94W-02 (untriaged): the session writer was not fenced by the claim → fixed.
  - R94W-03 (untriaged): card admission checked names, not the offer → fixed.
  - R94W-04 (untriaged): an interrupted opening lost its parent → fixed.
  - R94W-05 (untriaged): a stale takeover could run the first turn twice → fixed.
  - R94W-06 (untriaged): continuations were lost on restart → fixed.
  - R94W-07 (untriaged): takeover resumes exceeded the three-continuation limit → fixed.
  - R94W-08 (untriaged): a takeover could bypass an unanswered checkpoint → fixed.
  - R94W-09 (untriaged): an unreadable declaration disabled the output check → fixed.
  - R94W-10 (untriaged): `reply` was not terminal → fixed.
  - R94W-11 (untriaged): a takeover restored only the last generation → fixed.
  - R94W-12 (untriaged): a late reply marked the newer run reviewed → fixed.
  - R94W-13 (untriaged): the nobody-to-ask raise was not applied everywhere → fixed.
  - R94W-14 (untriaged): dispatch could hand on the same card twice → fixed. DW-542 is the residue.
  - R94W-15 (untriaged): the cross-host proofs were deferred → AC5 and AC8 were proved (PR #472). DW-540 is narrowed and DW-541 open.
- **#473** (`review-94help.md`). "changes requested": 7 untriaged. All accepted under R203, and DW-558 and DW-559 are narrowed.
  - R94H-01 (untriaged): helpers past a parked call ran twice → fixed.
  - R94H-02 (untriaged): forbidden helper steps bypassed the audit → fixed.
  - R94H-03 (untriaged): resume reset the turn budget → fixed.
  - R94H-04 (untriaged): parallel helpers did not count siblings' spend → fixed.
  - R94H-05 (untriaged): a failed completion lost its usage → fixed.
  - R94H-06 (untriaged): cancellation did not end the fan-out → fixed.
  - R94H-07 (untriaged): the acceptance-8 test never gave a helper a diff to read → fixed.
- **#473 restack** (`review-94help-merge.md`). "changes requested": 4 untriaged. All fixed under R215, with R214 as the rule they test.
  - R94HM-01 (untriaged): a budget-stopped workflow stayed `running` → fixed.
  - R94HM-02 (untriaged): helper spend could undo a successful reply → fixed.
  - R94HM-03 (untriaged): the helper and its parent disagreed on which budget wins → fixed.
  - R94HM-04 (untriaged): R214's test never ran a helper → fixed.

## Rulings and deferred work

**Rulings:** R1–R122, R158–R172, R174–R203, R214 and R215. That is 164 entries in `agents-coordinator-decisions-2026-10-02.md`, counting R191a. Several of them have more than one entry: R76, R82, R83, R103 and R105. R34, R68, R70, R71, R78 and R92 have no entry of their own (R70 and R92 are cited by other rulings). Epic 90 also used coordinator rulings D4–D15, which are written only in its epic doc. R173 belongs to epic 97, and R123–R157 and R204–R213 to epics 95 and 96.

Where each epic's rulings came from:

| Epic | Plan (2026-10-02) | During the build |
| --- | --- | --- |
| 89 | P1–P15 and R1–R29 (R28 and R29 are the plan reviews by Fable 5.1, accepted in full) | none of its own; review dispositions are in the PR bodies |
| 90 | — | R30, plus D4–D15 |
| 91 | — | R31–R47 and R72 |
| 92 | — | R48–R69 (the epic's Q rulings), R93–R95, R114–R122 and R158–R169 |
| 93 | — | R73–R91, R171, R172 and R174–R194 |
| 94 | R96–R113 (all 18 pre-numbered rulings used) | R170, R195–R203, R214 and R215 |

**Deferred work:** these DW numbers in `deferred-work.md`:

- DW-355…DW-384 and DW-418 (the plan);
- DW-430…DW-432 (the 2026-10-02 review wave);
- DW-419…DW-422 (epic 91);
- DW-433…DW-435, DW-441…DW-447, DW-452…DW-455, DW-460…DW-463 and DW-470…DW-472 (epic 92);
- DW-485…DW-489 and DW-500…DW-525 (epic 93);
- DW-530…DW-559 (epic 94).

DW-490…DW-497 sit inside that span but are epic 97's. The ledger holds 105 entries for these epics:

| Status | Count | Entries |
| --- | --- | --- |
| open | 91 | |
| closed | 9 | DW-366, DW-368, DW-442, DW-462, DW-523, DW-530, DW-536, DW-537, DW-538 |
| narrowed | 3 | DW-535, DW-539, DW-540 |
| no `status:` line | 2 | DW-453, DW-472 |

DW-433 was opened and closed (R94) and has no ledger entry.

The open DWs with the largest consequence:

- **DW-485:** an approver whose proxy runs on another host is never asked. Since R194, the call fails closed and does not park. With Nixi on electra and a run on hesperia, or the other way round, the action is refused, not decided.
- **DW-430:** a secret outside the nine known shapes is written into the session log. It then reaches the drive's history and every device that syncs the session.
- **DW-418:** on the Mac, a T2 or T3 approval can come from the same keeper that hosts the run, so a compromised keeper could approve its own request.
- **DW-510:** an unattended run's action that is raised to T4 names nobody who may decide it, so the run waits until the approval expires.
- **DW-432:** a conflicted session has no resolve action. A person has to move chunks out of `log/` by hand.

## What went wrong in the process

- **Some reviews shared the author's vendor.**
  - *What happened:* Fable 5.1 reviewed epic 89, epic 90 rungs 1, 2 and 7, and epic 91 rung 2 against Opus 5.5 authors (PR bodies #433–#439 and #446; `review-epic90-r7.md` line 3). No artifact names the reviewer of #440–#443, #445, #447, #448 or #468.
  - *Cost:* those rungs had no model-independent review.
  - *What changed:* from #449 on, every PR body names GPT-6 Astra. The resume note of 2026-10-06 (stack-notes) records that the operator asked for Astra on every BMAD review lane.
- **A transcript-based recovery was refused, 2026-10-05.**
  - *What happened:* recovering the fix lane of epic 93 rung 4 (`Fix93r4b`) by reading a worker's session transcript ended with a provider `reasoning_extraction` refusal (`ctx/artifact-only-rule.md`, Evidence).
  - *Cost:* the lane was redone from artifacts (`brief-Fix93r4.md`, `brief-Fix93r4c.md`).
  - *What changed:* the artifact-only rule. Every resumed worker gets it in its brief. The rule is not in this worktree's decisions file, although stack-notes line 4 says it is.
- **The coordinator died on 2026-10-06 at about 15:57 UTC.**
  - *What happened:* the lanes it ran died with it. The resume note that followed (`brief-resume-2026-10-06.md`) names epic 94's workflows mutation script, `mut-94wf-2.py`. Separately, the 94.3 builder had ended with its turns tests for AC3–AC9 unwritten (`brief-94wf-finish.md`).
  - *Cost:* a finisher lane for 94.3, and lanes restarted from what was on disk.
  - *What changed:* per-lane resume notes, "commit a compiling state early, one commit, amend", and the rule that mutation scripts restore on a signal.
- **The 8 GiB cgroup killed builds.**
  - *What happened:* lanes were OOM-killed (`signal: 9`) during epic 94 (`brief-resume-2026-10-06.md` § Memory; `brief-restack-94help.md`: "OOM-killed before").
  - *Cost:* reruns.
  - *What changed:* one cargo job per lane under a flock (`cargo-<slug>.sh`), and "wait two minutes, rerun once, then report it owed".
- **hesperia was busy or unreachable.**
  - *What happened:* `check:rust:macos` was not run for #438 and #439 ("the Mac was busy"). #452 and #465 were checked by inspection only. For #467, `ssh mac` timed out (PR bodies; epic 93 § 93.3 *As built*).
  - *Cost:* those shell edits merged with only CI's macOS job as their gate. One hesperia run is owed (B11 under *Owed device runs*).
- **The Linux container could not run some tests.**
  - *What happened:* inotify instances ran out. 37 keeper-sync watcher tests failed on #468, and `agent_turns::a_served_sessions_second_turn_opens_no_file_under_log` failed with EMFILE on #472 and #473 (PR bodies).
  - *Cost:* those tests ran only in CI.
- **CI flaked on these PRs.**
  - *What happened:* the recordings-search perf test failed on #467, and the notes-agent-dock D4 timing test on #470 and #473. The jobs were rerun on 2026-10-10 (stack-notes, "CI RED on pushed stack"). On #449, hesperia's run stopped on `recording_search_p95_under_50ms_at_10k_sessions` under load 54 (PR #449).
- **Rungs were built out of order and restacked.**
  - *What happened:*
    - Epic 97's drafts (#459, #461, #462) were built on `agents-92-labels`, between epic 93's PRs, and later restacked twice (stack-notes § Pushed; 2026-10-11 RS9697).
    - Epic 93's rungs 3–5 were each restacked onto their reviewed parents (epic 93 *As built* headers).
    - The 94 prep rung was restacked onto `agents-93-decide` (PR #468).
    - Helpers were restacked onto workflows, and the merge review then found four new defects (R94HM-01…04).
  - *Cost:* a restack round and a restack review per move.
- **Lanes shared one `CARGO_TARGET_DIR`.**
  - *What happened:* found on 2026-10-06, when [INFERENCE from the date] epic 94's and 95's lanes were running. One lane's newer binary passed as fresh in another (stack-notes § Rules learned).
  - *What changed:* `mklane.sh` gives each lane `target-<slug>`.
- **A worker wrote into a sibling worktree through relative paths.**
  - *What happened:* a rung-6 worker wrote into the UI worktree (stack-notes § Rules learned, which does not name the epic). [INFERENCE] It was most likely epic 93: rung 6 was built in `wt-93d` while rung 5's UI fix ran in the main worktree (`brief-E93r6.md`, `brief-E93r5fix.md`). Epic 92's rungs 5 and 6 had the same shape, so it could have been that epic.
  - *What changed:* absolute paths are required, and the main worktree's tracked folders are made read-only.
- **The merge landed main red.**
  - *What happened:* the stack merge through #477 that brought 89–94 to main left `6744d468` CI-red. The cause was epic 95's code (clippy 1.99 at `consolidate.rs:1171`, and okf fixtures under biome), not 89–94's (stack-notes, 2026-10-10 and 2026-10-11 02:00).
  - *What changed:* the `agents-ci-green` rung, #479.
- **A real model did not do what the stub did.**
  - *What happened:* the stewards' triage with claude-haiku-4-5 passed only 1 run of 3 (#457, DW-472).
  - *Cost:* the scheduled triage is unproven with a real model.

## What to keep

- **A review before the next rung builds on a rung, with each finding ruled and the ruling recorded.** Every 92–94 PR body names its review file, the ruling range and the DWs it left. The restack merge review found four composition defects that the single-rung reviews could not see (`review-94help-merge.md`, R215).
- **Live tests against a real homeserver.** Synapse on delectra found three defects that the doubles had missed:
  - a limited timeline lost 12 % of events (#441);
  - the person's device must exist before the DM is made (#445);
  - the target's host refused a declassified brief (DW-523, closed by R193 after the live AC6 run, #467).
- **Measuring before fixing a number.** The largest encrypted final edit measured 47 061 bytes, so R23's 60 KiB would not fit, and `FINAL_CUT_BYTES` became 45 KiB (epic 90 § 90.5 *As built*). The growth of a 10 000-line session was measured before any rotation rule was argued over (`docs/agents.md` § *What a session costs the drive*).
- **Mutation-proving every acceptance test.** The PR bodies record the counts: 62 killed on #472, 42 on #463, 37/37 on #471. Each review fix carried its own mutant.
- **Failing-first proofs.** PR #450 shows its tests failing with the fix reverted.
- **Small rungs, and an *As built* section per rung that supersedes the plan.** 14 planned rungs became 37 reviewable PRs. Every rung's *As built* and *review fixes* sections say what changed against the plan, so the epic docs are the record.
- **Plan-level adversarial reviews accepted before any code.** R28 and R29 turned 60 findings into amendments; A1–A4 landed in epic 89 while it was being built.

## Owed device runs

There are 24 owed device runs on hesperia (the Mac), kalypso (the iPhone) and electra (the server); none on delectra, because every delectra live run is recorded as passed, and none on the Android tablet or a NanoKVM. Most wait on the operator's production rollout on electra (AI-89-94-2). `docs/owed-device-runs.md` lists epics 95–99's runs; 89–94's are here.

There are 24. By device, hesperia is in 16, kalypso in 4 and electra in 13 (a row with two devices counts for each). "OP-n" is an operator prerequisite, listed after the table.

| # | device | item (epic.story #acceptance) | what to do | what proves it | operator prerequisite | source |
| --- | --- | --- | --- | --- | --- | --- |
| B1 | hesperia Mac | 89.2 #13 | On the installed build, put `[folder.agents]` in a scratch folder's `.keeper/keeper.toml`. Open Advanced settings and Files. Remove `[folder.sessions]`. | The flag shows as the folder file's; Files labels `80-agents`; acceptance 4's sentence appears. | OP-8 | epic-89 § 89.2 #13; sprint-status epic-89 "Owed when built" |
| B2 | hesperia Mac | 89.3 #12 | Ask a ⌘9 bot with a write grant on a scratch drive to edit `80-agents/nixi/SOUL.md`. | The tool result is the `AgentHome` sentence and the file is unchanged. | OP-8 | epic-89 § 89.3 #12 |
| B3 | hesperia Mac | 89.6 #10 | In Settings › Bots, add an `openai` provider at CLIProxyAPI's URL with its token. Press *Test*, pin a model, and chat in ⌘9 with a read grant on a scratch drive. | *Test* reads Online; the answer shows a `drive_read` step; the egress list shows the host once. | OP-8; the CLIProxyAPI token (R13) | epic-89 § 89.6 #10; sprint-status epic-89 |
| B4 | hesperia Mac | 90.1 #9 | Use one `keeper.db` with a build from before the extraction and one from after. In each, run a typed turn with a drive read, a spoken turn with a write that asks, a task *Run now*, Stop mid-stream, and follow while streaming. Query `bot_messages`, `bot_audit` and `task_runs`. | The rows are identical apart from ids and times. | the two installed builds | epic-90 § 90.1 #9; PR #438 "Awaiting hesperia"; sprint-status epic-90 |
| B5 | hesperia Mac | 90.2 #10 | Create, archive and unarchive a session in a scratch zone. `kill -9` mid-create (a dev build with a debug sleep), then relaunch. | The next start resumes the plan, with one session per id. | a dev build | epic-90 § 90.2 #10; PR #439 |
| B6 | hesperia Mac + electra | 90.6 #14 | Sign `nixi` in under Settings › Agents. Run `sudo systemctl kill -s KILL keeper-agentd@tgorka` on electra, message Nixi, then restart electra's daemon. | Within 180 s plus a tick the status reads `nixi@hesperia` and the chunk is `<date>.hesperia.<n>.jsonl`; after the restart the hand-back happens at idle and the status reads `nixi@electra`. | OP-1, OP-2, OP-3 | epic-90 § 90.6 #14; PR #444 |
| B7 | hesperia Mac | 91.1 | Open a real agent room in the installed app (real WKWebView). | The header with its run state, scope and label chips; one growing answer that is never marked edited; the Agents window. | OP-2, OP-3 | epic-91 § 91.1 (line 229); § Owed when built |
| B8 | hesperia Mac | 91.2 | Use the dock beside two open notes in the installed app. | The proxy is listed, the scope chip works, focus follows the note, and the layout holds. | OP-3 | epic-91 § 91.2 (line 272); PR #447 |
| B9 | hesperia Mac + kalypso iPhone | 91.3 #9 | With `nixi@electra` serving, leave the Mac unfocused and the iPhone in front, and have Nixi call a surface tool. | The request lands on the iPhone (the focused device) and its answer releases the turn. | OP-2, OP-3 | epic-91 § 91.3 (line 316); PR #448 |
| B10 | hesperia Mac + kalypso iPhone | 91.4 #7 | Say "Hej Nixie" (or the person's wake phrase), ask a question, then say the stop phrase. | The answer is spoken while it streams, and the stop phrase stops it. | OP-3 | epic-91 § 91.4 #7; PR #449 |
| B11 | hesperia Mac (gate) | 89–94 shell edits checked by inspection | Run `bun run check:rust:macos` on main (`6744d468` or later). Use nextest with `ulimit -n 65536` (stack-notes, 2026-10-11 RS9697). | fmt, workspace clippy and all tests pass, including the keeper-sync sweep test of DW-453. | none | PR bodies #433–#435, #438 and #439 ("not run"), #452, #465, #467 ("ssh mac timed out"); DW-453 |
| B12 | hesperia Mac + electra | 92.2 #9 | Have a real session with an agent card on hesperia's board while `nixi@electra` runs it. | The card shows its run badge and where it runs. | OP-1, OP-2, OP-3 | epic-92 § 92.2 #9 (line 240); DW-443 |
| B13 | electra + hesperia | 92.3 | Let `nixi@electra` run a real `@daily` card while hesperia is away, then let hesperia run it. | One run per window, on one host; `last_run` equals the window; a takeover settles the window. | OP-1, OP-2, OP-3 | epic-92 § 92.3 *As built* (line 299); PR #456; DW-454 |
| B14 | electra + hesperia | 92.4 #7 (NFR-122) | Push 100 session changes from electra (`agentd-tgorka`) and 100 from hesperia. | p95 until the other host has the commit by doorbell is ≤ 15 s, recorded in `docs/agents.md`. | OP-1, OP-2, OP-3 | epic-92 § 92.4 #7 (line 339); `docs/agents.md:4444`; PR #455; DW-452 |
| B15 | electra | 92.5 | Let Dr Tola Grey run a real `@daily` triage of tgdrive. | A card per inbox letter, safe letters delegated, nothing unsafe scheduled. | OP-1, OP-2, OP-3 | epic-92 § 92.5 (line 393); `docs/agents.md:4051`; PR #457 |
| B16 | electra | 92.6 | Have Nixi hand off to Dr Lucyna Novak on the real drives. | The hand-off is refused with Marta named (or parks as a *declassify* card, after 93). | OP-1 to OP-5 | epic-92 § Owed when built (line 527) |
| B17 | hesperia Mac + kalypso iPhone | 93.3 #8 | Bring up a pending approval card on both devices and decide it on the iPhone. | The Mac's card turns approved; recorded in `docs/agents.md` § *Measured* with the date, build and devices. | OP-2, OP-3, OP-4 | epic-93 § 93.3 #8; § Owed when built (line 397); PR #466, #467 |
| B18 | electra | 93.2 / 93.4 | Let a scheduled run on electra park at night, and approve it in the morning. | The run resumes and the approval is consumed once. | OP-1 to OP-4 | epic-93 § Owed when built (line 397); PR #467 |
| B19 | electra + hesperia | 93.2 #5, #10 | Take over a parked session from electra to hesperia, or back. | The approval is consumed exactly once. | OP-1 to OP-4 | epic-93 § Owed when built; PR #467; DW-487, DW-503 |
| B20 | kalypso iPhone + electra | 94.3, operator-verified | Start a card for `bmad-create-epics-and-stories`, assigned to a specialist, from the board. At its first checkpoint, answer `C` from the phone through Nixi. | The run resumes on electra. | OP-1 to OP-4, OP-6 | epic-94 § 94.3 *Operator-verified* (lines 573–576); PR #472 |
| B21 | electra | 90.4 #10 (NFR-112) | Run `p95_delivery_between_two_copies` against tuwunel with keeper-smoke-a and keeper-smoke-b, and read electra's tuwunel rate limits. | p50, p95 and p99 recorded in `docs/agents.md` § *Measured*. | OP-7 | epic-90 § 90.4 #10; PR #441 |
| B22 | electra | 90.5 #21 / 91.1 #8 (NFR-113) | Run `nfr_113_holds_over_fifty_turns` pointed at tuwunel. | p95 to the anchor and to the final edit ≤ 1 s, recorded. | OP-7; OP-2's agent users | epic-90 § 90.5 #21; epic-91 § Owed when built; PR #442 |
| B23 | electra | 90.6 #15 | Rerun 90.4's acceptance-9 race on tuwunel. | The ordering is recorded. | OP-7 | epic-90 § 90.6 #15 |
| B24 | electra | 90.5 #15 | Run `systemd-analyze security keeper-agentd@tgorka` after the install. | The exposure score is recorded in `docs/agents.md` § *Measured*. | OP-1, OP-2 | epic-90 § 90.5 #15 |

Operator prerequisites (these are operator actions, not device runs):

- **OP-1 (90.3 #9):**
  - OS users `agentd-tgorka`, `agentd-marta` and `agentd-neuraffica` on electra.
  - Forgejo `write:repository` tokens in `/etc/keeper-agentd/<p>/` (mode 0600, for `LoadCredential=`).
  - The `[[drives]]` owner and reader pins, copied from the Forgejo collaborators.
- **OP-2 (90.5 #15):** install `keeper-agentd` and `keeper-agentd@.service`; register the agent users on tuwunel through a registration-token window; run `init`, edit `agentd.toml`, run `login <drive>/<agent>`; then `systemctl enable --now keeper-agentd@<p>`.
- **OP-3 (91.5, operator actions 0–4):**
  - The owner writes the provider record in `docs/agents.md` § *The provider* (S-20).
  - The tuwunel users `@nixi`, `@tola-grey` and `@lucyna-novak` are created and their copies logged in.
  - Every machine is upgraded before `[folder.agents]` lands in each drive's `.keeper/keeper.toml`.
  - tgdrive is seeded through the keeper lane and an `fj` PR.
  - neuradrive is seeded from `agentd-neuraffica`'s own checkout.
- **OP-4 (epic 93):** `[[trust]]` pins in each `agentd.toml`, written after comparing the fingerprint `keeper-agentd status` prints with Settings › Account. On `agentd-neuraffica`, tgorka's entry gets `proxy = "@nixi:…"`.
- **OP-5 (epic 92, Q15):** tgorka invites `@lucyna-novak` into `agentd-tgorka`'s control room, and `agentd-neuraffica` joins it.
- **OP-6 (OA-94-2):** a BMAD skill with its `workflow.toml` in tgdrive's `80-agents/_workflows/`. OA-94-1, fixing the plugin template, is optional.
- **OP-7 (90.4 #10):** a tuwunel registration-token window and the smoke users `keeper-smoke-a` and `keeper-smoke-b`.
- **OP-8:** a scratch folder synced by the installed build on hesperia, never the owner's drives (C9).

Notes:

- **delectra:** nothing owed. Every delectra live run is recorded as passed in PR bodies #441–#471, including OA-94-3's `ask_human_round_trip_on_synapse` (#471).
- **Android tablet and NanoKVM:** nothing in 89–94.
- **Fits no device:**
  - 90.5's `workflow_dispatch` release dry run with the real signing key (GitHub Actions; PR #442 "Awaiting").
  - The owner's provider record (docs).
  - OA-94-1 (optional).
- **Done, but sprint-status's "Owed when built" comments still list them:**
  - 89's CLIProxyAPI live test (PR #433) and the golden `prompt_tokens` (`docs/agents.md:750–753`).
  - 89's 10 000-line growth measurement (`docs/agents.md` § *What a session costs the drive*).
  - 90's delectra Synapse users and live tests (PR #441–#443).
  - 90's measured Megolm cut (`FINAL_CUT_BYTES`, PR #442).
  - 92's quiet-folder failing run, recorded in PR #450's body.

## Action items

| id | action | owner | status |
| --- | --- | --- | --- |
| AI-89-94-1 | Write the owner's provider record in `docs/agents.md` § *The provider*: CLIProxyAPI's upstreams, the cloak-mode setting, and the owner's acceptance of the terms risk. It must come before any real drive is seeded (S-20; 91.5 operator action 0). | operator | open |
| AI-89-94-2 | Do the production rollout on electra, in the epics' order: 90.3's OS users, Forgejo tokens and pins; 90.5 #15's binary, unit and tuwunel agent users; 91.5's seeding of tgdrive (keeper lane and `fj` PR) and neuradrive; 93's `[[trust]]` pins after comparing fingerprints; 92's invite of `@lucyna-novak` (Q15); OA-94-2. | operator | open |
| AI-89-94-3 | Run the 24 owed device runs listed under *Owed device runs* (B1–B24). Record each one in `docs/agents.md` § *Measured* where its epic says so. | operator | open |
| AI-89-94-4 | Rerun with a non-Anthropic reviewer the reviews that shared the author's vendor: #433–#436, #438, #439, #444 and #446. Name the reviewer of #440–#443, #445, #447, #448 and #468, or review them again. | future lane (bmad-review, GPT-6 Astra once its quota returns) | open |
| AI-89-94-5 | Reconcile the records (docs only): see the list after this table. | future lane (bmad-scribe) | open |
| AI-89-94-6 | Build the two-host `Pair` harness that DW-487 and DW-503 describe. Close DW-454, DW-487, DW-503 and DW-541 with it: claim, park, crash and takeover across two host runtimes and two git checkouts. | future lane (bmad-dev) | open |
| AI-89-94-7 | Rule who decides an unattended run's T4 action (DW-510). Today every such run waits until its approval expires. | future lane (coordinator ruling) | open |

The records to reconcile under AI-89-94-5:

- Copy epic 90's D4–D15 into the decisions file.
- Give R34, R68, R70, R71, R78 and R92 entries, or mark them unused.
- Record the dispositions of R34-01…15, R7-01…11 and E94-PREP-02 and 04–07.
- Add `status:` lines to DW-453 and DW-472, and a ledger entry for DW-433's closure.
- Tick 94.2's OA-94-3 box, since #471 records the live run.
- Commit the artifact-only rule to the decisions file.

## The program as a whole (89–99)

Written by the close-out lane (2026-10-13) over the six retrospectives: this file for 89–94 and
`epic-95-retro-2026-10-13.md` … `epic-99-retro-2026-10-13.md` for the rest.

**Where it stands.** 50 stories in 11 epics. Epics 89–94 and stories 95.1–95.4 are merged (#432 the plan,
#433–#477: 42 PRs). The rest — 95.5 and epics 96–99 — is pushed and open as 30 PRs in GitHub stack #486
(#479–#485, #487–#506, and epic 97's #459, #461, #462), CI green except named flakes, every rung reviewed
and its findings ruled and fixed. Main itself is CI-red at `6744d468` until #479 (`agents-ci-green`) merges.
The last ruling is R379 (R313 and R314 written by this close-out); the last deferred item is DW-1380 (opened
by this close-out). 107 device or host runs are owed — 24 for 89–94 (above) and 83 for 95–99
(`docs/owed-device-runs.md`) — and none of them has been run: no agent has yet worked on the owner's real
drives, no push has reached a phone, and nothing has been installed on the Android tablet.

| epic | stories | PRs | reviewer of record | rulings | deferred work | owed runs |
| --- | --- | --- | --- | --- | --- | --- |
| 89–94 | 31 done | 37 + #432 merged | Fable 5.1 (89, parts of 90/91), GPT-6 Astra (92–94), some unrecorded | R1–R122, R158–R172, R174–R203, R214, R215 | 105 entries, 91 open | 24 |
| 95 | 4 done, 1 review | 4 merged, 3 open (#479–#481) | GPT-6 Astra (39 rounds); #479 unreviewed | R123–R140, 54 of R204–R300 | 138 entries, 116 open | 12 |
| 96 | 5 review | 6 open | Astra, then an Opus task lane (3 reviews), then Fable 5.1 | R141–R157, then 62 numbers in R213–R328 (incl. R313/R314) | 115 entries, 94 open | 23 |
| 97 | 3 review | 6 open | Astra (rungs 1–3), Fable 5.1 (4–6) | R173, R317–R319, R324–R327, one unnumbered | DW-402…406, 490…497, 1200…1221 | 8 |
| 98 | 4 review | 10 open | Fable 5.1 | R329–R364 | DW-1230…1329, 56 open | 29 |
| 99 | 2 review | 5 open | Fable 5.1 | R365–R379 | DW-1330…1372, 26 open | 11 |

**Reviewer independence, stated plainly.** Claude Opus 5.5 wrote every rung. GPT-6 Astra — a different
vendor's model — reviewed 91's last rung, all of 92–95, 96's first four rungs and 97's first three. When Astra's quota ran out (429
`usage_limit_reached`, late on 2026-10-10), every later review was Claude Fable 5.1: a different model, the same
vendor, so those reviews are **not fully model-independent**. That covers 96's screen and KVM, 97's heard,
pause and mark, all of 98 and all of 99, and Fable also reviewed 89 and parts of 90 and 91 earlier. Three
96 reviews ran on an Opus task lane — the author's own model — and are not independent at all. No fix round of
98 or 99 was re-reviewed; #479, CI486's edits, RS9697's merge and R328's fixes have no review file. AI-P-4
asks for one non-Anthropic pass over all of it.

### What went wrong across the program

- **The 8 GiB cgroup.** Every lane, review, tsc and rustc shared one container limit. The kernel's oom_kill
  counter reached 131 by 2026-10-10 and 194 on 2026-10-11; 97heard, F96kvm1, F96screen1 and F9697 were
  killed mid-work, and coordinators died with their lanes several times (10-06, 10-07, 10-10, 10-11). What
  changed: one cargo job at a time under `cargo.lock`, every JS gate under the same lock, idle Paseo agents
  archived the moment their report is read (each kept a 200–500 MB omp process), finisher lanes that first
  separate intended fixes from a dead lane's leftover mutants.
- **The Paseo daemon restart, 2026-10-11 06:16:55**, aborted every session mid-turn, and the operator's client
  archived the agents. Lanes were redispatched fresh from `ctx/brief-resume-2026-10-11c.md`; nothing was
  recovered from transcripts.
- **A transcript-based recovery hit a provider refusal (2026-10-05)** → the artifact-only rule: state is
  recovered from briefs, reports, reviews, git and logs, never from session transcripts.
- **Hosts ran out of room.** delectra filled its disk (docker build cache, `kg-target`) on 10-11 and again at
  99 % on 10-12, and mutation runs there failed with ENOSPC that first read as compile failures (96screen
  M05–M09); hesperia was down to ~19 GB free on 10-11 and timed out over ssh repeatedly.
- **The desktop's CDP token leaked**, URL-encoded, into the 98tablet lane's transcript on 2026-10-12, through
  a tool error. No later lane printed it, but the token must be rotated (AI-P-1).
- **Flaky CI.** recordings_search_p95 (perf), notes-agent-dock D4 (timing), add-folder-form "reports a draft…",
  the nse perf test, CodeMirror-not-mounted ×7 and a runner's SQLite `disk I/O error` on #499 each turned a
  green rung red at least once; reruns passed on identical code. Locally, inotify EMFILE fails
  `agent_turns` and the keeper-sync watchers.
- **`check-macos.sh` cannot pass on hesperia.** It runs `cargo test --workspace` over ssh, whose session starts
  at `ulimit -n 256`; `agent_turns` then fails (153 failed and one hang at the 97 tip; `parks::*` ×105 at
  98tablet) while `cargo nextest` with `ulimit -n 65536` passes the same tree (7493/7493, 7607/7607). DW-1380.
- **`rerere` replayed a stale resolution** during RS9697's restack; the restack was redone with rerere off, and
  RS98a ran every rebase that way.
- **Stacking costs.** Epic 97's drafts were built early on `agents-92-labels` and restacked twice; every fix to
  a lower rung cascaded rebases up the stack, and restack deltas were mostly never reviewed.

### What to keep

- One rung, one reviewer, one ruling per finding, recorded with its number before the next rung builds on it.
- An *As built* section per rung naming the symbol and the test for every acceptance item; the epic doc is the
  record, and the planning text above it is history.
- Mutation-proving each new behaviour test, and refusing to close a survivor by deleting the code it mutated.
- Live `#[ignore]` tests against delectra's Synapse; they found defects the doubles missed.
- Pushing early so CI compiles what the container cannot: the first iOS-shell compile (#492–#497) and the first
  Android build (#498) came from CI, not from a device.
- Artifact-only recovery and per-lane briefs that survive any coordinator.
- Device runs written as owed, never claimed.

### Program action items

| id | action | owner | status |
| --- | --- | --- | --- |
| AI-P-1 | Rotate `AGENT_DESKTOP_TOKEN` (deskctl's bearer): it leaked URL-encoded into the 98tablet lane's transcript on 2026-10-12. Update every stack's `.env` that carries it. | operator | open |
| AI-P-2 | Make `scripts/check-macos.sh` run `cargo nextest run --workspace` under `ulimit -n 65536` instead of `cargo test` (DW-1380). | future lane (tooling) | open |
| AI-P-3 | Merge stack #486 bottom-up, #479 `agents-ci-green` first — main `6744d468` is CI-red without it. | operator | open |
| AI-P-4 | One independent review pass with a non-Anthropic model (GPT-6 Astra once its quota returns, or the `mixed` profile) over everything only Anthropic models reviewed and every unreviewed delta: AI-89-94-4, AI-95-8, AI-95-9, AI-96-1, AI-96-2, AI-97-3, AI-98-9, AI-99-2. | future lane (bmad-review / bmad-audit) | open |
| AI-P-5 | Work through the 107 owed runs: B1–B24 in this file, then `docs/owed-device-runs.md`, starting with what needs no purchase (the free-team iPhone runs, hesperia's screen and MCP runs, electra's sandbox status). Record each where its row says. | operator | open |
| AI-P-6 | Bring the planning records up to date (docs only): every epic 89–99 header still says `status: planned 2026-10-02` and its *Stack rungs* lists the planned rungs; stack-notes § *Numbering* gives R158–R164 to 97 (they are 92's) and DW blocks 640–699 that were never used; number 97 mark's `R-NEW-1` (AI-97-4); commit the artifact-only rule to the decisions file (AI-89-94-5); fix the Android docs CI made stale (AI-98-7). | future lane (bmad-scribe) | open |
| AI-P-7 | Decide the container's budget: raise the cgroup above 8 GiB or cap the coordinator at one compiling lane plus reviews. Several OOM kills a day cost a finisher lane each. | operator | open |
| AI-P-8 | Keep delectra's disk from filling: a scheduled prune of `~/kg-target` and docker build cache (a makistack change). | operator | open |
| AI-P-9 | Fix or quarantine, each with a DW, the CI flakes named above (recordings_search_p95, notes-agent-dock D4, add-folder-form draft, nse perf, CodeMirror-not-mounted). | future lane (bmad-dev) | open |

**Open rulings needing the owner.** None blocks a PR of the stack: every finding the reviews raised has a
ruling. These decisions are the owner's and are open, each tracked in its epic's action items: who decides an
unattended run's T4 action (DW-510, AI-89-94-7); lower consolidation thresholds for `owner` proposals
(epic 95 Q4, AI-95-7); the vision sink that lets a model see a screen or KVM frame (DW-1163/DW-1175, AI-96-4 —
until then FR-813 is unmet); whether an outside system may learn the host's name and the session's path
(DW-1335/DW-1345, AI-99-5); whether any webhook-only system needs a gate (DW-415, AI-99-3). One record needs the
coordinator, not the owner: 97 mark's `R-NEW-1` — numbered R380 at close-out.
