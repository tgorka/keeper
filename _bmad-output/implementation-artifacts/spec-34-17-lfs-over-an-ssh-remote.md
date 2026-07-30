---
title: 'LFS over an ssh remote'
type: 'feature'
created: '2026-07-29'
status: 'review'
baseline_revision: '1be95be'
---

<intent-contract>

## Intent

**Problem:** An LFS server is always HTTP, even when git is not. `lfs::endpoint::derive` maps
`ssh://host/owner/repo.git` onto `https://host/owner/repo.git/info/lfs` (`endpoint.rs:32-41`), and
before this story that was the whole story for an ssh remote. It leaves the two halves of one sync
authenticating differently with nothing joining them: `git push` over ssh authenticates with the
user's **ssh key**, through their agent and their `~/.ssh/config`, and needs no token — §14 of
`docs/sync.md` says ssh remotes "delegate entirely to your own `ssh` binary, agent and config" —
while the LFS batch API over https authenticates with a **token**, of which keeper keeps exactly one
per profile, in the keychain.

So an ssh-remote profile is correctly configured with an empty keychain slot. Push works. Every LFS
transfer then goes out bare, the forge answers `401` with `WWW-Authenticate: Basic
realm="gitea-lfs"`, the refresher re-reads the same empty slot and has nothing new to say, and the
unit parks. That is the quiet half: by the time the transfer fails the commit is already published,
so the remote holds a pointer to content only the machine that made it can supply — the failure §8
"A pointer is never published ahead of its object" exists to prevent, arriving through the one door
that story does not watch.

The credential these servers want cannot be manufactured. `credential.rs:92-96` has said so from the
start: Forgejo's LFS `Bearer` is an HS256 JWT signed with the server's own `LFS.JWTSecretBytes`,
"obtainable only through `git-lfs-authenticate` over SSH". Probing the field forge bears it out
(figures under Verification): a PAT offered as `Bearer` is not merely rejected, it is not recognised
as a credential at all.

**Approach:** Ask the server, over the connection that already authenticates. `lfs::ssh` runs `ssh
[-o …] [-p <port>] [--] <user@host> "git-lfs-authenticate <path> <operation>"` — the wire form
git-lfs sends — and reads the JSON back into an `Answer`: a `Credential` (an `Authorization` value,
optionally an `href`, optionally an expiry) or `NoSshLfs`, meaning this remote has no such command
and the derived https endpoint still gets its turn. `Engine::lfs_access` makes that the middle of
three authorities: `.lfsconfig`, then the ssh handshake, then the derived endpoint plus the stored
token. Answers are cached per repository-and-operation with their own deadline and dropped when a
server rejects one.

## Boundaries & Constraints

**Always:** The argv is git-lfs's, element for element: the port precedes the host, the remote
command is **one** argv element, the leading `/` survives for `ssh://` and is stripped for
scp-style, and `.git` is neither added nor removed. `href` is used **verbatim** when the server
names one. The handshake is non-interactive by construction — `BatchMode=yes`,
`NumberOfPasswordPrompts=0`, `ConnectTimeout=10`, stdin `/dev/null`, `SSH_ASKPASS_REQUIRE=never`
with `SSH_ASKPASS` and `DISPLAY` removed, a 20 s ceiling over the invocation, and `kill_on_drop`.
The user's `~/.ssh/config` is still honoured, with keeper's options appended, because that
configuration is what their `git push` already works through. A stored token still applies when the
server grants a credential but sends no `Authorization` of its own **and** the endpoint is on the ssh
remote's own host — either because the server named no `href`, or because the `href` it named resolves
to that same host. That host comparison is the whole of the rule: see the `Never` below.

**Block If:** the repository's `.lfsconfig` names an LFS server (`lfs.url` or
`remote.origin.lfsurl`) — the handshake must not run at all, because that is a different authority
and not merely a different answer. The path contains whitespace or a control character: refuse
before spawning, with a `Config` error naming `.lfsconfig` as the remedy.

