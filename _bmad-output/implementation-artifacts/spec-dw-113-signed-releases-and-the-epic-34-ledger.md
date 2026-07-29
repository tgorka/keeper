---
title: 'A release cannot pretend it was signed, and the ledger says what Epic 34 found'
type: 'chore'
created: '2026-07-29'
status: 'review'
baseline_revision: '88452c1'
---

<intent-contract>

## Intent

**Problem:** Two pieces of housekeeping that had drifted in opposite directions.

1. **The release workflow treated Apple signing as optional.** With the `APPLE_*` secrets absent it
   built an ad-hoc bundle, published it as a draft titled `keeper <tag> (unsigned build)`, and said so
   in a `::notice::` — on an otherwise green run. The repository holds only
   `TAURI_SIGNING_PRIVATE_KEY`/`_PASSWORD`, so v0.4.0, v0.4.1 and v0.4.2 all shipped that way
   (DW-113). `spctl -a` rejects the result, and the worse leg is latent: macOS binds keychain ACLs to
   the signing identity, an ad-hoc signature's identity is a cdhash that changes on every build, so an
   ad-hoc build cannot read what a differently-signed build wrote and every update would sign the user
   out. The owner runs a locally-signed build instead of a release artifact for exactly this reason.
   The gate was also weaker than it looked: `signed=true` required only `APPLE_CERTIFICATE` and
   `APPLE_API_KEY_P8_BASE64`, so a repository missing `APPLE_API_ISSUER` (or the identity, or the
   keychain password) took the signed path and failed sixteen minutes in — or built and notarized
   nothing while reporting success.
2. **The deferred-work ledger was wrong in both directions.** DW-114 (no filesystem watcher) and
   DW-117 (`Keeper-Source: watch` on every commit) were fixed by stories 34.9 and 34.10 and still read
   `status: open`; and three things Epic 34 surfaced and did *not* fix were recorded nowhere — a
   diagnosed-but-blocked macOS viewport defect, a deliberate refusal that reads like an unfinished
   list, and a CI hang.

**Approach:** For (1), make an unsigned release structurally impossible rather than merely labelled: a
preflight step beside the existing tag-vs-version check names every missing Apple secret and fails the
job in seconds, and the entire ad-hoc branch is deleted, so there is no code path that can publish an
unsigned artifact at all. For (2), close DW-114 and DW-117 with resolutions that state what is fixed
*and* what is not, and add DW-118 (the 56 px obscured content inset, open, naming the blocked policy
decision), DW-119 (`target`/`dist`/`build` refused as tier 0, closed as wont-fix-by-design) and DW-120
(the durability-sweep hang, whose cause story 34.11 identified, with the upstream gitoxide loop left
open).

## Boundaries & Constraints

**Always:** Secret *names* only ever reach a log line; values arrive as `env:` and are tested with
`[ -n ]`, never interpolated into a script body — the same script-injection discipline the "Egress diff
note" step already documents. The preflight runs before `setup-bun`, the toolchain and `bun install`, so
a repository that cannot sign learns it in seconds rather than after a build. Every ledger entry keeps
the file's existing shape (`origin`/`location`/`reason`/`status`, plus `resolution` when closed,
`note`/`blocked`/`decision` where the file already uses them) and every claim in a new entry names a
file, a line, a test or a CI run a reader can check.

**Block If:** (none.)

**Never:** Do not invent secret names — the seven required are exactly the ones the workflow and
docs/release.md already reference. Do not touch `src-tauri/.config/nextest.toml`: it is story 34.9's
guard, it is correct, and DW-120 records it rather than changing it. Do not renumber or reword any
ledger entry below DW-114. Do not restate story 34.11's diagnosis as this spec's own work — DW-120
cites it. Do not weaken the updater-key leg into scope (see Design Notes).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Today's repository, tag pushed | no `APPLE_*` secrets at all | job fails at step 2 in seconds; `::error::` names all seven; nothing built, nothing published | red run on the tag; GitHub emails the actor |
| Cert + API key only | what the old `signed=true` accepted | fails, naming the five that are missing — it no longer proceeds to a build that cannot notarize | same |
| One secret pasted blank | e.g. `KEYCHAIN_PASSWORD` set to `""` | fails, naming that one | same |
| All seven present | fully provisioned | decode the `.p8`, import the cert, sign the sidecar, `tauri-action` signs + notarizes + publishes the draft | unchanged from before |
| Tag disagrees with the app version | `v0.7.0` vs `0.6.3` | still fails first, at step 1 — the cheaper check keeps its place | unchanged |
| App job fails for any reason | including the new gate | `keeper-syncd` binaries still publish: that job is `needs: release` + `if: always()` | unchanged |
| Egress diff note | app job failed | still runs (`if: always()`), so the release surface is still summarised | unchanged |
| Re-run after adding secrets | same tag, re-run workflow | proceeds normally; `gh release view` idempotence in the syncd job is unchanged | unchanged |
| Reader opens the ledger | wants Epic 34's outcome | DW-114/117 closed with what-is-and-is-not; DW-118/119/120 present with checkable evidence | n/a |
| Someone reaches for `target` in tier 0 | reads DW-119 | finds the refusal, the reasoning and the test that fails on it | n/a |

