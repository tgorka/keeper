---
title: 'A pointer is never published ahead of its object'
type: 'bugfix'
created: '2026-07-29'
status: 'review'
baseline_revision: '1be95be'
---

<intent-contract>

## Intent

**Problem:** Two independent defects, and the second is what made the first destructive.

**The credential was the wrong shape.** The LFS batch client sent
`Authorization: Bearer <PAT>`. No git forge accepts that: Forgejo reserves `Bearer` for the HS256
JWT its own server mints (`services/lfs/server.go`, `parseToken` → `handleLFSToken` →
`jwt.ParseWithClaims(…, LFS.JWTSecretBytes)`), and a personal access token is not that JWT. Probed
against a live Forgejo at `electra.siren-alsephina.ts.net`, the three shapes are distinguishable
and the reading is unambiguous:

| sent | response |
| --- | --- |
| `Bearer <token>` | `401`, `www-authenticate: Basic realm=gitea-lfs`, `{"Message":"Unauthorized"}` |
| *no `Authorization` header at all* | `401`, **byte-identical** |
| `Basic base64("<token>:")` | `401`, `text/plain`, no challenge, "Credentials are incorrect or have expired" |

The first two being identical is the finding: the server never examined the token. The third is a
different response because the credential was parsed and evaluated — with a real token it is a
`200`. Meanwhile `git push` to the same remote worked throughout, because git receives the same
secret through keeper's credential helper in the shape git wants. So the field symptom was a folder
that pushed perfectly and failed every large-file upload, which is exactly what was reported: three
`lfsUpload` units parked with `authentication rejected for electra…`, for objects of 70,843,648,
15,654,529 and 9,242,833 bytes.

The root cause is broader than one wrong string. One secret has three consumers wanting three
different wire shapes, and each consumer had hand-rolled its own.

**The push did not wait for its own uploads.** `do_push` called `commit_local` — which substitutes
pointers, stores the objects locally and queues one `lfsUpload` unit per object — and then ran
`git.push` **in the same call**, leaving the transfers for a later tick. So the remote received a
commit whose pointers name content only the committing machine has. This is the one failure nobody
observes: git accepts the push, the remote reports itself up to date, and the next peer to clone
checks out a tree of ~130-byte text stubs with no error anywhere.

`SyncError::LfsUploadPending` existed for precisely this, with a doc comment promising "the push
waits for its own uploads instead. The waiting unit is re-queued by whichever upload lands last" —
and it was **constructed nowhere in the repository**. Its three occurrences were the variant, its
`retriability()` arm and its `code()` arm. `sync_once` went further and carried a comment claiming
it drained the queue specifically so that a one-shot run could not "leave the remote holding a
pointer to content it does not have" — and drained it *after* the push leg, which satisfies the
letter of the sentence while leaving the exact window it describes.

A third, smaller defect fell out of the second: `record_failure` mapped every `Deferred`
retriability to `ProfileState::MediaAbsent`, which the UI renders as "Large files missing". A held
push would have accused an attached drive of being unplugged.

**Approach:** Make the token a type and let it own every spelling — `AccessToken::git`,
`lfs_basic`, `forge_api` — so a fourth consumer gets a method there or does not get the token.
Then construct `LfsUploadPending`: `do_push` counts outstanding `lfsUpload` units after the commit
and before anything touches the network, and refuses to publish while any remain. `release_held_push`
returns the deferred push to `pending` when the upload that just completed was the last one, and
`sync_once` drains before the push leg instead of after it. `record_failure` splits the two deferred
conditions so a held push reports `Syncing` rather than a missing volume.

## Boundaries & Constraints

**Always:** the count that gates the push includes **parked** units. Every failure path that
returns `Deferred` leaves the unit claimable only through an explicit un-defer, so something must
say the condition cleared; that something is a re-check at the top of every drain, so no single
completion has to be the one that notices. `LfsUploadPending` carries a count of outstanding units
rather than a list, because the journal rows *are* the list. The gate sits after `commit_local` —
the commit is what creates the debt — and before `open_repo`, so a held profile spends no git
invocation.