**Never:** Never append `/info/lfs` to an `href`. Never treat a refusal as an absent command — only
exit 127 or one of git-lfs's four not-found substrings means `NoSshLfs`. Never report an ssh
*refusal* as `SyncError::Auth`; and never report a failure to *reach* the host as `Config` either.
The two are separated by ssh's own signals: exit **255** together with one of ssh's connectivity
phrases (`connect to host`, `could not resolve host`, `connection timed out|refused|reset`,
`network is unreachable`, `connection closed by remote host`), and the 20 s ceiling, are
`SyncError::Network { host, reason }` — `Config` is `Permanent`, and parking a unit that would have
succeeded on the next attempt is how an offline laptop turns into a folder a human has to un-park by
hand. `permission denied` and `host key verification failed` stay `Config`: those do not improve by
being retried, and a permanent park with the server's own words is the correct answer. Everything
else non-zero is `Config` carrying the server's own words. Never accept a non-`http(s)` `href`.
Never read an absent expiry as "never stale". Never retry a login banner. Never write the minted
token to `sync.db`, a log line or an error message. **Never send the profile's stored token to a host
the *server* named and the profile did not** — the handshake is an instruction about where the API is,
not permission to spend a keychain secret somewhere else. Do not change `endpoint::derive`, the batch
client or the credential type: the ssh leg is another source of authority, not a change to what the
batch client does with one. In particular `parse_response` keeps returning the `href` verbatim after
a scheme check; the host comparison belongs to the caller that holds the token
(`Engine::spend`), because that is the only place that knows what the profile's own remote is.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| `ssh://` with a path | `ssh://git@host.example/owner/repo.git` | `git@host.example` + `git-lfs-authenticate /owner/repo.git upload` — leading `/` kept | — |
| An explicit port | `ssh://git@host.example:2222/owner/repo.git` | `-p 2222` emitted **before** the host argument | — |
| scp-style | `git@host.example:owner/repo.git` | operand `owner/repo.git` — slash stripped, and only here; no port is representable | — |
| No `.git` suffix | `ssh://git@host.example/owner/repo` | operand `/owner/repo`; `.git` is never added, and never removed where present | — |
| A host starting with `-` | `ssh://-oProxyCommand=x@host/r.git` | `--` emitted before the host, so it cannot be read as an option | covered: `a_host_that_looks_like_an_ssh_option_is_separated_from_the_options` asserts the argv |
| A download | any ssh remote, `upload == false` | operand ends `download`; separate cache key from `upload` | — |
| Not an ssh remote | `https://`, `http://`, `git://`, `/srv/git/r.git`, `C:/repos/t.git`, `ssh://host` with no path | `SshRemote::parse` → `None`: derived endpoint + stored token, as before | not an error |
| Whitespace in the path | `ssh://git@host.example/own er/r.git` | refused before spawning | `SyncError::Config`, naming `.lfsconfig` |
| `.lfsconfig` names a server | `lfs.url` or `remote.origin.lfsurl` set | that URL verbatim; **no ssh handshake runs** | — |
| Credential with `href` | `{"header":{"Authorization":"Bearer …"},"href":"https://h/o/r.git/info/lfs"}` | that `href` verbatim as the endpoint, that header on the batch request | — |
| Credential without `href` | `{"header":{"Authorization":"Bearer …"}}` | header used; endpoint falls back to `endpoint::derive` | — |
| `href` names the **ssh remote's own host** | `ssh://git@h/o/r.git` answered with `https://h/o/r.git/info/lfs` and no header | that `href`; the profile's stored token as Basic | — |
| `href` names a **different** host, no header | `ssh://git@git.example/o/r.git` answered with `https://lfs.cdn.example/…` | that `href` verbatim as the endpoint, and **no credential at all** — the stored token is not forwarded | the server that redirected is the one that must mint a credential; a 401 from it says so |
| `href` names a different host **with** a header | the same, plus `{"header":{"Authorization":"Bearer …"}}` | that `href` and that header — a server-minted credential is honoured wherever it points | — |
| `expires_in` present | `600` | trusted: `now + 600 s − 5 s` slack | — |
| `expires_in` absent | Forgejo/Gitea's actual response | `DEFAULT_TTL_MS` (10 min) − slack, **not** eternity | — |
| `expires_in` negative | `-5` | deadline already past: not cached, re-derived next time | the server disowned it |
| Command absent | exit **127**, or stderr matching `git-lfs-authenticate: *not found` | `Answer::NoSshLfs` → derived https endpoint + stored token | not an error; logged at debug |
| The forge refused | exit 1, `Forgejo: Unknown git command` / `LFS Server is not enabled` | `SyncError::Config` naming the remote, the exit code and the server's own words | said nothing? "It said: nothing. Check that the remote is a forge with LFS enabled" |
| The key was refused | `permission denied`, `host key verification failed` | `SyncError::Config` — `Permanent`, and correctly so: retrying does not grant access | the server's own words reach the file's row |
| ssh could not reach the host | exit **255** with `connect to host` / `could not resolve host` / `connection timed out\|refused\|reset` / `network is unreachable` / `connection closed by remote host` | `SyncError::Network { host, reason }` — `Transient`, so the unit backs off and retries | an offline laptop must not park a folder a human then has to un-park |
| A login banner | `Welcome to host\n{…}` on stdout | `Config` once, with the banner quoted — the banner **is** the fix | not retried |
| `href` not `http(s)` | `file:///etc/passwd`, `ssh://…`, a bare authority | `Config` — a non-http `href` must never become an endpoint | — |
| Server accepts then wedges | no answer within 20 s | `SyncError::Network { host, reason }` | the process is killed, not orphaned |
| Forty objects, one commit | forty journal units, same remote and operation | one handshake; the rest read the cache | — |
| The batch client gets 401/403 | a rotated key or an expired JWT | both operations' cached answers dropped; the next attempt re-derives | one wasted round trip, not a TTL of them |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/lfs/ssh.rs` -- **new, 812 lines.** The module header (`:1-74`)
  is the primary record: the hole, the five load-bearing protocol details, the expiry trap, the
  non-interactivity rules. Then the constants (`:92-111`); `Operation` (`:118`); `SshRemote`
  (`:138`) with `parse`, `cache_key` and `is_transmittable` (`:154`, `:200`, `:218`); `split_port`
  (`:227`); `Answer` (`:245`); `Credential` (`:256`) with `expires_ms` (`:536`); `authenticate` /
  `authenticate_with` (`:288`, `:300`); `is_command_absent` (`:431`); `parse_response` (`:452`);
  twelve tests (`:550-812`).