</intent-contract>

## Code Map

- `.github/workflows/release.yml` -- new `Apple signing material must be present` step immediately
  after the tag check; `Detect Apple signing secrets` becomes `Decode the notarization API key` (no
  `signed` output, no `::notice::`, no fallback); the three signed-path steps lose their
  `if: steps.signing.outputs.signed == 'true'` conditions and their now-meaningless `(signed path)`
  name suffixes; the redundant empty-`APPLE_SIGNING_IDENTITY` check inside the sidecar step is dropped
  (the preflight owns it); the two unsigned-path steps — `Build unsigned (ad-hoc) bundle` and
  `Publish unsigned draft release`, 119 lines — are deleted. The `syncd` job is untouched.
- `docs/release.md` -- "Required GitHub secrets" gains an "All of them are required" paragraph naming
  the failure behaviour and DW-113; the sidecar section's "Signed path / Unsigned path" pair collapses
  to the one path that exists.
- `scripts/build-keeper-rec.sh` -- one comment: "CI's unsigned path" became "CI's `--no-bundle` build",
  which is what that sentence was always about.
- `_bmad-output/implementation-artifacts/deferred-work.md` -- DW-113 gains a dated `note:` for the
  workflow half and stays open, because its own `location` names the repository's Actions secrets and
  those are still absent; DW-114 closed (its 2026-07-27 partial resolution demoted to a dated `note:`,
  per DW-29's precedent, so the entry has one `resolution:`); DW-117 closed; DW-118, DW-119, DW-120
  appended.

## Tasks & Acceptance

**Execution:**
- [x] `release.yml` -- add the preflight gate for all seven Apple secrets, before any build step. --
  A release that cannot be signed fails in seconds, and names what is missing.
- [x] `release.yml` -- delete the ad-hoc build and its publish step; make the remaining steps
  unconditional. -- No code path can publish an unsigned artifact, so "looked successful" cannot
  happen.
- [x] `docs/release.md`, `scripts/build-keeper-rec.sh` -- remove the two places that still describe an
  unsigned release path. -- The runbook and the workflow agree.
- [x] `deferred-work.md` -- close DW-114 (watcher armed per enabled profile; Linux close-write reachable;
  macOS bounded by the settle window; paced scan unchanged; teardown deliberately unwaited) and DW-117
  (`SyncSource` threaded to both commit sites; `Bot` derived from the worktree lane). -- Accurate, and
  it does not claim the machine checks that were not run.
- [x] `deferred-work.md` -- add DW-118 (obscured content inset; `status: open`; `blocked:` names the
  `unsafe_code` policy decision), DW-119 (tier-0 refusal; `status: closed`, wont-fix by design),
  DW-120 (the sweep hang: ruled-out list, story 34.11's cause, the upstream loop still open, and the
  nextest guard as a `note:`). -- What Epic 34 found and did not fix is now on the record.
- [x] `deferred-work.md` -- record the workflow fix on DW-113 without closing it. -- The defect had
  two halves and only one of them lives in this repository; claiming the entry would overclaim.

**Acceptance Criteria:**
- Given no Apple secrets, when a `v*` tag is pushed, then the Release job fails within seconds with an
  `::error::` naming all seven, and no release, draft or asset is created by the app job.
- Given only the two secrets the previous gate checked, then the job still fails rather than starting a
  build it cannot notarize.
- Given all seven, then the workflow behaves exactly as the old signed path did.
- Given the app job failing, then `keeper-syncd` artifacts still publish and the egress note still runs.
- Given a reader of the ledger, then DW-114 and DW-117 read `done 2026-07-29` with resolutions that
  separate what is event-driven / provenanced from what is not.
- Given DW-118, DW-119 and DW-120, then each names a file, line, test or CI run that can be checked
  without asking anyone.

## Design Notes

**Fail the job; do not publish-and-mark.** The two options were failing outright and publishing an
unmistakably-marked release. Publishing-and-marking has already been tried, three times: v0.4.0-v0.4.2
each carried the title `keeper <tag> (unsigned build)` and release notes that said "Unsigned (ad-hoc)
macOS build — first launch requires right-click → Open". It did not work, and the reason is structural
rather than a matter of wording: a draft release is a page you have to open, while the dashboard — the
surface anyone actually reads — stays green. Making the mark louder cannot fix that, because the mark
lives on the artifact and the lie lives on the run. The repo already has the pattern that does work,
eleven lines above: the tag-vs-version check fails hard, in seconds, with its comment saying "Fail here,
in seconds, rather than 16 minutes into a build that can only ship a lie." An unsigned build is the same
class of lie — worse, actually, because a version mismatch only re-offers an update while an ad-hoc
signature costs the user their keychain. Consistency with an existing, deliberate precedent beat
inventing a second convention for "release that is knowingly broken".

**Why the whole ad-hoc branch had to go rather than sit behind the gate.** Leaving it as unreachable
steps would mean a reader cannot tell whether the workflow can publish unsigned, and a future edit to
one `if:` would re-enable it silently. Deleting it makes the acceptance criterion a property of the
file, not of a condition. It also removes a real trap: the ad-hoc path shipped *signed updater
artifacts*, so in-app auto-update worked between ad-hoc builds — each with a fresh cdhash identity, each
therefore losing the keychain items the last one wrote. That path did not merely fail to fix DW-113's
keychain leg; it was the mechanism that would deliver it repeatedly.

**No escape hatch, on purpose.** A bootstrap knob (a variable, a tag suffix, a `workflow_dispatch`
input) was considered and rejected. A tag suffix cannot work — the tag must equal
`tauri.conf.json.version` exactly — and the other two are a new named surface whose only purpose is to
re-enable the defect. The cost of having no hatch is bounded and visible: until the secrets exist, tags
produce a red app job and still publish `keeper-syncd`, whose own job is ordered after this one with
`if: always()` precisely so that a macOS problem does not withhold a Linux binary. If the owner ever
does want an unsigned bootstrap artifact, `bun run tauri:build:signed`'s sibling path already produces
one locally, and it does not carry a release's implicit promise.

**Why all seven secrets, not the two the old gate read.** DW-113's harm is Gatekeeper rejection, and
Gatekeeper wants a Developer ID signature *and* a stapled notarization ticket. tauri-action notarizes
only when `APPLE_API_ISSUER`, `APPLE_API_KEY` and `APPLE_API_KEY_PATH` are all populated and otherwise
skips it quietly, so a repository with a certificate and a `.p8` but no issuer id would have published a
signed-but-un-notarized build with a green run — the same defect, one step further along. Requiring the
full set is what makes the job name ("Signed, notarized macOS release") true. The seven are exactly the
names already in the workflow and docs/release.md: `APPLE_CERTIFICATE`,
`APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `KEYCHAIN_PASSWORD`, `APPLE_API_ISSUER`,
`APPLE_API_KEY_ID`, `APPLE_API_KEY_P8_BASE64`.

**Seven `[ -n ]` lines instead of a loop.** A loop over indirect expansion (`${!name}`) is shorter and
worse here. The macOS runner's bash is 3.2, where `${#missing[@]}` on an *empty* array trips `set -u` —
i.e. the array idiom fails precisely in the success case — and the whole point of this step is to be
correct by reading, in a file nobody can test locally. Seven explicit lines have no version-dependent
behaviour and read as a checklist, which is what they are.

**`APPLE_SIGNING_IDENTITY`'s inline check was removed, not kept as belt-and-braces.** Its comment
explained that `signed==true` did not cover the identity string; with the preflight covering it, keeping
the check would leave two authorities for one fact and invite them to drift. The preflight is the gate.

**The updater keypair is deliberately out of scope.** `TAURI_SIGNING_PRIVATE_KEY` /
`_PASSWORD` still degrade silently: absent, the build simply ships no updater artifacts, and the
workflow comment says so. That is a sibling of DW-113 and arguably the same class, but DW-113 is scoped
to Apple signing, the two secrets *are* present in this repository, and widening the gate to them would
change what "a release" means for the first tag after a keypair rotation. Not fixed here, and not
quietly folded in.

**DW-114's partial resolution became a `note:`.** The entry already carried a dated `resolution:` for
the app-side half, whose last sentence ("What stays open is the watcher: … still have no production
constructor anywhere") is now false in the present tense but true as a snapshot of 2026-07-27. DW-29
already uses `note:` for exactly this — dated mid-flight progress under an entry that is not yet closed
— so the text is preserved verbatim under that key and the entry has one `resolution:`, which is what
"closed" means everywhere else in the file.

**DW-120 is the entry that changed shape while it was being written.** It was scoped as "cause never
identified", and story 34.11 identified it mid-wave: a stale `.git/refs/heads/<branch>.lock` after a
SIGKILL, plus a `gix-ref-0.66.0` loop that never advances its cursor off the root edit, so a failed lock
on a deref'd child ref spins at 100% CPU instead of erroring. Rather than duplicating that diagnosis,
the entry cites their spec and keeps open only the part their fix does not touch — the upstream loop,
whose sole *observed* trigger they removed. The ruled-out list stays in the entry even though the cause
is now known, because it is what makes a future reader stop suspecting the watcher, and a green Linux
run remains non-evidence for a macOS-only teardown.

## Verification

**What was run here.** The preflight step's script was extracted from the parsed YAML and exercised in
four states: no secrets (fails, names all seven), the two the old gate accepted (fails, names the five
missing — the case that previously entered a doomed signed build), all seven present (exits 0 with
"every Apple signing and notarization secret is present"), and one present-but-blank (fails, names only
that one). `bash -n` on the script is clean, and no secret *value* appears in any output. The whole
workflow was re-parsed with a YAML loader afterwards to confirm it is still valid and that the release
job now has no `if:` on any step except the always-run egress note, and that `syncd` still reads
`needs: release` / `if: always()`.

**Not run here, and why.** No Rust build, linter or test suite: nothing in this change is Rust, and
`cargo build` for the `keeper` crate does not work on this Linux box (tauri needs GTK). No frontend
tooling: no TypeScript, no `src/` file, and no generated IPC type is touched. A GitHub Actions workflow
cannot be executed locally at all, which is exactly why the change is a deletion plus one step whose
whole body is seven `[ -n ]` tests.

**How the owner verifies it on the next tag without burning a release.** In order of cost:

1. *Free, no tag.* On a branch, temporarily change the workflow trigger to `workflow_dispatch` (or copy
   the two gate steps into a scratch workflow) and run it. With the repository as it stands today —
   `APPLE_*` absent — it must fail at `Apple signing material must be present` within seconds, and the
   annotation must list all seven names. That is the whole behaviour under test; every step after it is
   the pre-existing signed path.
2. *One throwaway tag, nothing published.* Push `v0.6.3-check` — the tag-vs-version step rejects it
   first, which is itself the proof that the cheap gates still run in order. Then push a tag that does
   match: the run must go red at step 2 with no draft release created by the app job, and the
   `keeper-syncd` job must still attach its binaries. Delete the tag afterwards; no draft, no assets,
   nothing to clean up on the app side.
3. *The real release, once the secrets are in.* Provision all seven per docs/release.md, push the tag,
   and confirm the run is green and the draft's assets pass the checks already in
   docs/release.md → "Release-time verification": `codesign -dv --verbose=4 keeper.app` shows an
   `Authority=Developer ID Application: …` line and `flags=…(runtime)`,
   `spctl -a -t open --context context:primary-signature keeper.dmg` accepts, and
   `xcrun stapler validate keeper.app` confirms the ticket. If any of those three fail on a green run,
   the gate is insufficient and should be tightened — but they are now the only way a release can be
   both green and unsigned.

**Ledger claims, and how to check them.** DW-118: the AX measurement table is in
`epic-34-…md` and the evidence chain in `spec-34-2-…md`'s Design Notes; the policy it is blocked on is
`docs/constraints-and-limitations.md:52-59`. DW-119: `exclude.rs:104-127` carries the reasoning and
`exclude.rs:321` (`ordinary_english_build_directory_names_are_not_tier_zero`) fails if the three names
are added — `cargo test -p keeper-sync ordinary_english` is the one-line check, and it was not run here
because this change does not touch that crate. DW-120: CI run `30417789975` on commit `3523a25`,
`src-tauri/.config/nextest.toml` for the guard, `src-tauri/Cargo.lock` for the pinned `gix 0.86.0` /
`gix-ref 0.66.0`, and `spec-34-11-a-kill-during-a-ref-update-must-not-strand-a-folder.md` for the cause.