**Scope, stated as a boundary rather than left to be discovered:** the gate is a fact about
**keeper's own journal**, and the journal learns of an upload only from `commit_local`. A commit a
human makes with plain `git` — which story 34.19 newly makes work — routes through the clean filter
and stores the object, but files no `lfsUpload` unit, so `outstanding_count` is 0, the gate passes,
and the next push publishes a pointer whose bytes exist on one machine. Nothing in this story
reconciles committed pointers against the remote's object store, and nothing else does either. The
invariant this story is named after therefore holds for the commits keeper stages, not for the
folder. See the deferred-work entry filed for the reconciliation pass.

**Block If:** any `lfsUpload` unit exists for the profile, in any state. That is the gate.

**Never:** do not publish a commit while an object *keeper queued* is outstanding. Do not treat
`LfsUploadPending` as a failure: it is a wait on a condition, sibling to `MediaAbsent`, and the
profile is working rather than broken. Do not send a PAT as `Bearer` to any LFS endpoint. Do not
release a deferred unit with `undefer_profile`, which would also wake work waiting on an absent
volume and spend an attempt on it. Do not state the invariant unqualified anywhere a user reads it —
`docs/sync.md` §§3 and 8 name the human-commit gap for the same reason.

## I/O & Edge-Case Matrix