- `src-tauri/crates/keeper-sync/tests/lfs_ssh_authenticate.rs` -- **new, 207 lines**,
  `#![cfg(unix)]`: `fake_ssh` writes an executable `/bin/sh` stand-in recording one argument per
  line, and one test walks four server answers through it.
- `.../src/lfs/endpoint.rs:43-76` -- `override_url` split out of `resolve`, which becomes two lines
  over it. Existing callers are unaffected; the split exists so `lfs_access` can ask "has the
  repository already settled this?" without also getting an answer. `.../src/lfs/mod.rs:86` -- `pub
  mod ssh;`.
- `.../src/engine.rs` -- the wiring: `lfs_ssh_credentials` (`:297-312`) and `CachedSshAnswer`
  (`:315-326`); `lfs_access` (`:2521-2601`), whose doc comment is the three-authorities rule;
  `spend` (`:2603`); `cached_ssh_answer` (`:2635`); `forget_ssh_credentials` (`:2647`); the
  operation choice and call (`:2761-2768`); the two `Auth`/`Forbidden` drop sites (`:2789`,
  `:2845`).
- **Where the work landed, because a reader diffing one commit will not find it all.** The module,
  its tests, the `endpoint` split and the `mod.rs` line are commit `339acaf` (shared with story
  34.18). The `engine.rs` wiring is in `8a8aba4`, the 34.15/34.16 commit, where it arrived beside
  the credential refresher it shares a 401 path with; its doc comments name Story 34.17 throughout.
- Read and unchanged: `credential.rs:92-96`, `endpoint.rs:32-41`, `docs/sync.md` §8 and §14.

## Tasks & Acceptance

**Execution:**
- [x] `ssh.rs` -- `SshRemote::parse` and `split_port`: `ssh://` keeps the leading `/`, scp-style
  strips it, the two scp guards (a `/` before the colon, a one-character authority) are `endpoint`'s
  own, a URL naming no repository is `None`, a bracketed IPv6 literal finds only the port after `]`.
- [x] `ssh.rs` -- `authenticate_with`: options, then `-p <port>`, then `--` when the host starts
  with `-`, then the host, then the remote command as one pre-joined argument.
- [x] `ssh.rs` -- exit handling: `is_command_absent` (127, or four substrings) → `NoSshLfs`;
  anything else → `Config` quoting bounded stderr; spawn `NotFound` → `Config`; timeout → `Network`.
- [x] `ssh.rs` -- `parse_response`: exactly one JSON value, `Authorization` matched
  case-insensitively, `href` required to parse as `http(s)`, `expires_in` an integer or an error.
  `Credential::expires_ms`: the server's figure when it gives one (including a negative one),
  `DEFAULT_TTL_MS` when it does not, `EXPIRY_SLACK_MS` subtracted for every reader.
- [x] `endpoint.rs` -- `override_url` extracted; `resolve` delegates to it.
- [x] `engine.rs` -- `lfs_access` in three authorities; `spend`; a per-key cache with its own
  deadline; `forget_ssh_credentials` on `Auth`/`Forbidden`, dropping both operations.
- [x] Tests -- twelve units beside the module, one integration test that spawns a real process.

**Acceptance Criteria:**
- Given an `ssh://` remote with a port, when keeper needs an LFS credential, then the process it
  spawns receives exactly `-o BatchMode=yes -o NumberOfPasswordPrompts=0 -o ConnectTimeout=10 -p
  2222 git@forge.example` plus one further argument `git-lfs-authenticate /owner/repo.git upload`;
  and spelled scp-style, the operand is `owner/repo.git` with no leading slash and no port argument.
- Given a forge answering with `header` and `href`, then the batch request goes to that `href`
  unchanged and carries that `Authorization`.
- Given a forge answering with no expiry, then the credential is re-derived within ten minutes
  rather than trusted indefinitely.
- Given a remote with no `git-lfs-authenticate`, then LFS proceeds against the derived https
  endpoint with the stored token, and nothing is reported to the user.
- Given a forge that refuses, then the message quotes the server's own words and names the remote,
  and the remedy it implies is not "replace your token".
- Given forty large files in one commit, then one ssh connection is opened; and given a server that
  rejects a cached credential, then the next attempt asks for a new one.

## Design Notes

**`href` is used verbatim because `/info/lfs` belongs to the derivation, not to endpoints.**
`endpoint::derive` appends `.git/info/lfs` because it is *guessing* where a forge keeps its LFS API.
`git-lfs-authenticate` is that same forge answering directly, and Forgejo's `cmd/serv.go` builds
`<AppURL><owner>/<repo>.git/info/lfs` before handing the string over. Appending again yields
`…/info/lfs/info/lfs`: a 404 that reads like a routing, permissions or wrong-host problem.
`.lfsconfig` already works this way (`endpoint.rs:15-18`): anything a server or a repository *tells*
us is a root; only a URL we *derived* gets a suffix.