| condition | behaviour |
| --- | --- |
| no `lfsUpload` units for the profile | the push proceeds, as before |
| one or more outstanding, any state | `Err(LfsUploadPending { objects: n })` → `Deferred` → the unit is deferred at `now`, and `ProfileState::Syncing` |
| an upload completes and others remain | nothing is released; the push stays deferred |
| the **last** upload completes | the next drain's `undefer_kind(profile, "push")` returns it to `pending`, and that pass publishes |
| the process dies between the last completion and the release | the next drain re-evaluates the condition and releases the push; the folder is not stranded |
| a large file committed by **plain `git`**, not by keeper | no `lfsUpload` unit exists, so the gate passes and the pointer is published unbacked — out of scope here, and deferred |
| an upload is **parked** | still counted, so the push stays held — a stopped upload is the strongest possible "do not publish this yet" |
| a pull-only profile | `do_push` returns before the gate; the profile never commits |
| `sync_once` with uploads queued by its own commit | drains the journal, then pushes, then drains again for the units the push queues (a lane's `OpenPullRequest`) |
| a deferred push and an absent volume together | `undefer_kind` releases only `push`; volume-deferred work is left to its own condition |
| `MediaAbsent` | still maps to `ProfileState::MediaAbsent` |
| `LfsUploadPending` | maps to `ProfileState::Syncing` |
| `LfsUploadPending` reaching the daemon CLI | `EXIT_FAILURE`, because a one-shot run that ends here published nothing |

</intent-contract>

## Code Map

**`keeper-sync/src/credential.rs`** (new) — `AccessToken`, with one method per consumer and a
module header tabulating which shape each wants and why. `Debug` is hand-written to print
`redacted`. `challenge_accepts_basic` answers whether a `WWW-Authenticate` challenge is one keeper
can satisfy — `false` for `Bearer`, honestly, because that JWT is unmintable here.

**`keeper-sync/src/error.rs`** — `Forbidden { host }` separates "accepted the token and refused the
request" (403) from `Auth`'s "rejected the token" (401), because the remedies are opposites.
`LfsUploadPending { objects }` is `Deferred` and is deliberately **not** in `needs_user_action`.

**`keeper-sync/src/db.rs`** — `outstanding_count(profile, kind)` counts every row of a kind
including parked; `undefer_kind(profile, kind, now)` is the narrow sibling of `undefer_profile`, and
clears `last_error` and gives back the attempt it was charged, so a released row reads `InProgress`
rather than `Failed` with a stale "on hold" reason. `WorkKind::PUSH` and `WorkKind::LFS_UPLOAD`
expose the `kind` column spellings so a query can name one without owning a `WorkKind` value.

**`keeper-sync/src/engine.rs`** — `lfs_uploads_outstanding`, the gate in `do_push`,
`release_satisfied_waits` called at the **top of every** `drain_journal` before any unit is claimed,
the deferred split in `record_failure`, and `sync_once`'s reordered legs. The release started life as
`release_held_push`, hung off each successful completion; that made the last upload the only event
that could ever notice, so a crash or a SQLite error in that one window stranded the folder deferred
forever. Re-evaluating the condition per drain costs one indexed `COUNT(*)` and removes the window.

**`keeper-syncd/src/commands.rs`** — `sync_exit_code` gains arms for `Forbidden` and
`LfsUploadPending`. The match is exhaustive by construction, and both variants had been added to
`SyncError` without it being updated, so the crate did not compile.

**`keeper-sync/src/lfs/batch.rs`** — `auth_challenge` reads the challenge from `LFS-Authenticate`
*or* `WWW-Authenticate`, LFS-specific first. This is not cosmetic: Forgejo answers a rejected batch
with the plain `WWW-Authenticate: Basic realm="gitea-lfs"` only, so reading one header name meant
the challenge was always `None` against the very server the credential fix was written for. The 403
arm of `classify_status` also moves from `SyncError::Auth` to `SyncError::Forbidden`, which is where
that variant earns its keep — the exit-code classification was only its second consumer.

**`keeper-sync/src/lfs/basic.rs`** — the two upload/download `challenge` reads (`:443`, `:680`) go
through `auth_challenge` as well, so the transfer legs and the batch leg cannot disagree about
whether a server issued a challenge. Note the behaviour change this pair produces together: a
`Bearer`-only challenge is now *seen*, and `challenge_accepts_basic` answers `false` for it, so the
refresher returns `None` and the 401 fails immediately. Before, the challenge was `None`,
`challenge_accepts_basic(None)` is `true`, and one pointless retry was spent. Fewer round trips
against a server keeper cannot satisfy is the intended direction.

## Tasks & Acceptance

**Execution:**

1. Introduce `AccessToken` and route every consumer through it; send Basic to the LFS endpoints.
2. Add `outstanding_count` and `undefer_kind`.
3. Construct `LfsUploadPending` in `do_push`, after the commit and before the network.
4. Release the held push from `drain_journal`, re-deriving the condition on every drain.
5. Reorder `sync_once` to drain before publishing, and drain again afterwards.
6. Split the two deferred conditions in `record_failure`.
7. Repair `sync_exit_code`'s exhaustive match.

**Acceptance Criteria:**

1. The LFS batch request carries `Authorization: Basic base64("<token>:")`, never `Bearer`.
2. A commit **keeper made** carrying an over-threshold file does not reach the remote until its
   object has. A commit made by hand is out of scope — see Boundaries.
3. A profile in that state reports as syncing, not as missing a drive.
4. The held push returns to the queue on the first drain after the last upload completes, with no
   user action and no pause/resume cycle — including when nothing observed that completion.
5. A parked upload keeps the push held.
6. A one-shot `sync --once` transfers objects before publishing, and still opens a lane's pull
   request afterwards.
7. `keeper-syncd` compiles and classifies both new variants.

## Design Notes

**Why parked units count as outstanding.** The instinct is to count only live work — a parked unit
is not going to move on its own, so waiting on it looks like waiting forever. That gets the question
backwards. The gate asks "is it safe to publish a pointer whose object may not be on the remote",
and an upload that has *stopped being retried* is the strongest possible no: it is the state in
which the object is most certainly absent. Counting only live work would release the push precisely
when publishing is most destructive. The cost is that a parked upload blocks publication until a
human retries it, which is correct — the alternative is publishing a broken pointer and calling it
success — and the file's own row now carries the reason and the Retry (story 34.16).

**Why `Deferred` and not `Transient`.** A transient failure is retried on a backoff clock, and this
condition is not about time: it is about another unit in the same journal finishing. Backing off
against it would either spin or sleep arbitrarily long past the moment the condition cleared.
`Deferred` is the existing vocabulary for "waiting on a condition, not a clock", which `MediaAbsent`
already used, and it makes the wait free — no attempt is spent, and `attempts` is not inflated, so
"stopped after N attempts" keeps meaning what it says.

**Why the release lives in `drain_journal` and not in the gate.** "Outstanding" is a fact about the
whole queue, not about one unit, so no unit can decide it on its own behalf. Putting the check in the
gate would mean the held push had to be re-driven to discover it could run, which is exactly what
`Deferred` prevents; `claim_ready` selects only `pending`, so nothing would ever re-drive it. Without
a release the held push sits deferred until the profile is paused and resumed, which is to say
forever. This is the half of `LfsUploadPending`'s own doc promise that had to be built for the other
half to be safe.

**Why the release is evaluated every drain, not on each completion.** The first shape hung the check
off the completion of an upload — `release_held_push`, called after each successful unit — on the
reasoning that only the last upload to finish can know it was the last. True, and the wrong place to
put it, because it makes one event the *sole* opportunity: a process killed between that completion
and the `UPDATE`, or a SQLite error on it, leaves a `deferred` push with a satisfied condition and
nothing that will ever look again. "Forever, until a human pauses and resumes the profile" is not an
acceptable failure mode for a durability story. Re-deriving the condition at the top of every drain
costs one indexed `COUNT(*)` per profile per pass, is idempotent, and makes the release a function of
state rather than of an event — the same reasoning `recover_running` already applies to units left
`running` by a kill.

**Why `undefer_kind` and not `undefer_profile`.** The existing helper releases everything deferred
for the profile, which would also wake a unit waiting on an unmounted volume — spending an attempt
to be re-deferred immediately, and inflating the count a human later reads. Filtering on the indexed
`kind` column keeps the release as narrow as the condition that cleared. In practice the two rarely
co-occur (an upload reads its bytes from the store on that volume), but "rarely" is not a reason to
wake work that has not been asked for.

**The `MediaAbsent` mislabel, and why it was worth fixing in the same commit.** `record_failure`
matched on `retriability()` alone, so every deferred condition rendered as "Large files missing".
Shipping the gate without this would have produced a profile that said a drive was unplugged while
it was in fact uploading — a worse user-facing lie than the silent one being fixed, because it is
actionable in the wrong direction. The state word is what a user reads; the retriability is
internal.

**Why `sync_once`'s drain moved rather than being duplicated.** The original drain ran after the
push and its comment claimed it prevented this defect. Moving it before the push makes the comment
true. A second drain is still needed afterwards, for the units the push itself queues — a worktree
lane's `OpenPullRequest` — because a one-shot run may have no next tick. The pre-push drain passes
`scan_when_idle: false`: it exists to settle what the commit just created, and scanning the tree a
second time in one pass would cost a full walk for nothing.

**Why the refusal is not `needs_user_action`.** `LfsUploadPending` needs nothing from a human. It is
excluded from that set deliberately, so it produces no notification and no amber banner — the
product promise is that convergence never waits on a prompt, and this condition resolves itself.

## Verification

**Owed first.** No test drives a real Forgejo. The credential shape is evidenced by the live-server
probe tabulated in Intent — run by hand against `electra`, not in CI — and by the unit tests below
asserting the exact header bytes. A regression that reintroduced `Bearer` would pass the whole suite,
because nothing in it speaks to a server that distinguishes the two. That gap is the reason the
defect existed.

**The gate, end to end.** `engine::tests::a_pointer_is_not_published_until_its_object_is_on_the_remote`
builds a real repository with one over-threshold file, opens and elapses the settle window, claims a
push unit the way the supervisor does, and asserts:

- `do_push` returns `LfsUploadPending { objects: 1 }` — reached before any network call, so the test
  needs no remote;
- after `reschedule_after`, the profile reads `ProfileState::Syncing`, **not** `MediaAbsent`;
- one `claim_ready` offers the upload and not the deferred push;
- `clip.mp4`'s activity row names the upload and `.gitattributes` names the push, both `InProgress`,
  the latter carrying a reason containing "on hold";
- after completing the upload and draining again, `claim_ready` offers exactly the push again, and
  `clip.mp4` reads `Success` with no unit id left.

**Mutation-checked.** Replacing the gate's condition with `if false` makes that test fail. The probe
and its revert were both run; the suite is green with the gate in place.

**The durability matrix, and the false pass it had been giving.**
`keeper-syncd/tests/durability_matrix.rs::a_kill_during_a_large_object_transfer_leaves_the_object_recoverable`
asserts a 24 MiB object "must still reach the remote after a mid-transfer kill" and checked it with
`git ls-tree`. `git ls-tree` reports a *blob* for the path, and for an LFS path that blob is the
pointer — so the assertion had been satisfied, for as long as it existed, by a remote holding a
pointer to content nobody else had. With the gate in place it failed honestly, which is how the
missing filesystem transport (story 34.18) was found. It now also walks the remote's own
`lfs/objects` and requires a file of exactly `BIG` bytes. **Mutation-checked**: neutering the object
copy makes the new assertion fail while `git ls-tree` still reports the file present — reproducing
the old false pass exactly.

**Unit tests, `keeper-sync/src/credential.rs`:**
`the_lfs_header_is_basic_with_the_token_as_the_username` (asserts the literal
`Basic dGtuLTEyMzo=` and that it does not start with `Bearer`),
`every_consumer_gets_its_own_spelling_of_one_secret`, `debug_never_prints_the_token`, and
`a_basic_challenge_is_answerable_and_a_bearer_one_is_not` (using Forgejo's own challenge string
verbatim).

**Journal helpers, `keeper-sync/src/db.rs`:**
`outstanding_work_counts_the_parked_units_too` and
`one_kind_of_deferred_work_can_be_released_without_disturbing_the_rest`.

`a_push_stranded_in_deferred_is_released_by_the_next_drain` covers the window the first shape left
open: a `deferred` push with no outstanding uploads is released by a drain that observed no
completion at all. `a_released_wait_does_not_read_as_a_failure` asserts the released row reads
`InProgress` rather than `Failed` with the stale "on hold" reason.

**The challenge header, `keeper-sync/src/lfs/batch.rs`:**
`a_challenge_is_read_from_either_header_name` (`:1077`) drives `auth_challenge` through the three
cases that matter — no challenge at all, Forgejo's `WWW-Authenticate: Basic realm="gitea-lfs"` alone,
and both names present with the LFS-specific one winning. The middle case is the defect: reading only
`LFS-Authenticate` made the re-authentication path unreachable against the one server this engine
targets. `Forbidden`'s classification is covered by `classify_status`'s table test, whose 403 row is
now `(403u16, "forbidden")` (`:1012`) — flipping the arm back to `Auth` fails it.

**Not covered, explicitly:**

- Nothing exercises the gate against a real remote, so "the remote never receives an unbacked
  pointer" is verified as "the push is not attempted", not as an observation of a server's contents.
- The three parked uploads on the field host have **not** been retried. That host still runs 0.6.3,
  which does not contain this fix; the objects were confirmed present in its local store by `stat`,
  and nothing more.
- `Forbidden` is covered where it is *classified* — `classify_status`'s 403 row — which is its
  primary purpose; the exit-code arm was the secondary one. What has no test is an end-to-end 403
  from a live forge, which needs a token with read but not write scope against a real repository.
- **The invariant is narrower than the title.** Nothing exercises, or could exercise, a pointer
  committed by a human: no `lfsUpload` unit exists for such a commit, so there is nothing for a test
  to assert about the gate. The gap is real rather than untested, and it is filed as deferred work
  (a reconciliation pass over committed pointers, or a clean filter that records the debt it
  creates). Reading this story's title unqualified is the mistake the Boundaries section exists to
  prevent.
- The whole workspace, including this change, passed `cargo fmt`, `cargo clippy --workspace
  --all-targets -- -D warnings` and `cargo test --workspace` on macOS/arm64 — **as this story
  originally shipped.** The epic-34 review (2026-07-30) then moved the release to a per-drain
  re-derivation and added tests; the workspace has not been re-run green as one command since.