**keeper imposes its own TTL because a silent server is not the same as an eternal token.** The wire
format carries `expires_in`/`expires_at` and Forgejo and Gitea send **neither** — their
`LFSTokenResponse` has two fields, `header` and `href`. Their JWT expires anyway, on
`LFS_HTTP_AUTH_EXPIRY`, default 24 hours. git-lfs reads absence as "never stale", harmless for a
command-line process whose cache dies when it exits. keeper is a daemon measured in days, so
inheriting that reading means a process that works until roughly 24 hours after it last restarted
and then fails every transfer with a 401 that one restart fixes — the worst diagnostic shape there
is. Upstream's own escape hatch, `lfs.defaulttokenttl`, exists for exactly these servers. Ten
minutes, and the asymmetry picks the number: wrong-short costs one ssh round trip, wrong-long costs
a 401 part-way through a multi-gigabyte upload that does not resume.

**The refusal is `SyncError::Config` and not `Auth`, because `Auth` prescribes the wrong remedy.**
Both are Permanent, so the unit parks either way and the message reaches the file's own row; the
classification is about what it tells the user to do. `Auth` means "the token was rejected" and says
to replace it. Every refusal that can arrive here means something else: the forge does not serve LFS
at all ("Forgejo: Unknown git command"), or has it switched off ("LFS Server is not enabled"), or
this ssh key lacks write access. A new token fixes none of the three, and on an ssh remote there may
be no token at all to replace. So the error carries the server's own words. They are ugly, and they
are the only diagnostic these servers give; quoting them is searchable, where classifying them is a
dead end.

**But `Config` for *every* non-zero exit was too broad, and the cost was borne by the wrong case.**
`Config` is `Permanent`: the unit parks, and a human has to press Retry. That is right for a forge
that does not serve LFS and for a key that lacks access — neither improves on its own. It is wrong
for a laptop that was on a train. ssh reports a connection it could not make with exit **255** and
its own prose (`connect to host … port 22: Connection refused`, `Could not resolve hostname`), and
classifying that as a configuration error parks every LFS unit on the profile until somebody notices
and un-parks them by hand — turning a network blip into manual work, in a subsystem whose entire
promise is that convergence never waits on a prompt. Those exits are `SyncError::Network`, which is
`Transient` and backs off. `permission denied` and `host key verification failed` stay `Config`,
because they are exactly the cases a retry cannot help. The split is on ssh's own signals rather than
on a guess, and `error.rs`'s retriability table is untouched — this is a classification fix, not a
change to what the classifications mean.

**`BatchMode=yes` and a connect timeout are mandatory here, where git-lfs sets no ssh options at
all.** git-lfs runs in a terminal a human is watching, so a prompt is a question with an answer.
keeper is a background sync daemon: there is no terminal, the prompt is not answered, and `ssh` does
not fail — it waits. An indefinite hang is worse than an error: it holds a transfer slot, produces
no Activity row and reports nothing anywhere. Hence `BatchMode=yes`, `NumberOfPasswordPrompts=0`,
`SSH_ASKPASS_REQUIRE=never` with `SSH_ASKPASS` and `DISPLAY` removed (the GUI askpass helper is the
one prompt `BatchMode` does not close), stdin on `/dev/null`, and — because `ConnectTimeout` covers
only the connect and key exchange — a 20 s ceiling with `kill_on_drop`, so a server that accepts and
then wedges is a `Network` error rather than a permanent slot leak.

**The `git-lfs-transfer` pure-SSH probe is deliberately skipped.** git-lfs 3.x has a second ssh path
— the `ssh` transfer adapter, `git-lfs-transfer` over pktline (research §5.7) — which it probes for
before falling back to `git-lfs-authenticate`. The probe alone buys nothing: knowing a server speaks
pure SSH is useful only if you can then speak it, and speaking it means a complete second transfer
implementation with its own framing, beside the `basic` adapter keeper already has and which §5.7
calls "the only universally supported one". Skipping it also saves an ssh round trip that fails on
every remote lacking the command — which is every remote keeper has met. Nothing in this tree
establishes whether Forgejo implements it, and the decision does not depend on the answer:
`git-lfs-authenticate` returns a credential for the HTTPS batch API, the path an https remote
already takes.

**A path containing whitespace is refused up front rather than sent and mangled.** The server
receives the remote command as one string in `SSH_ORIGINAL_COMMAND` and shell-splits it; git-lfs
quotes nothing, and matching git-lfs is the entire discipline of this module. So
`git-lfs-authenticate /own er/r.git upload` arrives as four words and the server reads `er/r.git` as
the operation — such a path is not "difficult to send", it is unrepresentable. Adding quotes would
be a private dialect no forge is written to strip. The refusal happens before anything spawns, names
the path, and points at `.lfsconfig` as the way to name an LFS server directly, because that is the
actual remedy. Control characters are refused alongside it for the obvious reason.

**`authenticate_with` takes the program explicitly because the alternative is not testing the argv
at all.** The workspace forbids `unsafe_code`; `std::env::set_var` is `unsafe`; so a test cannot put
a stand-in `ssh` on `PATH`. Without the seam the argv is unobservable, and an unobserved argv in
this protocol is one nobody has checked — every mistake in it comes back as "Invalid repository
path" or a bare 401, with no indication of which half was wrong. Two of the five load-bearing
details are invisible to any assertion weaker than a real spawn: that the remote command is **one**
argv element rather than three (OpenSSH joins trailing argv with single spaces, so both forms put
identical bytes in `SSH_ORIGINAL_COMMAND`, and the difference shows only in the process's own
`argv`), and that `-p` precedes the host. So the seam is a testability decision, not a knob:
`authenticate` is the only production entry point and hard-codes `"ssh"` from `PATH`.

## Verification

**The load-bearing assertion spawns a real process.**
`the_handshake_sends_the_argv_git_lfs_sends_and_reads_every_answer`, the single test in
`keeper-sync/tests/lfs_ssh_authenticate.rs`, writes an executable `/bin/sh` stand-in that appends
one argument per line to a file, then asserts the recorded vector element for element. It rewrites
the stand-in between stages: a Forgejo-shaped grant (`header` + `href`, no expiry) with the full
argv asserted, `-p 2222` before the host and the one-element remote command included; a download,
asserting the operand changes; the same repository scp-style, asserting the leading slash is gone
and no `-p` appears; exit 127 with "command not found" answering `NoSshLfs`; exit 1 with "Forgejo:
Unknown git command" answering an error carrying both the server's sentence and the remote's name;
and a login banner ahead of the JSON answering an error naming the path. `#![cfg(unix)]`, the
stand-in being a shell script.

**Twelve unit tests beside the module**, covering what a spawn cannot reach cheaply:
`the_operand_form_matches_what_git_lfs_sends` (five URL shapes);
`a_remote_that_is_not_ssh_has_no_handshake_to_attempt` (six non-ssh spellings);
`an_ipv6_literal_keeps_its_brackets_and_finds_its_port`;
`a_path_with_whitespace_is_refused_rather_than_mangled`;
`the_cache_key_separates_upload_from_download_and_host_from_port`;
`the_forge_response_shape_round_trips`; `a_silent_server_gets_a_conservative_ttl_not_eternity`
(absent, 60 s, −5 s); `optional_fields_are_optional_and_extra_ones_are_ignored`;
`a_login_banner_is_a_configuration_error_not_a_retry` (plus empty, doubled and mistyped bodies);
`an_href_that_is_not_an_http_url_is_refused`; `only_a_missing_command_is_read_as_no_ssh_lfs`;
`captured_stderr_is_bounded`.

**Not exercised against a real forge over ssh, and that is the material gap.** The field host —
`hesperia`, syncing to a self-hosted Forgejo at `electra.siren-alsephina.ts.net` — is an **HTTPS**
remote, so nothing in this story's path has run against a live server. What the field established is
the premise, not the mechanism: probing it showed `Authorization: Bearer <PAT>` earning a `401`
byte-identical to sending no credential at all, while `Authorization: Basic base64("<token>:")`
earned a different `401` reading "Credentials are incorrect or have expired" — the second was parsed
and evaluated, the first was not recognised as a credential. That is the proof that this `Bearer`
cannot be minted from a PAT and must be asked for. It says nothing about whether the handshake
succeeds there: no ssh remote was ever pointed at it. This path is tested and unexercised in the
field.

**Also not covered, explicitly:**
- **The `--` guard was untested, and is no longer.** This document flagged it as the one detail here
  with a security consequence and the one detail without coverage —
  `ssh://-oProxyCommand=…@host/r.git` is arbitrary command execution without the separator, because
  ssh reads the host as an option and `ProxyCommand` runs a shell command. The review that produced
  this spec is what surfaced it, and it was closed rather than filed: the argv assertion now builds
  exactly that hostile remote and requires `--` to appear at a lower index than the host, and
  `a_host_that_looks_like_an_ssh_option_is_separated_from_the_options` pins that userinfo survives
  parsing so the guard's trigger condition genuinely holds for such an input. Mutation-checked —
  deleting the guard fails the argv assertion with "`--` must be emitted for a host beginning with
  `-`".
- **The engine layer is thinly tested.** `lfs_access`, `cached_ssh_answer` and
  `forget_ssh_credentials` have no tests. The three-authorities ordering, the
  one-handshake-per-forty-objects claim, the cache expiry and the drop-on-401 behaviour are reasoned
  from the code and from `Credential::expires_ms`, which *is* tested — the composition is not. Nor is
  there a `.lfsconfig`-outranks-ssh test at that level. The exception is `spend`, which now carries
  the credential decision and is covered:
  `a_server_named_endpoint_on_another_host_never_receives_the_stored_credential` asserts that an
  `href` on a foreign host with no server-minted header yields that endpoint and **no** credential,
  that the same href with a minted header yields the header, and that an href on the remote's own
  host still gets the stored token. It also pins the second half of that fix: a derive failure
  propagates instead of fabricating `https://invalid.localhost/` with the credential still attached —
  `*.localhost` resolves to loopback on every mainstream resolver, so the "unreachable" fallback was
  an authenticated request to whatever was listening on local 443.
- The 20 s timeout, the `ssh`-missing-from-`PATH` path, the `env_remove` / `SSH_ASKPASS_REQUIRE`
  environment, `kill_on_drop`, and an IPv6 host reaching the argv are unasserted; IPv6 is covered at
  `parse` level only. No test run was performed while writing this document: the test names, the
  argv, the options and the constants were read from the tree.

**Checked by reading:**
- `git-lfs` v3.7.0 `ssh/ssh.go` (`GetLFSExeAndArgs`, `GetExeAndArgs`), `lfshttp/ssh.go`,
  `lfshttp/client.go` — the argv, the pre-joined remote command, the `--` rule, and the six-attempt
  retry this module deliberately does not reproduce.
- Forgejo/Gitea `cmd/serv.go` and `models/git/lfs.go` — that it builds
  `<AppURL><owner>/<repo>.git/info/lfs` into `href` itself, that `LFSTokenResponse` carries no
  expiry, and that both forges `TrimPrefix("/")` and `TrimSuffix(".git")`, so either spelling of the
  operand works — matching git-lfs's is what means never debugging a path difference.
- `research-sync-2026-07-25.md` §5.6 (git-lfs's credential order, ssh first) and §5.7 (the adapter
  table: `basic` universally supported, `ssh` = `git-lfs-transfer` over pktline).
- `credential.rs:92-96` and its test asserting `challenge_accepts_basic(Some("Bearer
  realm=\"gitea-lfs\""))` is `false`.
- `docs/sync.md` §8 "Where the objects actually go" and §14 stated the ssh handshake correctly and
  now also state the host rule: which credential goes to a server-named endpoint was not documented
  anywhere, and `docs/egress.md` had gone further and claimed the destination "is unchanged, so the
  destination disclosed here still covers it" — false, because Forgejo and Gitea build the `href`
  from `AppURL`/`ROOT_URL` and not from `SSH_DOMAIN`. Both files are corrected in this PR;
  `docs/egress.md` is the release-diffed egress record, so a wrong claim there is the one that
  matters most.
