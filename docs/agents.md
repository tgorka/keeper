# Agents

An agent is a named actor of **keeper-hq**, the agents server the person runs — a separate, closed
repository — and keeper is only its client (D-37, AD-418). keeper ships no server address, runs no
agent and opens no listening socket. It pairs with the server you name, holds a local replica of
each database your device may open, and shows, writes and decides through those replicas
(`src-tauri/crates/keeper-core/src/hq/mod.rs:1-39`). The design is keeper-hq's
`ARCHITECTURE-KEEPER-HQ.md` (AD-417…AD-433); the decisions are D-37 and, for what it replaced,
D-31 and D-32 in `docs/decisions.md`.

**Two databases.** keeper-hq serves `tgdrive` and `neuradrive`, named after the drives whose
readers they mirror (AD-433). Each is its own syncular instance on its own Postgres schema and
role. A session lives in exactly one of them, and nothing crosses into the other: not a message,
a delegation, an invitation or an attachment. An agent that serves both keeps one memory in each.

| database | people | agents |
| --- | --- | --- |
| `tgdrive` | tgorka | Nixi (tgorka's proxy), the six Masons (Amelia, John, Mary, Murat, Sally and Winston), Argo and Skiff, Dr Tola Grey (tgdrive's steward) |
| `neuradrive` | tgorka, Marta | Nixi, the six Masons, Argo and Skiff, Dixi (Marta's proxy), Dr Lucyna Novak (neuradrive's steward) |

A device holds the databases its pairing opens (`GET /databases`, below): tgorka's devices both,
Marta's `neuradrive` alone.

**Instances.** An agent is one actor, and each place it runs is one instance with its own client
id and token: `nixi@electra` in a container beside the server, `nixi@hesperia` in Docker on a
laptop. Both share the agent's sessions and memory through the server. Which one answers is your
choice (*Which instance answers*, below). On electra the agents run in six containers, split by
trust boundary and principal: `nixi`, `mason`, `flotylla` (Argo and Skiff), `tola-grey`,
`lucyna-novak` and `dixi` (keeper-hq `ARCHITECTURE-KEEPER-HQ.md`, the containers table).

**Definitions and models.** An agent's definition — its soul, its tools, its model and its
databases — is baked into keeper-hq's image from `agents/<id>/agent.toml` in keeper-hq. No
definition is read from a drive. As of keeper-hq `main` 60b0da0, every model goes through the
operator's CLIProxyAPI router (R-HQ-72):

| agents | model |
| --- | --- |
| Nixi, Argo, the six Masons | `openrouter/z-ai/glm-5.3-flash` |
| Skiff | `openrouter/qwen/qwen3.7-flash`, with `max_tokens` 16384 so its reasoning and its answer both fit |
| Dixi | `gpt-5.6-terra` |
| Dr Tola Grey, Dr Lucyna Novak | `claude-opus-5-5` |

keeper-hq's definitions guard refuses any model outside its checked-in list of the ids the router
serves (`agents/provider-models.txt`). keeper neither chooses an agent's model nor shows it before
an answer names it (the *Verbose* view, below). Nixi is the only agent that holds the Paseo broker, and
she runs on a flash-tier model. A broker start is T3 and always waits for a person, and outside
content raises it to T4, so the person stays the check (keeper-hq `review-agent-models.md`,
rated Medium). A broker start (`create_agent` or `send_agent_prompt`) that times out may still
take effect, so check the broker before asking for it again (DW-E108-14).

On electra, on 2026-10-23, against keeper-hq 60b0da0, Nixi answered on
`openrouter/z-ai/glm-5.3-flash` (108.3b's acceptance, below). She is the one agent verified there on
these models: the other agents on the new models have not been run one by one, and that run (D8)
is still owed.

**The runtime.** keeper-hq's `hq-agent` holds the turn loop, the tool host, approvals, MCP, the
sandboxed `run`, memory and the agent's half of attachments (108.3b). keeper's `keeper-agent`
keeps only the client halves: the drive's sessions runtime, *Promote*, the surface of a note and
`zone::read_text` (`src-tauri/crates/keeper-agent/src/lib.rs:1-16`). keeper-core still carries
some `keeper_core::agents` decision modules and the tool policy in `keeper_core::bots`, because
keeper-hq imports them at a pinned revision. keeper itself runs no turn with them (AGENTS.md).

**keeper-hq's own build on electra** (108.3b acceptance 3, met on 2026-10-23 against keeper-hq
60b0da0 with the R-HQ-72 models). The first run, on fdf5255, had failed all four checks on the
agents' models, not on the build. The rerun, with Nixi in a tgdrive session, was checked by the
owner against the rows in `tgdrive`:

- **A turn:** Nixi answered.
- **A file:** a text file was read through `run` (`cat`) after a T2 approval: the person approved,
  the approval was consumed once by `nixi.electra`, and the call's row is `done`.
- **An MCP call:** `mcp__paseo__list_agents` returned; its row is `done`.
- **A broker approval:** `mcp__paseo__create_agent` went pending at T3, was approved, and was
  consumed once by `nixi.electra`. The broker did not answer within keeper-hq's 120 s deadline,
  and Nixi told the person the call was refused. The broker created the agent after keeper-hq's
  120 s deadline had passed, and keeper-hq recorded the call as refused (DW-E108-14).

Nixi's container logged no error or refusal over the run, and the device the run paired as was
revoked afterwards.

## What left keeper with the agents' move

Epic 108 took out everything that made keeper an agent host. What follows is absent from keeper
now, not switched off:

- **The agents' Matrix path** (108.2). There are no agent rooms, claims, host manifests,
  placement or doorbells, and keeper sends no `dev.keeper.agent.*` event. Matrix stays for the
  messenger and its bridges. The rooms an agent worked in before are hidden (*Old agent rooms*,
  below).
- **This Mac as a host** (108.2). keeper on a Mac hosts no agent. A laptop agent is a keeper-hq
  instance in Docker (105.3).
- **The ⌘9 bots** (108.3a), with their scheduled bot tasks. The model providers stay for notes
  search (Settings › Notes › *Model providers*; `docs/egress.md`).
- **`keeper-agentd agents init`** and its seeding of the stewards into a drive. Dr Tola Grey and
  Dr Lucyna Novak run in keeper-hq.
- **What only the drive host did**, which keeper-hq's runtime does not do today (R-HQ-71,
  DW-E108-12): the stewards' scheduled cards; memory consolidation and curation proposals; skills
  and workflow files; the BMAD runner; headless browsing. Also an agent driving the Mac's screen
  through Peekaboo, or reading the Mac's own folders (DW-E108-5). keeper-hq's delegation, memory
  rows, asks and web search are unaffected. Bringing any of the others back is new keeper-hq work,
  in the order the owner sets.

What the drives hold stays as it was, and no agent writes it any more:

- An agent's old home in a drive (`80-agents/<id>/`: `SOUL.md`, `USER.md`, `MEMORY.md`, the
  journal and proposals) was not imported (DW-E108-4). Nothing reads it.
- A session an agent worked in keeps its `agent.toml` and `log/` in the drive's sessions zone.
  The sessions board still draws its agent's key on the card (DW-E108-3), and *Promote* still
  reads its log (`src-tauri/crates/keeper-agent/src/promote.rs:30-33`; `docs/sessions.md`).
- A folder file's `agents` key is still read, so a folder file that names it is not refused, and
  is otherwise ignored (DW-E108-2).

## The agents server and pairing

Story 106.1 (AD-418, AD-422; D-4). **Settings › Agents & voice › Agents server** holds one
setting, `agents.server_url`: the base URL of the server you run. Blank means no server. Anything
else must be an `http://` or `https://` address with a host, and no user name, password, query or
fragment; a path is kept, for a server behind a reverse proxy
(`src-tauri/crates/keeper-core/src/hq/server.rs:1-50`). keeper ships no address, and the
databases are discovered from the server, never typed.

**Pairing.** A device that is already paired offers **Add a device**. It asks the server for a
one-time code (`POST /pair/start`, `hq/mod.rs:1140`) and shows it in groups of four with a QR
code. The code works once, for ten minutes
(`src/components/settings/agents-server-section.tsx:43-44`). The QR carries a
`keeper://agents/pair` link with the server's address in it, so a scanned code needs no address
typed first (`hq/pairing.rs:20`, `:108`). On the new device you type the code under *Agents
server*, or open the link and confirm **Pair this device**. Typing is forgiving: case, spaces,
dashes, `O` for `0` and `I`/`L` for `1` are all accepted (`hq/pairing.rs:14-60`). The device then
redeems the code (`POST /pair/redeem`, `hq/mod.rs:1039`) for a token of its own. That token lives
in the platform keychain, under `agents-server/<base URL>` (`hq/mod.rs:88`), and nowhere else:
never in `keeper.db`, a settings file, a view model or a log line. How a person's first device
gets its code is the server's business; keeper only redeems one.

Pairing sends the operating-system host separately from the device's display name: the short
hostname on desktop, or `UIDevice.name` on iOS (Story 109.1). An unavailable, empty or oversized
host refuses enrollment locally; keeper never substitutes a model name or random account-device
identifier. The host is read when redeeming, not when the service singleton is constructed: a
notification's background launch may precede UIKit device-name initialization. A later attempt
uses the now-known name without restarting keeper. The display name is simply `keeper`, never
parsed as host identity. The server binds the host to the new credential. T4 decisions must come
from a known host different from the claim holder's; an approval commit cannot override it.
Existing hostless credentials cannot decide T4: re-pair them, or adopt a host through the
sign-in enrollment introduced by Stories 109.5/109.6. On iOS 16+, `UIDevice.name` returns generic
`iPhone` without `com.apple.developer.device-information.user-assigned-device-name`, which the
app does not currently hold. Apple must grant that entitlement for assigned-name access. Generic
`iPhone` differs from `hesperia`, but two iPhones would share it; the actual kalypso enrollment
and T4 decision still require the physical-device check.

**Signing in** (Story 109.6; Q109-01 … Q109-06 a, S-ID-1 … S-ID-9; the server half is keeper-hq's
109.5). A device whose person is signed in to the organisation account gets its credential by
signing in, with no code (`keeper-core/src/hq/signin.rs`):

- **When.** With `agents.server_url` set and the account signed in, keeper enrols on its own after
  every start, every settings change and every sign-in (`Hq::consider_sign_in`). A device with no
  credential gets a new one. A paired device sends its own token in the body, so the server
  *adopts* it: the same client id, a new token, its host bound now (Q109-04 a). A device whose
  sign-in lapsed or was suspended (S-ID-9: the server refuses its token) signs in again the same
  way, silently, keeping its client id. An outcome is remembered per account *and* per sign-in of
  it: keeper's own mark, minted by every interactive sign-in and kept by every refresh
  (`StoredSession::sign_in`), never the provider's `auth_time` (a refresh may drop it, and a
  sign-in completed from the provider's own session may repeat it). A sign-in that could not be
  refreshed is tried again as soon as the person signs in anew, and a device the server no longer
  knows (`hq.device_unknown`) is never replaced by a new client id on its own. An account that
  moves under three attempts in a row before any could send waits for the next change of the
  account or the setting, so it is never a loop against the provider and the server.
- **One attempt, one account, one server.** An attempt holds the account it checked — its id,
  issuer, client, scopes and sign-in — and the token is minted from that very account
  (`OrgAccount::fresh_token` refuses another one, `TokenError::Changed`). Before the request is
  sent the attempt checks, under the service's lock, that its configuration is still the current
  one and the account still the one it checked; otherwise it sends nothing and the account as it
  is now is considered afresh. A server change, a cleared address or another account cancels the
  running attempt. One whose request is already sent is not cancelled: it keeps what the server
  answers under that server's keychain item, because the server may already have issued the
  device or rotated the old token, and restarts onto it only if that server is still set. It
  replaces only what it started from — no credential, or the token it adopted: if the item moved
  on meanwhile (a Forget, or a newer attempt that kept its own), the late answer is discarded and
  its device revoked with its own token (`POST /clients/{id}/revoke`), so no credential is left
  that nothing holds.
- **To whom.** The account's access token leaves the device only after the server's
  unauthenticated `GET /auth/config` names the account's own issuer and client, exactly, and no
  scope the account's sign-in does not ask for (S-ID-2, `signin::trusts`). It goes only to
  `POST /devices/enroll` and `POST /clients/self/attest`, as a bearer, never in a URL (S-ID-1).
  Each presentation mints a new token (`oidc::minted_access_token`): the server spends its `jti`
  and refuses one issued more than five minutes before. Neither token is ever logged.
- **What is kept.** The device token goes to the keychain item a pairing's lives in
  (`agents-server/<base URL>`), replacing it, with when it was last attested.
- **Attestation** (Q109-03 a). A credential that came from a sign-in is re-attested six days into
  the server's seven-day window (`signin::Bound::due_at_ms`), with a freshly minted token. A
  signed-out account does not attest; the credential lapses, and the next sign-in adopts it back.
  Attesting stops only on an answer about the device itself: the server no longer knows it (401
  `hq.device_unknown`: discovery then marks it removed), it never came from a sign-in (409
  `hq.attest.not_enrolled`: an enrolment adopts it), or it is another person's (403
  `hq.attest.other_person`). Anything else — a refused sign-in token (`hq.signin.stale`,
  `.replayed`, …), a person, app or role an administrator can restore (`hq.not_a_person`,
  `hq.signin.client`, `hq.signin.role`), a 403 that is not keeper-hq's (a proxy's page), an
  unreachable server — is tried again after ten minutes, doubling at each one in a row up to six
  hours, and back to ten minutes after a success; a 401 also asks the server again whether the
  device is known. Every wait is kept on the wall clock and slept at most an hour at a time, so a
  computer that slept through it attests at its next wake. Settings says why under the
  signed-in line until an attestation goes through.
- **Settings.** For a person keeper can sign in for, *Agents server* shows no code field and no
  **Add a device**: it says how this device stands with signing in, and lists **New devices** —
  the server's notices of the person's other devices signing in, being adopted or pairing
  (`GET /clients/self/enrolments`, S-ID-8), this device's own left out.
- **Pairing stays** for anyone keeper cannot sign in for: no account, a server without sign-in
  (`/auth/config` answers 404), or a refusal (another issuer or client, a person the server does
  not list). 109.7 retires it.

**Discovery.** At every start, and after pairing, `GET /databases` says whom the token was issued
to and which databases it opens (`hq/routes.rs:289-295`). The device holds one replica per server,
database and person under the data directory, at `hq/servers/<server>/<database>/<actor>.sqlite`,
each with its own socket at `<base>/<database>/…`. Writes made while no server is paired go to
`hq/local/local.sqlite`. The whole `hq/` tree is kept out of device backup
(`hq/databases.rs:1-33`). A replica links only once this run's discovery says its database is
open; a refused one is retried on a growing backoff (`hq/mod.rs:15-23`).

**Devices.** The *Devices* list shows the person's devices from every replica's `clients` rows.
**Remove** revokes one on every database (`POST /clients/{id}/revoke`, `hq/mod.rs:1174`). A
device the server removed stays removed across restarts until it is paired again, and its copies
stay readable until the person removes them. **Remove this server's copies** deletes that server's replicas, then
its pairing from the keychain; what waits in the local replica stays (`hq/mod.rs:1201`). A copy
that cannot be deleted is named, and the pairing is kept with it.

With no address set, or cleared after pairing, keeper contacts no agents server: see the next
section. What keeper sends the server, and when, is the *Agents server* row of `docs/egress.md`.

## The Agents window on this device

The Agents window reads Rust's local database sessions, not Matrix rooms. Sessions from every
database this device holds share one list. Each row and the opened session's header name the
database; identical session ids in two databases remain separate conversations. Rows show the
agent, last activity and Rust's attention state. **Active**, **Archived** and **All** change only
which rows are shown, never their order or the underlying subscription.

Rust derives each name as `<agents> · <title>`, with the primary agent first and other agent
members in actor-id order. Person members do not enter the name. The word handle comes from
the immutable session id and the shared Syncular vocabulary; a collision within a database
appends the session number. It is a display label, not another identity or storage key. A blank
title uses the handle. Renaming changes only the title, never the session id or handle.

Right-click a row, use Shift+F10 on a focused row, or long-press it on a touch screen to open its
menu: **Rename**, **Copy id**, **Copy name**, **Details**, **Drives…** (only where you may
choose the session's drives; it opens the session and its Drives menu, see § Live answers),
**Model…** (only where you may change the session's model and its agent offers more than one,
`AgentSessionRowVm.choosesModel`; it opens the session and the composer's model picker, the same
picker `/model` writes through) and **Archive** (or **Unarchive**).
Rename edits inline with the current title selected, so typing replaces it; Save queues the title
on this device, including offline, and Cancel/Escape leaves it unchanged. Closing the editor means
local enqueue, not server acceptance: the row says it is waiting until the server settles the
write. The state comes from the replica's outbox and
outcome journal (`Replica::row_writes`, restricted to writes of `title`), the same per-row
delivery the instance choice reads, so it survives a restart. A later refusal stays visible
beside the row and in its menu in plain words (`hq.v3.not_member`: "you are no longer a member of
this chat"; a conflict: "another device changed this chat first"; any other code is quoted).
**Retry rename** reopens the refused proposed title; retrying supersedes the old refusal, and
acceptance clears the waiting state. Only the newest title write counts: a rename — from the menu
or `/rename`, which take the same path — sets aside every earlier title write of the row,
including one still waiting, so if an older rename is refused after a newer one was written, that
refusal never stands — the row follows the newer one, and Retry never offers the older title. The
set-aside is made in the same step of the replica's owner that writes the new title, before any
sync round can settle the older one. The set-aside of a waiting write is kept beside the replica
(`<replica>.blob-access.sqlite`) until its outcome arrives. If that record cannot be written, the
rename still reports success (the title was queued), the failure is logged, and the set-aside still
holds in memory until keeper restarts. The chat's own creation is not a rename and is never set
aside by one: while it waits, a rename queued behind it shows no rename state, and neither does its
own refusal. An unrelated archive write is not a pending
rename. If delivery cannot be read, the row says it is unavailable rather than claiming
confirmation. Clipboard refusals also stay visible beside the row. Archive/Unarchive still
reports only the local write; its later refusal is not surfaced on the row yet.
Development fixture: `?sessions=names&rename=refused` — the editor closes on the local save, the
row waits, the server refuses the first rename 1.2 s later, and a retry is accepted 1.2 s after
its save.
Details includes the database, immutable id, creation time, reported trust and known instances;
a known host is labelled `<agent>@<host>`.
Pin and Mark unread are absent: this surface has no operation for them, and whether to build them
waits for the owner's decision (A109-05).

Opening a session shows its newest visible page. **Earlier messages**, or scrolling to the top,
asks Rust to widen the history window. **Archive session** keeps the list row and marks the
session archived; **Unarchive session** continues it. An archived read-only page has no composer.
The frontend does not download all history to assemble a conversation.

**New session** offers the database directory's agents. An agent serving one database names that
database without a picker; an agent serving several asks where the session belongs. Sending the
first message creates the session and message together. Message captions — **waiting**, **sent**,
**taken** — come from Rust, not delivery/read receipts. The shared composer retains a refused
send; database sessions never read or write Matrix drafts. An unsent draft lives only in the open
composer and is not restored after leaving that session. New session is the exception: opening a
session from anywhere — a row, a tapped notification — ends New session (one store transition,
`agentSessionsStore.select`), and what was written there, with the agent and database chosen, is
kept in the webview's store until New session opens again, where it is back in the composer.
**Cancel** (or Escape on a phone) and a created session drop it; a relaunch does too.

**Slash commands.** In an open session, type `/` to see the commands Rust offers, type more to
filter, and press Enter to run the selected command. Unknown or unavailable commands are refused,
never sent as prose. Queued attachments do not disable commands: `/stop` runs without uploading
the files and leaves them queued; a refusal keeps both the files and the draft. A command that
leaves the session discards its unsent tray, just like navigating away. To send a literal leading
slash as an attachment caption, double it: `//stop` sends the caption `/stop`, not a command.
The same `//` escape applies without attachments. `/clean` and `/new [title]` open a fresh,
empty session with the same agent and database, keeping the old conversation in the list.
`/clean` also ticks the old session's drives that you may still tick (Q109-17 b, "same agent,
database and drives"): a drive the agent no longer offers, or one the audience rule now refuses,
stays behind, and the new session's drive control shows what came across. `/new` ticks none
(Q109-20 b). They neither clear history nor create an empty prompt. `/rename <title>` changes its
title; `/archive` keeps its history and makes it read-only. `/people` opens People, `/tools` opens
the existing tool switches when that agent offers tools, and `/help` opens the command list.
`/context` opens the same context **Details** panel as the ring beside Send (below).
Each of `/model`, `/effort` and `/drives` is offered when that choice exists for you here: `/model`
when the agent offers models, `/effort` when it offers efforts (it lists only those), and `/drives`
when you have the session's drive control (below). `/stop`
writes a person control row naming each unfinished turn; a compatible runtime cancels only the
named running turn and keeps the output already received. With no unfinished turn it refuses.

**With no server address set** (`agents.server_url` blank, or cleared after pairing) keeper
contacts no agents server. A device that was paired keeps the databases of the server it last
synced with (`hq/last-server.json`, its base URL and nothing else; the pairing stays in the
keychain) listed and written to: New session offers their directories' agents, and a session or a
message written there waits in that database's own outbox. Settings › Agents says *No server is
set* on each such database with its count, and on the server line with their total. Nothing
links, no discovery runs and no push target is sent; setting the address again links them, and
*Forget* forgets that server. A device never paired holds no directory, so New session has no
agent to offer and says so; nothing is written for it.

Approval cards show the exact action and its binding digest. A decision is addressed to its
database and session and bound to that digest; a server refusal stays beside the action. A queued
decision is not final: if Rust reports it refused and decidable again, the same card permits a
new decision. The
**Waiting for you** group collects pending decisions across databases. A question in a message
offers its options, or the normal composer for a free-text answer; Rust closes the question after
the person's reply. On phones the list and conversation are separate screens. Opening a session
focuses **Back to sessions**, not the composer, so it does not summon the software keyboard.
**Back to sessions** or **Escape** returns focus to the originating database-qualified row;
an open selector consumes Escape before the screen does. The Notes dock and tablet assistant
sheet use the same list mirror with independent selections, and launch opens Rust's local
default session.

**Always allow (109.11, Q109-09 b, Q109-10 a, Q109-12 a).** A card's actions are Rust's
(`AgentApprovalVm.actions`): *Allow once* and *Deny* as buttons, and every action in its `…`
menu — *Always allow …* and *This session only* only where `hq::grants::offer` offers a grant.
It offers one for a T2 or T3 request of a tool on the server's allow-list (`run`,
`mcp__paseo__create_agent`, `mcp__paseo__send_agent_prompt`) in an open session that has not
read outside content, and only to a person the session's agent serves in that database — keeper
reads that from the `person_agent` keys the server granted its grants window
(`Replica::granted`), as V15 decides it (`hq.v15.agent`). syncular-client never pulls a unit
the server revoked again, so keeper asks again for revoked grants units itself
(`grants::reask_revoked`: out of the window and back in): once after the replicas start, at most
hourly after, and when the person opens a session or Settings › Always allowed (at most once a
minute). An agent that starts serving the person then gets its offer, and the grants made about
it on another device list here; never at T4, in an untrusted session,
for `delegate`, a drive tool, any other tool or an invited member the agent does not serve, where
the menu leaves it out rather than disabling it. The confirm says how broad the
grant is in Rust's words: a `run` grant covers every call whose program (and first argument, if
it names one) matches, whatever follows; a Paseo grant covers every call of that exact tool,
whatever its arguments. The sentence adds that this is any code for an interpreter named alone,
with any option first (`bash -lc`, `sh -ec`, `python3 -c`, `node -e` …) or an eval subcommand
(`deno eval`), for a program whose first argument is program text (`awk`), and for a wrapper
that runs whatever program follows it (`env`, `sudo`, `ssh`, `nice`, `timeout`, `xargs` …). The
confirm returns focus to the card's `…` menu when it closes; a decision taken in the banner moves
focus to the card's new place in the reading column, else to the next request in the banner,
else to the composer. The scope is derived again in Rust from the request's own call; the
grant row (`standing_grants`, keyed by `vocabulary::person_agent_key`, the server's
`personAgentKey`) and the approval go in one commit, so a refused grant leaves the request
waiting with the server's words. Settings › Agents › *Always allowed* lists every grant the
person made — agent, scope, database, its session if bound, expiry, state — with *Revoke*.
Revoking stops new uses, not approvals already issued, and the section says so. A grant bound to
a session reads *Ended* once its session's row here is archived, and stays ended if the session is
reopened; a session merely absent from the device (left, removed, not arrived yet) leaves the
grant as it was, with Revoke. A grant reads *Expired* when its expiry passes: the views wake at
the soonest one (`grants::next_expiry`). A grant made on a device that has since been signed out
(its `clients` row, joined on `granted_by_client`, has `revoked_at_ms`) reads *Inactive*: "made on
“<device>”, a device that was signed out, so it decides nothing" — the server refuses every
approval it would make (`hq.v8.grant_credential`) — and keeps *Revoke* so the person can clear it
(RG-08). A refused grant or revocation is read back from the replica's per-row outcome record.

The requests waiting for this person sit in a banner above the composer (`AgentSessionViewVm
.banner`), out of the reading column. **Sending a message declines them** — each its own denial,
written before the message (`approvals::decline_pending`) — and the banner says so, including
that a message saying yes still declines. The webview sends back the `(id, digest)` pairs its
banner showed, and Rust declines only those still waiting at that digest: a request that arrived
after the banner was drawn stays pending. A T4 request this device cannot decide — it is on the
host the agent runs on, or either host is unknown, which V8 refuses for a denial too
(`hq.v8.t4_other_host`) — is not declined and offers no decision: its card says "Needs a decision
from another device." with no *Allow once* or *Deny*, it stays in the reading column (and out of
the decisions panel), and the banner adds "1 request needs a decision from another device; a
message here leaves it waiting" (DW-E109-27, R109T-02). When the host of the instance holding the
session is the unknown one, no device can decide it yet, and both say so instead: "Can't be decided
until the agent's host is known." (R109T-04; `approvals::Undecidable`). A text message, a message with files, an ask option and
`/stop` decline; no other slash command does, nor a spoken question (it is sent with no banner
shown). A denial that cannot be written on the device stops the message, and the banner says
which request still waits; the composer keeps the draft and shows the same sentence hidden from
assistive technology, so it is announced once. Mock shell: `?sessions=launch` draws a trusted T2
`run` (grants offered) and a T4 Paseo start (none); `&grants=untrusted` makes that session
untrusted; `&grants=refused` has the server refuse every grant write; `&grants=unserved` makes
the session's agent one that does not serve the person; `&grants=decline-fails` fails every denial
write; `window.__keeperMockLateApproval()` lands a request without drawing it. The grants list
holds an ended grant, an inactive one made on a signed-out device, and one that expires 20 s
after the page opens.

**Always allow: known limits.**

- The "any code" line does not yet know Windows forms (`powershell.exe -c`, `python.exe -c`,
  `cmd /c`), package runners (`npx`, `bunx`, `uvx`, `uv run`, `pipx run`) or programs that can
  exec (`find -exec`, GNU `sed`'s `e`); for those the breadth sentence ("whatever arguments
  follow") stays literally true (R10911-17).
- Revoked grants units are asked for again at most hourly, inside a window pass (a change or an
  expiry wakes one), so an idle device can wait longer; and if the second window write of a
  re-ask fails, the unit stays out of the window until the units change or keeper restarts — no
  offer and no rows meanwhile (R10911-18).
- A grant made on a device that is suspended or whose attestation lapsed reads *Active*, though
  the server refuses it as it does one from a signed-out device. The directory keeper syncs carries
  only `clients.revoked_at_ms`; showing those states (and that such a grant decides again once the
  device signs in) waits for keeper-hq to sync them, ideally as a server-computed flag (RG-08).

Conversations agents had before they moved to the database are archived sessions of their
database (*Imported history*, below). Instance selection is **Answers from** (below); message and
attention state remain Rust-owned throughout.

Browser checks use the development mock shell (`?sessions=launch`, optionally
`&platform=phone`), not a server or a performance measurement. `?hq=cleared` is a device whose
address was cleared after pairing (both databases held, writes counted per database);
`window.__keeperMockPushTap({database, sessionId}, arriveAfterMs?)` is a tapped notification, and
with `arriveAfterMs` a session that reaches the list only then. The installed macOS shell and the
archived-session ≤1 s tailnet target still require the native/device gates; mock page timing is
not evidence for that target.

## Live answers, the turn fold and view modes

**The answer last (109.14, Q109-13 a).** The window draws Rust's reading column (`items`) in its
order and nothing else: a person's message, each turn's **fold**, the turn's answer below it, an
agent's question where it was asked, an answer still being written last. A message the column does
not name is not drawn — a lead-in ("Checking — one minute.") lives inside its turn's fold.

- **While a turn runs** its fold is one live line, Rust's sentence: "Searching the web… · 3 steps".
- **When it ends** everything but the answer — lead-ins, thinking, tool calls — is one line above
  the answer: "Worked for 14 s · ran 1 command, searched the web 3 times", or the status a turn
  ended in ("Interrupted · …", "Stopped · …", "Refused · …", "Failed · …").
- **Refusals and failures** stay one plain line each under the fold line, folded or not: never a
  card, never behind a disclosure.
- **Opened**, a fold shows its lead-ins and its tool calls in a scroll area of at most
  `min(24rem, 40vh)`; each call is its own disclosure with its status in Rust's words
  (`statusLabel`: Started, Finished, Refused, Interrupted, Paused, Failed, or
  `Unknown status "<word>"`), how it ended ("Interrupted 12:03", never "Finished" for every
  ending) and its result in a `max-h-64` scroll area. The renderer groups, orders and words nothing.

An open answer grows as its text arrives. The transcript scrolls inside its own viewport: a growing
answer does not move the composer or replace an unsent draft. Scrolling back stops following the
newest text until you return to the bottom. If the claim ends without a final message, the retained
text says **The answer stopped.**

**View modes** — **Normal**, **Thinking** and **Verbose**, Claude desktop's transcript modes —
replace *Show thinking* and *Show details*. Normal is the default.

| Mode | Folds | Thinking | Answer details |
|---|---|---|---|
| Normal | closed | not sent | not sent |
| Thinking | closed | sent, shown under the fold line | not sent |
| Verbose | open | sent | sent: model, input/output/total tokens, time to first token, duration, finish, error |

The mode is your preference on every device (`agents.view_mode`, user-global, settable in a
settings file). The control shows it as it holds now, carried with the open session's view: a
change synced from another device or a settings file reaches open sessions too. A settings file
that sets it keeps it — the control stays as the file says and **Saved, but a settings file keeps
this preference as it is.** appears beside it. An older device's or file's `agents.show_reasoning`
and `agents.show_details` read as one mode: details on is `verbose`; otherwise thinking on is
`thinking`; otherwise `normal` (in `keeper.db`, in every `keeper.toml` layer, and in the synced
settings file and its base, which is rewritten once under the new key).

- **A `keeper.toml` that pinned one old switch now pins the whole mode.** A layer holding only
  `"agents.show_details" = false` pins `agents.view_mode = "normal"`, and one holding only
  `"agents.show_reasoning" = true` pins `"thinking"`: the switch the file left open is no longer
  the person's to choose, and the View control stays as the file says. Whoever owns such a file
  should rewrite it as the one `agents.view_mode` line it means (R10914-02).
- **Devices on different versions, while any of them is older than 109.14.** The first newer
  device to sync rewrites the synced settings file under `agents.view_mode`; an older device then
  finds its two switches gone from the file and turns both off, and keeps the newer key without
  reading it. A switch turned on afterwards on the older device is dropped again by the next newer
  device to sync, so it never reaches the newer devices. Update every device to read the mode
  everywhere (R10914-03).

Rust decides what each mode sends, and a mode never widens who reads what. Thinking reaches the view
only in the Thinking and Verbose modes and only for a person who can read the session's trace; it
remains readable after the answer finishes. A person without trace access receives no reasoning or
tool-result rows and their folds carry no steps — their fold line counts no calls ("Worked for
14 s"), and only a close's own error stays a failure line; the current tool's name is public to the
session ("Searching the web…"), not its arguments or result. These are server/Rust boundaries, not
CSS hiding. Missing measurements have no label and are never filled with zero; a reported zero
remains a real measurement. Answer markdown renders inert text: model-written HTML does not become
HTML, and model-written links do not navigate.

**Context window** beside Send opens a side **Details** panel; `/context` opens the same panel. It
shows the latest answer's last-round occupancy (`metadata.context.used`), not the accumulated
billing usage in `metadata.usage`. Like the rest of an answer's details it follows the **Verbose**
view mode, for any member, trace reader or not: `metadata.context` is in every member's replica,
so the setting decides only what is drawn. The labelled ring and its tooltip show used/window and
the rounded percentage; an unknown model window shows **used ≈ N tokens**, with no window
percentage or ring. Tab from the message field to the context button, then Enter to open Details;
Escape closes it and returns focus to whatever opened it. Its seven expandable parts are system
and persona, memory, built-in tools, MCP tools, history, attachments and this turn's tool results;
parts a newer runtime adds that this build does not name are summed into one **Other** row, so
they never hide the meter. Counts and shares are estimates, marked **≈**; the total is always
`used`. Missing or old metadata omits the ring rather than inventing zero, and `/context` then
says the latest answer has no measurement (or that the view mode is not Verbose). Malformed counts, a
missing or repeated known part, or parts above the used count are refused, with one tracing
warning per answer. A turn stopped or refused before its first measured round has no context, and
the meter does not fall back to an older answer. Over-window occupancy keeps its real percentage
label while the ring and accessible meter stop at 100%.

For the context viewing aid, `?sessions=launch` opens alpha with 48000/200000 tokens (24%); beta,
**Review the research notes**, has the same used count and an unknown window. Their source answer
metadata deliberately carries different cumulative usage (125000 input tokens). These are mock
projections, not evidence of a running model or the packaged app. The mock ships in the Normal view
mode, so choose Verbose to see the ring.

**Tools** contains only tools the answering instance offers, from its definition and container.
Switching off narrows that set; switching on restores an offered tool and never adds one. An
unknown or absent offered-tools list renders no Tools control, and neither does a session that
cannot be changed here (archived). Changes wait for Rust's view; a
refusal is visible beside the controls. **Untrusted** marks a session that has read outside content;
consequential actions still need a person.

**Drives** (109.23; Q109-20 b, V17) sits under the session's title: a chip per ticked drive and a
menu of check items. No drive is ticked until a person ticks one — not even the session's own
database. The menu lists every drive the agent offers (`agent_profiles.drives`, any instance of it,
as the server reads the ceiling), then the drives keeper knows of that it may not read here, each
with Rust's reason: another agent's drive ("Nixie does not offer omega."), or one the audience rule
refuses — drive D in database S only when everyone who reads S reads D (D-34), read from the
directories of the databases this device holds ("Ben reads alpha but cannot read beta"). A drive
whose readers this device cannot see (the person is not in that database) is offered and the server
decides. keeper reads readers from the `actors` rows the server's directory sync writes, and that
sync only ever adds rows: a person the registry no longer lists in a database still counts as its
reader here (R10923-02). keeper may then show a drive as not allowed that the server would accept;
the other direction is benign, since the server refuses and the refusal is shown. Only the server
can close this, by removing departed people from `actors`. Ticking writes the session's
`session_drives` row; unticking the last drive writes `[]`, never a delete, because syncular refuses
any later write to a deleted id. A tick waits for the server (**Waiting for the server…**). V17
checks every drive of the selection and names none, so a ticked drive that stopped being allowed
(its agent no longer offers it, or the audience rule now refuses it) would make every other change a
refused write. Such drives go together: unticking any of them — in the menu or with its chip's
button — unticks all of them in one write, which V17 takes, and the menu and the line under the
chips say so before the person acts (**Untick delta and gamma to change the other drives: Nixie no
longer offers them. Unticking either unticks both.**). Every other change is refused here, naming
them all (**Untick delta and gamma first: Nixie no longer offers them.**). Unticking one alone would
leave a selection V17 still refuses for the other, so nothing could ever change (R10923-06). A
ticked drive whose readers this device cannot see counts as failing too once a standing server
refusal names it, until a newer selection sets that refusal aside: beside a drive keeper knows
fails, each would otherwise hold the other's untick back (R10923-08). A server refusal
(`hq.v17.audience`, `hq.v17.not_offered`, …) is hung on the drives of the refused selection keeper
knows fail that way (for the audience rule, failing those, the ones whose readers it cannot see),
never on the drive that was clicked; it reaches the drive's row and the line under the chips —
inside the menu while it is open, where a screen reader still hears it — read from the replica's
per-row outcome record, until the next selection sets it aside. The menu always says, and the header
repeats once a tick takes the selection from none to one, what D-34's accepted risk allows
(R-HQ-79): once a drive is ticked the agent may read it and send what it reads to the web without
asking, even after the chat has read outside content. Only a person member (`owner` or `person`) of
a session that is not archived gets the control, `/drives` and **Drives…** in the Agents list's
right-click menu, which opens the session and its menu; anyone else sees none of them, and an agent
that has not said what it reads gets no control. Fixtures: the default mock offers `alpha` and
`gamma`, refuses `beta` by the audience rule and `omega` as another agent's, and its fake server
refuses a tick of `gamma` a moment later; `?drives=nonmember` draws no control; `?drives=stale`
ticks `alpha` and a `delta` and `gamma` the agent no longer offers; `/drives` opens the menu.

The viewer rule is Rust's: “A viewer renders a turn that is still being written as the turn's
`turn_chunks` contiguous from seq 0, followed by the latest `text` it has seen on
`session:{session_id}` whose `from_seq` is the seq after the last of them; it keeps the latest tail
for each `from_seq` — through a presence leave — until that seq's chunk arrives, and replaces both
with the agent's `messages` row for the turn (`turn_id`) once it holds it.” The frontend renders
that projection, never folds chunks or presence itself.

The session VM's `items` and `folds` are what the window draws (109.13, drawn since 109.14):
`messages` holds the bodies the column names, and `live` the text of answers still being written;
neither carries a turn's thinking or tool calls any more — those are the fold's alone.
Each turn has one fold containing its earlier agent-message IDs, permitted reasoning and
tool steps, a summary, reported duration, status label and single-line failures. A persisted
terminal answer is identified by the runtime Closed row's `metadata.finish` key, including
an explicitly null finish reason; a committed lead-in does not end the live answer. An agent's
ask (`metadata.kind = "ask"`) is a question to the person: always a reading item where it was
asked, never a lead-in, an answer or a fold. The final answer is a separate reading item
immediately after its fold, at the answer's own position after any intervening person
messages. A streaming answer is read where its Closed row will land — after everything already
written — so a person message sent mid-stream sits above it before and after it finishes. A
second finished row of one turn (a kill-cut row, then a late close) stays a reading item at its
own position; the turn keeps one fold, its last close decides the status, and every close's
error stays a failure line. Historical tool-only folds stay at their earliest recorded work
time, before later messages and answers, without fabricating a live answer. Tool results
arriving after the answer update that same fold, never add work below it.
The status reads each runtime close explicitly: `cancelled` (a person's Stop) says
**Stopped**, as does a person's stop row naming a turn that has not closed yet; `cut` (the
instance restarted mid-turn) and interrupted work say **Interrupted**; `failed` (a broken
stream or a provider error) says **Failed**, as does work-only history whose tool call failed
(the importer's `failed` step); only `content_filter`, or tool-only work that ended refused,
says **Refused**. A tool step whose status word keeper does not know is never running work: the
turn reads by its close, and the step keeps one failure line naming the word. A closed turn
reads by its close even while a call's row still says `started` (109.14).
`stop`, `length`, `tool_calls`, a null reason or a provider's
own word is an ordinary close. Parked work says **Paused** unless an unexpired pending
approval actually asks the person, in which case it says **Waiting for you**. Trace access and
the view mode still gate the corresponding fold contents; the mode does not gate the reported
duration used in a summary. Missing or negative durations are absent; a reported zero
is retained. Grouping, ordering and all these sentences belong to Rust, not the renderer.

Development fixtures: `?sessions=launch&live=stream`, with `&platform=phone` for the phone shell.
`live=error`, `stopped`, `nonreader`, `no-tools`, `untrusted`, `long`, `refused` and `pinned` cover
provider error, ended claim, trace exclusion, absent offers, outside-content marker, long strings,
refused preference/tool writes and a settings file keeping the view mode at Normal.
`&turns=all` adds to alpha's `device.1` one turn of every fold state: tool-only work cut by a
restart, a person's stop, an imported failed call, and the screenshot's turn (a lead-in, a run,
three searches with one refused, then the answer). Timed mock deltas are UI evidence only, not a
server or latency proof.

### Measured: live-answer presentation

2026-10-20, real app in the agent-desktop Chrome with the development mock shell, at 1280 × 900
and 420 × 900. During an 18-second timed answer, 171 desktop and 175 phone geometry samples
kept the composer's top at exactly 850.5 px and 844 px respectively (0 px movement); its draft
and textarea node were retained. The long-string phone case also held 844 px across 175 samples.
There was no document-width overflow. Phone controls — thinking/details, Tools, offered switches,
tool disclosure, code-copy, Back, Archive and Send — measured at least 44 × 44 px. Both preferences
were exercised on/off and survived mock-shell reloads; offered tools were switched off and back on.

ARIA snapshots and screenshots covered live text, tool start/completion, thinking on/off, final
details, provider error, stopped answer, nonreader, absent tools, refused writes and long output.
The stopped notice sits after the retained answer so it remains visible at the reading edge,
rather than disappearing above a long reply. These successful runs recorded no browser console
errors or failed network requests. The mock's persisted preferences are not proof of native
settings storage; Rust and the native shell gates own that boundary. Layout geometry is browser
evidence, not a claim that jsdom measures layout.

The native tailnet p95 publish-to-receive target (NFR-124) requires the real agent and viewer
timestamps. Browser mock timing does not measure it; no native latency result is claimed here.

### Measured: a live answer against the real server

2026-10-21, `hq_live.rs`'s three live-answer tests on one Linux host: the keeper-hq server
(`hq/104-5-claims` 2dc38f4, Bun 1.4.2, serving this branch's contract, schema 3) over a throwaway
Postgres 18 on delectra, one real `keeper-agentd run` serving Nixie on `tgdrive` from the e2e model
stub (40 reasoning tokens, then 240 answer tokens, 25 ms apart), and keeper-core viewers. A second
Nixie client stood in for R-HQ-30's runtime half and wrote only an `agent_profiles` row whose
`tools` named `journal_append` and `journal_read`.

- **Growing answer** (four runs): the owner's view grew in 151–223 steps over about 6 s, median
  step 25 ms, each step a prefix of the final 1,090 bytes, the last equal to the final row. A
  second device that paired and opened mid-answer showed at once at least what the first had
  shown by then (98/98, 118/118, 146/146, 274/274 bytes), then the same final row byte for byte.
  With *Show details* on (now the Verbose mode), the final row showed model, 50/240/290 tokens,
  time to first token 27–33 ms and duration about 7.1 s. With *Show thinking* on (now the Thinking
  mode), the thinking streamed and stayed.
- **Model to viewer** (three runs, 544 steps): from the stub emitting a token to keeper-core's
  session view first showing that length, p50 3.6 ms, p95 21.5 ms, p99 256 ms, max 556 ms. The
  outliers fall while the second device pairs in the same process. This is one host over
  loopback and includes the instance's work, so it is **not** NFR-124's figure: the tailnet
  publish-to-receive p95 (electra → hesperia) is still owed.
- **Non-reader:** a person invited when the session opened saw the answer grow and the same
  final row, was shown no thinking or tool step with *Show thinking* on (now Thinking), and held no
  `reasoning` or `tool_calls` row for the session.
- **Tool switches:** both offered tools were listed on. `journal_append` switched off on the
  owner's device showed off on the guest's, and the next turn offered the model only
  `journal_read`; switched back on, the next turn offered both. A name not offered was refused.
- **Killed instance:** SIGKILLed with 202 bytes shown, the view kept those 202 bytes and said the
  answer stopped 28.7 s later, once the claim's lease had lapsed.

The Agents-window archive test assumes no agent answers its messages, and the pairing test needs
two databases; neither fits this rig, which runs an agent on one database.

## People in a session

A session can hold several people. **People** in the session header opens a sheet (a side sheet
on the desktop, a bottom sheet on a phone) listing the session's members — the owner, the other
people and the agent — read by Rust from the session's `session_members` rows and the directory of
the session's own database. Every message names its author from the `author_actor` the server
stamped on the row; a client cannot choose it, and the server refuses a row that claims another
author.

Only the session's owner changes who is in it:

- **Add** offers only the people of the session's database who are not in it yet — in a
  `tgdrive` session never someone who belongs to `neuradrive` alone. Adding writes the session's
  `invite`, so it can wait offline in the outbox; the person shows as *Joining…* until the server
  has written their membership, and their devices then list the session and its newest page. The
  server refuses an invite naming anyone outside the database, and an invite changed by anyone but
  the owner; a refusal shows in the sheet. There is no invitation by address.
- **Can read thinking and tool results** is a person's reader flag, off when they are added.
  Turning it on lets the server send them the session's reasoning, tool calls and approvals after
  their scope is refreshed; turning it off makes their devices evict those rows. Both are the
  server's `grant_trace` / `revoke_trace` commands, so they need the database to be syncing; an
  offline device says so and changes nothing. A device draws the thinking, tool steps and
  approvals — in the session and in the decisions panel — only while its person's membership reads
  the trace: from the moment it knows the flag is withdrawn or the person removed, none of them is
  shown and keeper refuses a decision, even while rows not yet evicted (or a decision still in the
  outbox) are on the device. The server refuses such a decision too (`hq.v8.not_reader`), and the
  request says so in keeper's words.
- **Remove** (with a confirmation) is the server's `remove_member`: the person leaves the invite,
  their membership is deleted, and their devices drop the session and its rows, keeping only their
  own writes still waiting to be sent.

A server change whose answer never arrives (the connection dropped after the server acted) — or
arrives and cannot be read — is not lost: keeper keeps the request until the server's own answer
(applied, replayed or refused) settles it. The sheet says keeper did not hear back (or could not
read the answer, which also says something must change before asking again — keeper never asks
again by itself), the database syncs at once, and asking again about the same membership asks
under the same request id, which the server answers from its record (`cached`) instead of acting
twice — so a removal repeated after the person was invited again on the server does not remove
their new membership. The membership is told by the fields fixed when it was made
(`added_at_ms`, `added_by_actor`, `role`, which no write changes): a read point or a reader flag
landing meanwhile is the same membership and keeps the request. The server stamps every new
membership of a person in a session a later `added_at_ms` than any before it, even when they are
invited again within the same millisecond, so two memberships never look alike. Once the device
has seen them invited again (a new membership, stamped anew), Remove is a new removal.

Every Remove and reader switch also names the membership it is about — the `added_at_ms` the
sheet shows — and the server refuses it (`hq.member.changed`) when that person's membership is
another one by now, or — for a reader switch — gone (a Remove of someone gone is refused as not a
member): a decision about someone never lands on their next membership, which nobody decided
about. The sheet then says so, and shows them as they are.

A replayed answer is the end of that request: it ran before, and keeper asks nothing more of its
own — it syncs and the sheet shows where things stand, which may be a change made since on another
device. A new decision is the person's next switch or Remove, sent under a new request id about
the membership shown then. A reader switch whose result the sheet has shown is settled even if its
answer never came, so switching it again later — after it was undone on another device, say — is
that new decision at once. Refusals are matched by the server's stable codes
(`hq.remove_member.*`, `hq.member.changed`, `hq.v3.owner_only`,
`hq.v11.invite_outside_directory`, `hq.v3.owner_cannot_leave`, `hq.v8.not_reader`); any other is
shown as the server said it.

A member who is not the owner sees the same list without those controls and may **Leave
session**, which deletes their own membership; the server then takes them out of the invite. The
owner has no Leave, since nobody else could change who is in the session and 106.4 has no
transfer of ownership; the server refuses an owner's leave as well (`hq.v3.owner_cannot_leave`,
R-HQ-40). If a device that held its person as a member leaves anyway and the server refuses, the
session comes back and its sheet says the server refused the leave. If the session open on a
device leaves its list — the person left or was removed — the window closes it and says so, and
drops that note if the session comes back.

Development fixtures: `?sessions=launch&people=member|refused|offline|long`, with
`&platform=phone` for the phone shell.

### Measured: many people against the real server

2026-10-21, `hq_live.rs`'s `many_people_in_one_session_by_the_owner_and_the_server` on delectra:
the keeper-hq server from a `git archive` of `hq/106-4-members` 654ec57, again of 7490af5 on
this branch rebased onto 106.3's fix wave 9f7a93ba, and of 941ccdc after the review (Bun 1.2,
over a throwaway Postgres 18), serving this branch's contract (schema 3) in place of the archive's
own schema 2 — keeper-hq still pins schema 2, and a schema-3 client receives no rows from a
schema-2 server. One
real `keeper-agentd` served Nixie on `tgdrive` and another Dixi on `neuradrive`, from the e2e model
stub; tgorka owns the session, kuba belongs to `tgdrive` alone and marta to `neuradrive` alone.

- **Directory:** a `tgdrive` session offered only kuba and a `neuradrive` one only marta; keeper
  refused adding marta to the `tgdrive` session, and an invite naming her written anyway was
  refused by the server (`hq.v11.invite_outside_directory`).
- **Add:** kuba's device listed the session 150–310 ms after the owner added him and showed its
  newest page; his message showed on the owner's device as `kuba`.
- **Authorship and ownership:** a message from kuba's device claiming tgorka was refused
  (`hq.v1.author_actor`); an invite kuba wrote naming one more person was refused
  (`hq.v3.owner_only`).
- **Reader flag:** without it kuba held no `reasoning` or `tool_calls` row after a thinking
  answer; granted, the reasoning reached him 170–240 ms later; withdrawn, his replica evicted
  every reasoning, tool call and approval row of the session and kept the answers.
- **Approvals follow the flag** (106.4 review, against keeper-hq 941ccdc with Nixie offered
  `run`): granted, kuba was shown the agent's T2 approval and held its tool call; the moment his
  device knew the flag was withdrawn his view showed no approval and nothing of the session waited
  for him in the decisions panel (the window had already evicted the rows in that run, so the
  rows-still-here boundary is covered by keeper-core's unit test, not by this run).
- **Remove and leave:** removed by the owner, the session left kuba's list and every row of it
  left his replica within about 110–120 ms, and he left the owner's sheet (and could be added
  again); added again and leaving himself, he was gone from his list, the owner's sheet and the
  invite.

The *People* sheet was walked in the agent desktop's Chrome over CDP on the mock shell at 1280
and 420 px (the phone shell): open and close (Close and Escape, focus back on **People**), add
(*Joining…*, then a member), the reader switch on and off, remove with its confirmation (focus on
Cancel, back on **Remove** after Cancel, on Close after removing), a member's view without the
owner's controls and **Leave session** (the window closes the session and says so), the refused
and offline sentences, and twelve-word names with no document or sheet overflow. Every button
measured at least 44 × 44 px; the runs recorded no console error or failed request.

After the 106.4 review the long-name fixture's removal confirmation was measured with it open: it
had grown to 954 px (the sheet's scroll area 550–555 px wider than the sheet) and now fits — at
1280 px a 383 px confirmation in the 416 px side sheet, at 420 px 388 px in the bottom sheet,
neither scrolling sideways, its destructive control reading *Remove* with the person's whole name
as its accessible name, both controls 44 px tall and at least 64 px wide.

## Bringing another agent in with `@` (109.24)

A line that starts with `@<handle>` in a session's composer is not a message to the session's
agent (Q109-21 a, R-HQ-76). keeper, acting as the person, starts a **child session** for the agent
the handle names: its row names this session as `parent_session`, it lives in the same database,
the person is its owner, and the whole line is its first message — the three rows in one commit,
through the same creation path as New session (`keeper_core::hq::sessions::create_child`). That
is the shape keeper-hq's V3 lets a person member of the parent insert; V11 keeps it in the
database. The session's own agent is not asked about it, and no agent prompts another: only people
start these threads.

Typing `@` at the start of the draft opens a menu of the database's directory agents other than
the session's own, matched by handle or a word of the name — Rust's rows
(`keeper_core::hq::mention::preview`), drawn by the mid-text menu's keyboard model: ↑/↓ move,
Enter or Tab insert, Escape closes. A mention of the session's own agent, of an agent of another
database, of a handle no agent has, or with nothing after the handle is refused before anything is
written, with a sentence under the composer; the draft stays. A mention with files is refused too:
the files go in the thread once it exists. An `@` later in a line is text for the session's agent.

The parent draws the threads it started inline, where each was started: the agent's name, the
person's line, the agent's latest turn as 109.13 folds it (`AgentChildSessionVm.items`, a `child`
item in the parent's `items`), and **Open thread**, which opens the child like any other session
in the list. The view is bounded: only threads started within the months the parent shows, the
newest 30 (`MAX_INLINE_CHILDREN`), and when older ones of those months are left out the parent
says how many, above the first, and that they open from the sessions list. For each thread only its
first line and its newest turn are read — the newest turn's id first, then that turn's own rows,
tool calls and chunks (`timeline::latest_turn`, `live::extras_of`, `live::turns_of`); the thread's
earlier turns, their tool calls, thinking and chunks are never read, so what a parent reads does not
grow with a thread's history. The inline fold is its one summary line, Rust's sentence — never its
steps, their failures, its thinking or lead-ins — and it never opens, in any view mode, so the answer
stays first in the box; the details are the thread's, behind Open thread. Nothing inline is a live
region: the thread announces its work in its own view, and the inline line names whose work it is
(*Tola Grey: Searching the web… · 1 step*). The person's line and the answer are cut at 1,200
characters (`MAX_INLINE_ANSWER`, `truncated`), and the answer sits in a height-capped box: whenever
either is cut or the box overflows, the block says *This continues in the thread.* Measured with 200
threads each answered with 20 KB: 510 ms and a 4.3 MB view before the bound, 29 ms and 62 KB after;
30 threads whose newest turn ran 1,000 tool calls each stay at 63 KB. An archived thread is one line, *Thread archived*, with
Open thread: its rows are read only when it is opened.

A thread is told apart by its first line's id (`thread_id`), never by its session id: a refused
child's rows leave the replica, so the next creation mints the same id again. If the server
refuses the commit (`hq.v3.parent_not_member`, `hq.v11.agent_outside_directory`, …), the child
never existed: the line stays on the parent, marked refused, with the reason in plain words read
from the replica's outcome journal, and there is nothing to open. Two refusals are two lines, and a
later mention that reuses the id hides neither.

A mention declines what the session's banner showed, as any message does (Q109-12 a): the local
checks run first, so a mention refused on this device declines nothing; then the declines; then
the child's commit (`mention::send_line` passes 109.11's `approvals::decline_pending` as
`mention::start`'s `decline` step; the composer sends the banner's shown requests with the line,
`agent_send_line`'s `declines`). Answering an
agent's option button still goes to the session's own agent verbatim (`agent_send`); only the
composer's line is read for a mention (`agent_send_line`).

Development fixture: `?sessions=launch`, alpha's *Plan the next release*: `@tola-grey …` is
answered inline with a long answer cut for the parent, `@mason …` is refused by the mock server,
`@researcher …` is beta's agent, `@assistant …` is the session's own, any other handle is unknown;
archiving Tola's thread draws it as archived in the parent.

## Which instance answers

An agent is one actor and can run as several instances — `nixi@electra` on the server and
`nixi@hesperia` on a laptop are two clients of Nixi, sharing her sessions and memory through the
server. **Settings › Agents & voice › Answers from** lists, per agent, its instances from the
directory of each database you share with it (their `clients` rows, and the name, description and
definition hash of their `agent_profiles` rows), each marked **Online**, **Offline** or
**Not known here**. Online is the instance's presence on `dir:<database>`, which this device
receives over its own socket; while that database does not sync on this device nobody here can
tell, so it says **Not known here** rather than offline. A socket that has just connected is no
answer either: the server sends the peers already on the key in the step that registers it, so the
replica asks — a leave on `dir:<database>` (taken silently while the key is registered, refused
while not) and one on a key it never holds (always refused, after the first) — and only once that
second refusal comes alone does it treat the presence it holds as complete
(`ReplicaState::presence_known`, keeper-hq's `PresenceGate`). Until then, and again after every
reconnect, the instances are **Not known here**. Nothing of an old socket reaches a new one: the
socket's reader shares one queue with the next socket's, so whenever keeper lets a socket go —
lost, unreachable, refused or closed — it joins that reader and then drops every frame it left
queued, before a new socket starts. A join of an instance that stopped while that socket died is
therefore never shown on the next socket, whose snapshot does not mention it (row changes dropped
with it come back in the next round, from the cursor they never moved).

Choose the instance that answers your new sessions; the others answer, in the order shown, while
it is offline (**Ask … earlier** / **later** reorders them). One instance alone is a whole choice.
The choice is an `instance_prefs` row (default instance and ordered fallback) in your own `actor`
scope — never a setting of this device — so every device of yours follows it after one sync. A row
cannot cross databases (AD-433), so keeper writes a row into each database this device holds that
you share with the agent: your order less the instances that database's directory does not list
(R-HQ-43). No database ever receives a client id it does not list; one that lists none of the
instances you chose gets no row change, and the section says so in that database's terms. The
section shows one order — the rows written together last, merged back into one — and, whenever the
databases' own orders differ, a write still waits or one was refused, each database's line: the
order its row asks, **saved on this device, waiting to sync**, or the server's refusal. Above them
it says where the choice is saved and where it waits (**Saved in tgdrive; waiting to sync to
neuradrive.**), so a row that is the same here but not yet at its server is never shown as
agreement. What waits is read from the replica's outbox and a refusal from its outcome journal —
the replica file, scoped by server, person, database and row (`<person>/<agent>`) — so neither is
lost when keeper quits, and another person's or server's never shows. A choice made on a device
that holds only one of the databases reaches only that one (DW-E106-7); when a database's row is
not the order shown, or a database refused it, **Apply this order to all databases** writes the
order shown into each again without changing it, and a new write sets the earlier refusal aside.
`agents.default` keeps its meaning: which agent keeper opens at launch, not which of its instances
answers.

The instances route by the row themselves (keeper-hq's `routing`): for a session nobody holds, the
first instance of the order whose presence is online claims it, and with no row any online instance
may. keeper records no instance on the session (`preferred_client` stays empty) and never pings an
instance. An open session stays with the instance holding its claim; another takes it over only
after the holder's lease lapses.

An instance whose definition hash differs from its siblings' is marked **Different definition**,
with the start of its hash: with a majority hash, the instances that differ from it; with no
majority, every instance, since each differs from the others.

The open session's header names the instance answering — the claim holder — and each instance's
state, from the session's own database alone: **nixi@electra is answering.**; a holder whose
presence is gone is still named, offline, until another takes the session over once the lease runs
out; a session nobody holds names the instance that takes it next by your order, or says it waits
when none of them is online. A holder whose claim stopped renewing — its row unchanged a whole
lease as this device watched it, or renewed more than two minutes ago by its own clock — is named
as the one that **held this conversation; its lease ran out**, never as answering, with the
instance that takes it over next (never the lapsed one) or that it waits. The header is drawn again
when the lease would lapse, with no new row to wake it.

Development fixtures: `?sessions=launch&instances=two|offline|lapsed|disagree|stale|refused|long`,
with `&platform=phone` for the phone shell. They keep the choice per agent and per database as Rust
does: a write waits about 0.7 s for its server, `refused` has beta refuse it and keep its old row,
`stale` adds an instance only alpha lists, and Researcher's single instance can be chosen.

### Measured: two instances against the real server

2026-10-21, `hq_live.rs`'s `two_instances_answer_by_the_persons_choice_and_who_is_online` on
delectra, twice: the keeper-hq server from a clean `git archive` of `hq/106-3-tools` 3e8db43
(schema 3, Bun 1.2.23 baseline) over a throwaway Postgres 18, the e2e model stub, and two real
`keeper-agentd run` processes built from the same archive — `nixie.electra` and `nixie.hesperia`,
each with its own token, home and state, both serving Nixie on `tgdrive` and `neuradrive`; tgorka
in both, on two keeper-core devices ("mac" and "phone").

- **The choice is a row:** `nixie.hesperia` chosen on the mac showed as the phone's choice 139 ms
  and 55 ms after it was written, with no disagreement; each database held one `instance_prefs`
  row `tgorka/nixie` naming it, authored by tgorka.
- **New sessions follow it in both databases:** a new session in `tgdrive` and one in `neuradrive`
  were taken by `nixie.hesperia` 0.6–1.2 s after they were written, the header saying
  **nixie.hesperia is answering.**
- **Offline, and the other answers:** with `nixie.hesperia` SIGKILLed, the mac's view showed
  it offline 2–4 ms later (its socket closed, so the server sent its presence leave at once; a
  partition that keeps the socket half-open waits for the server's presence expiry instead, which
  this run did not exercise). A new session was taken by `nixie.electra` 0.5–1.2 s after it was
  written, the header naming it. The two sessions the killed instance held said
  **nixie.hesperia holds this conversation and is offline; …** until its lease lapsed, and were
  taken over by `nixie.electra` 30.0–31.6 s after the kill.
- **A held session stays with its holder:** with `nixie.hesperia` — the default — back online for
  15 s, all three sessions stayed with `nixie.electra`.
- **The new holder is named:** with `nixie.electra` SIGKILLed in turn, the header of the session it
  held named `nixie.hesperia` 25.3 s and 26.8 s later, once the lease lapsed and it took over.

The instances' labels are their client ids here because the rig's registry names each client by
its id; the deployed registry names them `nixi@electra` and `nixi@hesperia` (R-HQ-54). The same
with *nixi@hesperia*'s container on the Mac (105.3) and the installed apps is owed on the devices.

After the 106.5 review the same case ran again on delectra against a clean `git archive` of
keeper-hq `hq/106-3-tools` 6406b32, on the tree rebased onto 106.4's second review round. Both
instances showed **Online** only once each device's presence ask had come back (4–37 ms after
pairing). The choice showed on the phone 128 ms after the Mac wrote it, and the new sessions were
taken in 0.5 s. With `nixie.hesperia` SIGKILLed it was **Offline** 108 ms later. The device named it
as having held its sessions (**nixie.hesperia held this conversation; its lease ran out, so
nixie.electra takes it over next.**) 29.4 s after the kill, before `nixie.electra` took them over at
29.9–30.5 s. Nothing ever said a holder whose lease had run out was answering. The rest of the case
held as before.

## A push from the agents server

Story 106.6 (FR-855, FR-856; AD-432, D-35, R-HQ-44). With keeper closed, the phone learns of
two things and nothing else: an approval waiting for you, and your own proxy's answer
(Nixi's for tgorka, Dixi's for Marta) in a session you are in. Asks are not pushed.

**What the phone registers.** The iPhone's APNs token (as Sygnal reads a pushkey, base64)
and its app (`dev.tgorka.keeper.ios` or `.ios.dev`, by the build's `aps-environment`), or
Android's UnifiedPush endpoint and its app (`dev.tgorka.keeper.android`), go to the agents
server with your existing push gateway (`push.gateway_url` on the iPhone, read through the
same grammar; the gateway discovered on the endpoint's server on Android) as this device's
push target: `PUT /clients/self/push-target`, once per change and again after every start
(`keeper_core::hq::push::PushTarget`, `Hq::set_push_target`). Clearing the gateway, or
Android losing its endpoint, sends `DELETE`. A device not paired, or removed, sends nothing.

**What the server sends.** Ids only, in the Matrix Push Gateway's `notify` shape: `event_id`
is the item, `room_id` the session, `default_payload.keeper_hq` names
`{database, session_id, kind, item_id}`; the iPhone's adds `mutable-content` and an alert
that is only a key (`KEEPER_HQ_ITEM`, localised on the phone as *Open keeper to see what
arrived.*). No title, text or summary reaches the gateway.

**What the phone shows** is decided on the phone (`keeper_core::hq::push::classify`), from
one authenticated read of the item: `GET /<database>/push/item/{kind}/{id}` with this
device's token, the pairing of the server whose replica of that database this device holds.

| item | shows |
|---|---|
| a pending approval you may decide, below T4, whose whole request fits — the tool call the server read with it, its tool and exact arguments on one line of at most 120 characters, no control or direction character and no character a reader cannot see in any of them; not a Paseo start or steer; not a `run` asking for the network | *nixie asks: \<keeper's summary\>*, the tier's words, the request (`run {"argv":["git","status"]}`); **Approve once** and **Deny** |
| the same at T4, longer, of a tier keeper cannot read, or a request keeper cannot show whole (no tool call from the server, arguments it cannot read, an invisible character, a Paseo prompt, a networked run) | the tier and *open keeper to read the whole request*; **Deny** only |
| an approval not yours, already decided or expired | keeper's words, no action |
| your proxy's answer | the agent and its words |
| an ask, a person's message, your own | nothing |

The item read returns the approval row and, beside it, the tool call it runs (`tool_call`,
the call's `tool_calls` row, read only when it is that approval's own call in that session);
nothing of either reaches the gateway. keeper's summary is a sentence about the call
(*Run \`git\` in workspace/keeper* names the program, not its arguments), so it goes in the
title and never decides whether **Approve once** is offered: only the whole request does
(R1066K-01, `hq::push::fits`). Anything
that cannot be read — no pairing, the server or the tailnet unreachable (DW-E106-5), a refusal
(`401`, `403`, `404`), an answer about another item — is the generic notification with no
actions.

**A character a reader cannot see refuses Approve once (R1066K-05).** `rm report<U+200B>.txt`
reads as `rm report.txt` on a lock screen and removes another file. So the tool's name, every
key and every value, at any depth, is checked against every Unicode format character
(General_Category `Cf`, U+200B, U+2060, U+00AD, …) and every Default_Ignorable_Code_Point —
the variation selectors and the tag characters among them — as of Unicode 16.0
(`agents::push::HIDDEN`). Keeper neither strips them nor renders them as escapes: such a
request offers **Deny** alone and *open keeper to read the whole request*. A request whose text legitimately holds one (an emoji with a
variation selector) is decided in keeper.

**The same item may arrive twice; keeper tries to show it once.** Android tags the
notification with the item. On the iPhone the identity is a key on the notification,
`keeper_hq_key` = `<database>/<kind>/<item id>` (`hq::push::KEY_INFO`), not the notification's
identifier: iOS names a remote notification itself, the person's gateway sends no APNs
collapse id (Sygnal has none), and re-posting the notification locally would need the remote
one to show nothing, which needs the filtering entitlement (DW-408). So on arrival the
extension asks Rust for the item's key before it reads anything (`keeper_nse::answer_key`),
and every notification of the item carries it — composed, generic, and the fallback shown when
the extension runs out of time — the generic one carrying nothing else of keeper's. That call
starts the delivery's one deadline (twenty seconds, `keeper_nse::DEADLINE`), which the wait for
its turn, the read and the waits on the notification center share; a delivery still queued at
it reads nothing (R1066K-06). Every answer then names the notifications with the same key to
remove first (`hq::push::Fold`): those on the lock screen, those of the extension process that
arrived before it and those it handed iOS whenever they arrived, waiting until the deadline for
the ones not there yet. iOS's expiry asks Rust too, at once and outside the serial queue, so
the fallback removes the item's others instead of waiting behind the read that used the time
up. Being named is not being handed (R1066K-07): an earlier delivery the fallback named while
its read was still going shows when that read ends, and removes the fallback, so the one
handed iOS last is the one left. A removal is kept until the extension sees the notification
gone, and every later answer names it again meanwhile. A re-posted *Open keeper to finish
deciding.* keeps the key too.

This is best effort, not a guarantee: the record is the extension process's, a removal only
applies to a notification already on the lock screen, and iOS may end an extension before it
acts. **Proposed deferred work** (number to be assigned by the coordinator): *item-level
collapse on the APNs request.* The push that reaches APNs carries an `apns-collapse-id` derived
from the item key, so iOS itself keeps one notification per item whatever the extension
managed: `hq1-` followed by the unpadded base64url SHA-256 of the whole key
`<database>/<kind>/<item id>` — 47 ASCII bytes for every item. Not the key itself: APNs refuses
a collapse id over 64 bytes (`BadCollapseId`), and a valid key passes that easily (a database
name may be 63 bytes and an item id 1024, and the id need not be ASCII). Not a truncation of it,
nor the item id alone: two items sharing a prefix, or one id in two databases or of two kinds,
would collapse into one notification. A digest of the full key is fixed-length, the same for
every delivery of the item, distinct across namespaces, and computed from ids alone. Sygnal's APNs
pushkin sends no collapse id, so this needs a gateway that sets one from the push's
`keeper_hq` ids (a Sygnal change or a gateway of keeper's own; the agents server still sends
ids only, D-35). The runs that prove it: the gateway's tests include a key of the longest valid
database name and a long item id (the header stays 47 bytes and APNs accepts it), and one item
id in two databases and as two kinds (distinct collapse ids); on kalypso, three deliveries of
one approval with the server blackholed (it accepts and never answers), plus one whose
extension expires, leave exactly one notification on the lock screen.

**An action** writes the decision row through the session's own replica, exactly as the card
does (`Hq::decide_pushed`, `hq::approvals::decide_pushed`): the request and digest the
notification showed, only an action its category offered, never *Approve once* on a request
the replica holds at T4 (the server's whole-commit validator refuses a T4 decision from the
claim holder's host in any case, AD-428). On the iPhone an action that launches keeper in the
background installs the settings layers first — the same files and account layers setup
installs, so the server is the one the effective `agents.server_url` names, never a stored
address a file shadows (R1066K-03) — then starts the agents client alone, no Matrix account
and no drive sync, and waits up to twenty seconds for the request to reach its replica and
for the server to take the decision; otherwise the notification is replaced by *Open keeper
to finish deciding.* with the reason. **A tap** opens the item's session in the Agents window
(`use-push-open.ts`, `PushOpenVm::Session`), over New session if that was open (its draft is
kept, above). A session this device does not hold yet — Keeper was closed when it was created or
the person added — is not a failure while it may still arrive: Rust answers its open as still
arriving (`HqError::Arriving`, retriable) while the replicas on disk are opening — every
restart, a relaunch from the tap included, whatever the address — the server has yet to say what
is open, or its database syncs, and the view says so and asks again when Rust's list brings the
session or its status moves (a restart always moves it once its replicas are open, or failed).
No timer: once the replicas are open, an address cleared, a removed device or a database not
open to it ends the wait with Rust's sentence. Even after a refusal, the session appearing in
Rust's list opens it.

**The Android tablet gets the agents server's push while keeper is running.** After Android
has stopped keeper's process it does nothing with it: the distributor's message posts the
generic notification, and an action on a notification posted earlier cannot decide. That is
deferred by the owner (R-HQ-58, DW-E106-15: the cold-start item read, a decision queued from
an already-shown action, the session target kept through the replacement), to be revisited
with the tablet for a device run.

**Proved on Linux:** the table, the fit over the whole request, an invisible character of
every class refusing *Approve once*, the ids grammar, the fold of a second delivery, the
actions and the tap (`keeper-core/src/hq/push.rs` tests); the push
target sent once per wish, cleared with the gateway, retried at the next discovery and never
from a removed device (`hq/tests.rs`); the decision row written after the request arrives, T4
refused, a missing request refused at the deadline (`hq/window_tests.rs`); a cold launch
starting the server a layer names, with no stored address and over a different stored one
(`keeper-core/tests/hq_cold_action.rs`); the extension's read over a real HTTP listener
answering recorded item JSON — one `GET` with the bearer token, the generic notification for
every refusal and an unreachable server, a short summary over a request it cannot show whole
offering *Deny* alone, and two deliveries naming the right removals with the tailnet off and
on, the second arriving while the first is read, a fallback replaced; three deliveries queued
behind a server that takes the connection and never answers ending within one deadline; the
expiry of a delivery queued behind such a read answered at once with the item's removals, the
queued read giving up at its own deadline and removing nothing after it; a removal named again
until it is reported gone (`keeper-nse` tests). What iOS then shows is not proved anywhere off
the device. Android's one notification per item (`keeper-core/tests/unifiedpush_android.rs`);
the webview's landing (`src/hooks/use-push-open.test.ts`). **Inspection only:** `push_ios.rs`,
`push_android.rs`, `hq_ipc.rs`, `lib.rs`'s `install_settings_layers`,
`NotificationService.swift` (CI's macOS and iOS jobs compile the Rust; the Swift is compiled
only by the device build). **Owed on devices:** kalypso (the token reaching the server, the
notification through Sygnal, the actions with Face ID from a cold launch with the server set
only in a settings file, the tap, the generic notification with the tailnet off, an approval
with U+200B in an argument offering *Deny* alone, deliveries of one item — the second arriving
while the first is read, three with the server blackholed, one whose extension expires while
queued — and how many notifications iOS leaves) and the Android tablet through ntfy while
keeper runs.

## Files in an agents session

Story 107.1 (FR-857…FR-860; AD-429, AD-433). A file or photo put in an agents session is a
syncular blob of the session's own database: uploaded once, before anything references it, and
referenced by `attachments` rows committed with the message whose text is the caption, in one
commit. Nothing goes into a drive, and no bytes cross IPC as JSON: the paperclip and
drag-and-drop hand Rust **paths** (`agent_attach_path`), a paste hands it a **raw IPC body** with
the session, type and name in headers (`agent_attach_bytes`).

**One Send, one message** (R1071C-02). Neither call writes anything: each reads and checks its
files and stages them in the process under handles it returns (`attachments::Staging`), never in
the replica, so nothing staged can reach the server. An image preview stages its snapshot
on selection; Send reuses that handle and stages the remaining paths and pastes, then hands
every handle with the caption to
`agent_attach_commit`, which takes them all and commits them as **one** message, its text the
caption, once; a handle no longer staged refuses the whole Send and commits nothing. Handles are
spent by the commit, released by `agent_attach_release` when the composer drops them, and
released with their session when its last view closes; a process that dies holding some leaves
nothing behind.

**The bound.** One file may have at most `attachmentMaxBytes` from the contract's
`syncular/limits.json` — 26,214,400 bytes (25 MiB) — the number the server's validator reads too.
A larger file is refused on the device before anything is uploaded or written, with the agent
host's sentence (*huge.zip is 25.1 MB and an agent takes at most 25.0 MB from one message, so it
did not get the file.*, `keeper_core::hq::attachments::too_large`); the server refuses it again if a device skips the check. A paste is measured against
the bound before the shell copies its raw body out of the request, so an oversized paste costs no
second copy of itself.

**Upload before reference.** keeper hands the bytes and the commit that names them to the replica
in one call (`Replica::commit_with_blobs`), which its owner thread runs as one step: it caches each
blob under its content address (`sha256:<hex>`) and queues its upload, then commits the message and
its rows. No round runs and nothing is evicted between the two, so a blob is pinned throughout —
by its upload row, then by the commit — and a round that uploads it and is then cut off before its
push leaves it pinned by the commit, across a relaunch, until the server takes the message. If a
blob or the commit fails, what that call staged and nothing else holds is unstaged again. What
each blob was before the call (cached? already queued?) is written beside the replica
(`<replica>.blob-access.sqlite`, `unfinished_commit`) before the first is staged and cleared once
the message is in the outbox or the blobs are back: a process that dies in between leaves that
record, and the next open puts the blobs back before any round, so no blob is ever uploaded
without the message that names it; one another waiting message pins stays queued for it
(R1071K-07). If putting them back fails too (storage full, say), the record stays and the
replica's owner runs no round — a round uploads every queued blob, named or not — until it has
put them back: it tries again on every wake, backing off while it fails, so neither another Send
nor a relaunch is needed for a failed Send's file to stay on the device (R1071K-10). The
replica's next round uploads every queued blob (`POST …/upload-grant`, then
`PUT /<database>/blobs/<blobId>`) before it pushes the commit that names it. `blob_ref` is syncular's canonical BlobRef JSON,
`{"blobId":…,"byteLength":…,"mediaType":…,"name":…}` in that key order. Offline, the staged blob
is pinned in the replica file — it survives a relaunch — and uploads once on reconnect; the same
bytes attached twice are one blob.

**The chip** (`AgentMessageVm.attachments`, one `AgentAttachmentVm` per file: name, type, size and
its label, state, the refusal sentence, and the URL that opens it). A chip of this device's
message reads **uploading** while its commit is on its way to a server this device reaches,
**waiting** while it cannot reach one (offline, no server, refused credential), **sent** once the
server has it — always, for anyone else's — and **refused** with the server's reason as a sentence:
the commit refused (`blob.not_found`, `hq.attachment_writer`, `hq.attachment_message`,
`hq.attachment_size`, `hq.attachment_blob_ref`, `hq.attachment_blob_size`, `hq.v1.author_*`), or
the file's upload (`blob.too_large`, `blob.hash_mismatch`). A refused commit's rows leave the
replica, so its message is read back from the replica's outcome journal and shown in its place,
state **refused**, with its caption.

**A refused upload** is resolved where it arrives, so it never stops the replica. The adapter
knows which blob the refused grant or `PUT` was for; it ties the refusal to every commit in the
outbox that names that blob and keeps it beside the replica (`<actor>.blob-access.sqlite`, table
`upload_refusals`), then takes the blob out of the upload queue and out of those commits' pins.
The next round uploads whatever else is staged and pushes every commit — those too, which the
server refuses without the body, so their messages come back through the outcome journal like any
refused commit. Nothing else changes: other commits, and every other body those commits pin, stay
as they were. That file's chip reads **refused** with the upload's reason; the other files of its
message read **blocked**, naming it (*tiny.csv was not sent: the agents server refused huge.zip,
attached to the same message. Attach tiny.csv again.*); no other message's file is touched. The
explanation outlives a relaunch, and goes once its commit neither waits nor stands refused.

The rows of an active session's attachments are held in a window beside its messages; their bytes
are not. An archived session holds none — until it is opened: its messages come through the
server's `sessionPage` and its attachments' rows are held while it is open (`sessions::plan`), so
its chips show and its files open through the same row check, downloaded only when opened; the
rows go again when it closes.

**Composer and chip layout (UX-DR-E107-1).** The session composer reuses the paperclip,
native file drop, file paste, and the platform's Take photo / Choose photo controls.
The existing photo-to-JPEG conversion remains in place; a Mac picker still passes the
original file path. Selected files sit in a wrapping tray above a full-width text field,
with removal buttons and the size for pasted/photos' bytes. A native path has no size
metadata until Rust reads it: its tray says **Size on send**, never an invented number.
After Send, Rust's message owns the chip and its size/state; the tray is not another
outbox. Send stages the whole selection under Rust handles (paths in one call per run of
them, each pasted/photo body as its raw IPC request), then one `agent_attach_commit` sends
them all with the caption as one message (R1071C-02). A staging refusal commits nothing:
what was staged is released, and the whole selection and the caption stay for a retry.
Files added during a send stay selected for the next one. A paste that carries a file and
plain text (a screenshot with its words) stages the file and puts the text into the draft at
the caret; plain-text-only paste is the browser's own, and text or HTML never becomes an
attachment (R1071C-01). A pasted or picked file is selected from the moment it is pasted or
picked, not when its bytes have been read: until every file of that paste or pick is read,
Send is disabled, Enter does nothing and the composer says *Reading the file to send…*; the
batch then joins the tray whole, so a caption never leaves before its file and no message
carries part of a multi-file paste. A failed read adds none of that batch and says so
(R1071C-04). Each batch still being read shows as a *reading…* chip beside the tray, with a
remove control; removing it, or pressing Escape in the composer, drops that batch (Escape
drops all of them) and releases its hold on Send while the caption stays. A read still
pending after `FILE_READ_TIMEOUT_MS` (60 s) fails like a failed read, with the same
sentence. A dropped or failed batch's read that finishes later adds nothing: only a batch
still pending when its read lands joins the tray, and every other batch keeps holding Send
(R1071C-05).

A message chip is a keyboard-reachable Open button: file icon, wrapping filename,
Rust's size label and state beneath it. Its refusal sentence sits immediately below,
inside the same bordered card. A *blocked* file — not refused itself; another file of its
message was — reads **not sent** in its label, description and announcement, with Rust's
sentence in neutral text; only the refused file is styled as an error (R1071C-03). State changes and refusals are announced politely.
Cards use one column on a phone and two where space permits. The text field sits on its
own row above the attachment toolbar and Send, leaving room to type at 320 px. An
enqueue-time refusal appears under the composer, with the draft and unsent selections kept.
No progress percentage, resend promise, or successful upload is invented in TypeScript.

**Own-image thumbnails (109.15, Q109-14 a).** The composer shows a 64 px removable
preview; a person's own sent images use a two-column grid and open the existing full-image
dialog on activation. Rust alone supplies `thumbnailUrl` after joining the attachment to
that person's `person` message. Another member's or an agent's images remain file chips.
`keeper-hq-blob` requests with `thumbnail=1` render PNGs with a maximum 256 px long edge,
from the staged snapshot or the already-cached blob only: a missing cached body never starts
a download. The handler checks ownership before consulting its bounded 64-entry memory cache.
PNG, JPEG, GIF and WebP are decoded with dimension, pixel-allocation and attachment-byte
limits. EXIF rotation and mirroring are applied before scaling. One bounded decoder reads
dimensions and pixels; compressed PNG ICC profiles over 8 MiB are refused by a streaming
preflight (the PNG decoder otherwise silently ignores profile-limit errors).
Unsupported raw formats (including HEIC), invalid images and missing bytes retain an
openable file chip. The existing photo-picker normalization can first convert HEIC to JPEG
when the webview can decode it; this is not a native HEIC thumbnail decoder.
Removing a selection releases its stage, even if staging finishes late;
Send consumes that same snapshot, and a failed Send restages retained selections for retry.


**Opening.** A chip's URL is `keeper-hq-blob://blob/<database>/<blobId>` (on Android and Windows,
`http://keeper-hq-blob.localhost/…`). The handler serves only a blob an `attachments` row of that
replica names, from the replica's cache, and downloads it on the first open — a second device
shows the chip as soon as the message syncs and downloads nothing until it is opened. It answers
as `keeper-media://` does (200, 206 for a `Range`, 416, 404 for anything it will not serve) with
`nosniff`, and serves pictures, sound, video and PDF as themselves, text as `text/plain`, and
everything else — HTML and SVG among them — as `application/octet-stream`, so no attachment runs
as a page.

Open mounts a modal preview only on activation, with the VM's URL unchanged on the first
request. Raster images use an image element; other types use a sandboxed frame without
script permission. The modal also offers Download file and Retry opening (a cache-busting
query on the same scheme); no `fetch`, blob decoding, or base64 path is involved.
Image load errors are announced. Frame HTTP failures are not reliably observable by the
webview, so Retry remains available even when the embedded viewer cannot report its error.
Closing returns focus to the chip. Installed WebKit/custom-scheme preview and download
behavior still needs the hesperia/kalypso device check; Chrome's mock blob URL is not that proof.

**The cache.** Each database's replica keeps at most **256 MiB** of blob bodies beyond what its
outbox pins (`keeper_core::hq::blob_cache::BLOB_CACHE_MAX_BYTES`, also the client's
`blob_cache_max_bytes`): ten of the largest files a message may carry, so a session's recent files
open again without a download, while a phone holding both databases spends at most half a
gigabyte on them. Past the cap, the least recently opened body the outbox does not pin is evicted
first — referenced or not; the next open downloads it again. A body the outbox pins (staged, or
named by a commit the server has not taken) is never evicted, even when the pinned bodies alone
pass the cap. When each body was last opened is kept beside the replica
(`<actor>.blob-access.sqlite`); the cache's size is the client's `blob_cache_bytes_approx`
(`Hq::blob_cache_bytes`).

### Measured: attachments against the real server

2026-10-22, `hq_live.rs`'s
`attachments_upload_before_reference_open_on_demand_and_are_refused_where_they_must_be` on delectra:
the keeper-hq server from a clean `git archive` of `hq/107-1-blobs` 307ed16 (its own contract,
schema 3; Bun 1.2.23, both databases with their blob directories) over a throwaway Postgres 18,
tgorka in both databases, kuba in `tgdrive` alone, Nixie in the directory with no instance running;
the test binary built in the agent container, run through delectra's gate. Four runs in a row,
03:01:18–03:02:02 UTC, and two more on the tree rebased onto 106.6's first review round,
03:15:31–03:15:53 UTC, 8–13 s each, every one green:

- **Attach → upload → commit:** a CSV and a PNG attached by path from the mac (behind a relay that
  sees every byte it sends) showed two chips at once, *uploading*; both read *sent* 1.8–2.7 s
  later. The server answered one `PUT /tgdrive/blobs/<blobId>` for each.
- **Open on demand:** the phone showed both chips as soon as the message synced, holding 0 bytes
  of blobs (`blob_cache_bytes_approx`); opening the CSV downloaded its 26 bytes, served as
  `text/plain`, and the cache then held exactly those.
- **Offline:** with the relay cut, a pasted file's chip read *waiting*; keeper was shut down and
  started again on the same files, the chip still *waiting* and the blob still staged in the
  replica file; the phone had no row. Restored, the chip read *sent*, the server had answered one
  `PUT` for it, and the phone held one row.
- **A non-member** (kuba's token) asking for the CSV's blob was refused `blob.forbidden`.
- **A reference to a blob the server does not hold** was refused `blob.not_found`; the mac showed
  the message *refused*, its chip saying *The agents server does not hold ghost.txt, so it refused
  the message that names it. Attach it again.*
- **One byte over 25 MiB** was refused on the mac before anything was sent (the agent host's
  sentence); staged anyway on a fourth device that skipped the check, the server refused the upload
  (413, `blob.too_large`), that device's chip read *refused* with keeper's sentence, and the phone
  never saw the message.

The server's log had no error line; it recorded each forced reference as `push.rejected`
`blob.not_found`. Two earlier runs of the case failed on the test's own counting, not on keeper:
one counted the offline file's rows across every session (the same bytes attached in the run
before are a second row of one blob — content addressing, as it should be), and two saw the
CSV's `PUT` request line twice: the transport sends an idempotent request again on a fresh
connection when its pooled one has died [INFERENCE: not traced to the byte in those runs].

**After the first review round** (Fix1071K, R1071K-01…06), 2026-10-22 04:01:22–04:02:27 UTC, the
same server revision and rig, three runs in a row, 16 s each, every one green. The relay now
rebuilds each connection's HTTP/1.1 exchanges from both directions' framing (start lines split
across reads, several messages in one read, `Content-Length` and chunked bodies, an upgraded
socket left alone), so the case counts the `PUT`s the device **sent** and those the server
**acknowledged** (2xx) separately; each file above was sent once and acknowledged once. The run
adds:

- **Cut after the upload, before the push:** the relay let blob requests through and closed any
  connection asking for anything else. A pasted file's `PUT` was acknowledged; the replica file
  still held its body, pinned by the message's commit (`_syncular_blob_commit_refs`), and the phone
  had no row. keeper was shut down and started again behind the same cut: still pinned, the chip
  *waiting*. Restored: *sent*, the phone showed it and opened it. The device sent that `PUT` three
  times and the server acknowledged three — once per round that reached it, since this server's
  grant answers `{}` rather than *present* for a body it already holds.
- **A refused upload no longer stops the replica:** the device that skipped the check staged
  25 MiB + 1 with its message; the server refused the upload (413, `blob.too_large`); the
  replica resolved it, pushed the message without the body, and the server refused that
  (`blob.not_found`, *push references blob … which has not been uploaded*). The message stayed
  shown with its caption, its chip *refused* with the upload's reason (*huge.zip is larger than
  the 25.0 MB the agents server takes from one message, so the server refused it.*), and a new
  session that device opened next reached the phone. Shut down and started again: the same
  refusal, the same reason, and its next message reached the phone too.
- **Archived, then opened:** archived on the mac, the session's attachment rows left the phone's
  replica (0 rows); opened archived, its message came through `sessionPage` with both chips, the
  cache held nothing more for showing them, and the PNG — never opened on the phone before —
  opened, its bytes downloaded then.

Development fixtures: `?sessions=launch&attachments=chips` (alpha's `device.1`: a message whose
files are uploading, waiting, sent and refused, and a message refused whole for one file's upload,
its other file blocked), with `attachments=offline` or `attachments=refused` for what a new
attachment does.

**Proved on Linux:** keeper-core over real on-disk replicas — the bound before anything is staged,
one commit holding the message and its rows with canonical `blob_ref`s, the pin and one upload row
across a relaunch, opening only what a row names and only as a type that cannot run, the chips'
states and their order (the order the files were attached in: the rows' ids increase within the
one millisecond their commit shares), a refused commit's message and chips, every refusal code's
sentence, a refused upload's
file *refused* and its message's others *blocked* with no other file touched, before and after
the server answers the message, and a selection staged by path and by paste — or several pastes —
sent once as one message with its caption once, a handle never committed released and a Send
naming one committing nothing (`hq/attachments_tests.rs`); eviction order and the pin over the
replica file, the reported size, staging and its commit as one step that a round right after —
its `PUT` answered, its push cut — leaves pinned across a relaunch under cache pressure, a failed
commit taking back only what it staged, a commit cut short by the process dying after its first
upload and before its message leaving nothing to upload once reopened (a blob another waiting
message pins kept), a failed Send whose clean-up failed too uploading nothing of it — no round
while storage stays faulted, the same owner putting it back on its next wake once storage
recovers, the waiting message's file uploaded — without a reopen or another commit, a refused
upload resolved and explained across a relaunch while the next
rounds go past it, a moot refusal pruned after a round without a reopen (`hq/syncular.rs`,
`hq/blob_cache.rs`, against a local server that answers the blob requests and cuts the push); an
opened archived session's attachments window (`hq/sessions.rs`); the relay's HTTP framing, a
`100`/`103` before the final answer included (`tests/hq_live.rs`). **Inspection only:**
`hq_ipc.rs`'s four commands — `agent_attach_path`/`agent_attach_bytes` stage and return handles,
`agent_attach_commit` sends them as one message, `agent_attach_release` drops them;
`agent_attach_bytes` checks the borrowed body's length against the bound before copying it, with
a test of its own — `hq_blob_protocol.rs` and their registration in `lib.rs` (CI's macOS job
compiles them). **Owed:**
the chip and the composer's tray (the UI lane, UX-DR-E107-1), hesperia and kalypso (acceptance 1,
9 and the installed apps).

## Talking to an agent

Stories 108.2 and 108.3a (AD-384, AD-411; R-HQ-68). **Settings › Agents & voice › Voice** holds
the wake switch, the wake phrase, the stop phrase, the listening language and *Speak to*, under the
`voice.*` keys (`voice.wake_enabled`, `voice.wake_phrase`, `voice.stop_phrase`, `voice.locale`,
`voice.target`). A value an older build wrote under the matching `bots.*` key is read under its
`voice.*` key, and a value already under the new key wins (`keeper-core/src/registry.rs:203-211`, `:223-233`).
Recognition never leaves the device (`docs/egress.md`, *Voice adds no egress*).

**Speak to** lists every agent of every database this device holds
(`src-tauri/crates/keeper-core/src/hq/spoken.rs:119`) and is stored as `agent:<agent>`
(`:68`, `:94`). A stored value from before 108.2 named an agent's Matrix room
(`agent:!room…`). It is read as no choice, once, with a notice (`:110`; UX-DR-E108-1). With no
choice the turn is refused with *Nothing to talk to yet: choose an agent under Speak to.*
(`keeper-core/src/voice/target.rs:31`); nothing is sent to an agent nobody chose.

**Where a spoken question goes** (`hq/spoken.rs:11-19`, `:148`). It goes to the agent's main
conversation: its newest session that is not archived in any held database, the one launch and the
docks open. With none, a new session is made in the only database that names the agent, or in the
one the person chose for it. With several databases and no choice, nothing is written, and the
person is told to start the conversation in Agents, where the database is chosen. The words the
voice turn heard become a person's `messages` row there, written into the replica's outbox like a
typed message (`Hq::spoken_send`, `:579`). No audio and no partial transcript is written anywhere.

**The answer.** The agent's turn names the message it answers. Its text, as the viewer folds it
(*Live answers*, above), is spoken sentence by sentence as it streams; the agent's final row
completes it. An answer that has not started within 60 s, or that brings nothing new for 120 s,
or whose instance stopped without ending it, ends the voice turn with a sentence naming the agent
(`:71`, `:75`).

**Stop.** The stop phrase stops speech on this device; it does not cancel the agent's turn. Where
the person stopped is a person `messages` row whose metadata is `{"kind": "heard", …}` with the
contract's `heard_until` object (`Hq::spoken_heard`, `:638`). The instance holding the session's
claim copies it into `sessions.heard_until` and cuts the answer there in the next turn's context.
It never answers a heard row, and the Agents window draws none (`hq/sessions.rs:597`). A later
mark supersedes an earlier one, from whichever device.

The device keeps the mark per question, beside the answer it follows: where each sentence handed to
the synthesiser ends in the answer's text, and how many the synthesiser read to their end, as its
delegate reports them. A sentence it was cut off in does not count, so the agent is never told you
heard more than you did. When the answer is stopped — the stop phrase, the mic button, the hotkey,
the tray (reason `stop`), or a question asked over it (`barge_in`) — the mark is written first, and
your next question goes in once it is; one not written within 10 s is not waited for, and cuts the
turns after it (`HEARD_MARK_WAIT`, `hq/spoken.rs:81`). An answer heard to its end, or one a failure
ended, is marked nowhere. A choice under *Speak to* naming an agent no held database lists is
refused with *The agent chosen under Speak to is in none of the agents' databases this device
holds: choose again under Speak to.* (`:85-86`).

- **When you talk over an answer.** The answer pauses, mid-word, the moment you speak — at the
  voice activity model's onset (three frames, 96 ms) with the account's turn models, at the
  recogniser's first words without them — and what you said decides: "mhm", "yeah", "okay"
  (English) or "mhm", "tak", "no", "jasne" (Polish), the whole utterance and nothing else, resume
  it; with the turn models, wordless speech under 600 ms resumes it too, and an onset that comes to
  no words under 250 ms resumes it as a false onset. The stop phrase ends the answer. Anything else
  stops it and is your next question — "no, wait" in English is a question. The list is the one
  for the language recognition runs in; any other language has "mhm" and "aha". On the Mac this is
  the same as on the phone ([ios.md](ios.md) § *When you talk over an answer*); a capture without
  voice processing (an aggregate input, AD-213) hears keeper's own voice, so there the voice
  activity model does not listen while keeper speaks, and the button, the hotkey and the tray stop
  the answer.
- **When your question ends.** Without the account's turn models it ends 1.8 s after the
  recogniser's last words. With them (Silero VAD and Smart Turn v3.2 from `_models/`, on an
  Apple-silicon Mac and the iPhone) a finished sentence ends it about a quarter of a second after
  you stop, and a sentence the model judges unfinished waits for the 1.8 s pause
  ([ios.md](ios.md) § *How a spoken turn ends*). The audio goes from the tap to the models and
  nowhere else.
- **The wake phrase** is the one you set; an agent's name renames nothing.

The shell's `hq_ipc` runs the follow and tells the voice turn each step; no `voice*.rs` file
reaches the network, which `voice_on_device` checks (`keeper-core/tests/voice_on_device.rs`).
**Owed on devices:** the voice turn end to end on hesperia and kalypso, against keeper-hq with the
heard runtime deployed (R-HQ-65).

### Measured: a spoken turn's end

NFR-114's end-of-turn half, per device, over at least twenty spoken questions with the turn
models present — about half complete sentences, half trailing off — from the app log of the run
(debug mode on; `KEEPER_VOICE_LOG=<keeper.log> cargo test -p keeper-core --test voice_timings
measure_a_device_run -- --ignored --nocapture`): the p95 of `finish_recognition_ms −
speech_end_ms` over the turns the model ended (≤ 300 ms; `finish_recognition_ms` is when the
port ran `endAudio`, as it reported it), the p95 of `sent_ms − finish_recognition_ms`
(≤ 600 ms), and the pause turns sent less than 1.8 s after their speech end
(none). Each sent text is also compared by hand with what was said: no question may lose its last
word.

| device | OS | build | date | n | model turns | finish p95 | send p95 | early pauses | last words lost |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| hesperia | — | — | owed | — | — | — | — | — | — |
| kalypso | — | — | owed | — | — | — | — | — | — |

Both rows are owed: story 97.2 is done only when they are filled from real runs (97.2 #8).

### Measured: talking over an answer

NFR-114's backchannel half, per device, with the turn models present, over at least twenty
pauses — "mhm" and "yeah" (English), "no" and "tak" (Polish) during long answers, "stop" and
"no, wait" mid-answer, and ten minutes of an answer read with nobody speaking — from the app log
of the run (debug mode on; `KEEPER_VOICE_LOG=<keeper.log> cargo test -p keeper-core --test
voice_timings measure_a_pause_run -- --ignored --nocapture`): the p95 of `pause_ms − onset_ms`
(≤ 150 ms), of `resume_ms − speech_end_ms` over pauses resumed by their duration and of
`resume_ms − final_words_ms` over pauses resumed by their words (each ≤ 300 ms; `final_words_ms`
is when the deciding words reached the turn, `resume_ms` when the synthesiser had continued),
and the false onsets per ten minutes of reading aloud — more than one is reported to the
coordinator before 97.3 closes.

| device | OS | build | date | n | pause p95 | duration resume p95 | words resume p95 | false onsets / 10 min |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| hesperia | — | — | owed | — | — | — | — | — |
| kalypso | — | — | owed | — | — | — | — | — |

Both rows are owed: story 97.3 is done only when they are filled from real runs (97.3 #11).

## Old agent rooms

Story 108.2 (FR-863; UX-DR-E108-2). Before 108.2 an agent's conversation was a Matrix room, and the
person's accounts are still in those rooms. keeper hides them from every room list, and from
notifications, by their create type: `dev.keeper.agent.session` for a session room,
`dev.keeper.agent.control` for a principal's control room
(`src-tauri/crates/keeper-core/src/old_agent_rooms.rs:21-31`; `keeper-core/src/notify.rs:596`). Only the room's
type is looked at; no agent event is read or sent.

**Settings › Agents & voice › Old agent rooms** says how many such rooms the accounts are still in,
and **Leave old agent rooms** leaves all of them. Only the person's own membership changes
(`keeper-core/src/account.rs:4418`). The section is absent when there is no old room left. The agents' own Matrix
users, their devices and their rooms on the homeserver are the operator's to clean up
(DW-E108-8).

**At every launch,** keeper removes what an install upgraded from a version with the Matrix path
left behind (`src-tauri/crates/keeper-core/src/agents_cleanup.rs:1-26`, `:37-50`): the twelve
`keeper.db` tables only that path wrote, after the MCP servers' keychain tokens they name; each
agent copy's keychain items and its store under `<data_dir>/agents/`; and the setting
`agents.claims_lease`. Each step is idempotent and logs a WARN line if it fails. Nothing touches a
drive. The ⌘9 bots' tables are dropped once, the first time a 108.3a build opens `keeper.db`
(`keeper-core/src/registry.rs:184`, `:188-201`), all but `bot_providers`, which notes search keeps; stored bot
tasks leave `sync.db`, each named once in a WARN line, and Tasks shows one notice
(`keeper-sync/src/db.rs:4858-4870`; UX-DR-E108-3). The phone's Matrix pusher is removed by its
own path for one more release (DW-E108-1). **Owed:** the messenger smoke and a live *Leave old
agent rooms* against Synapse on delectra, and the upgrade from 0.10.x on hesperia and kalypso.

## Imported history

Story 108.1 (FR-861, FR-862; AD-417, AD-433). The conversations agents had while they lived in the
drives were imported into the databases once, by keeper-hq's import tool, under an operator-only
import actor whose credential opened one database alone. Each drive went into its own database.
The run was on electra on 2026-10-23, and a second run wrote nothing:

| drive → database | sessions | rows |
| --- | --- | --- |
| tgdrive → `tgdrive` | 8 (6 Nixi, 2 Dr Tola Grey) | 303 |
| neuradrive → `neuradrive` | 0 (no agent session in its sessions zone) | 0 |

**In keeper,** an imported session is an archived session like any other. It is listed under
*Archived* and *All* with its database and opens through the server's `sessionPage` (*The Agents
window*, above). No instance reads or claims an archived session, so nothing imported is answered
again. Each message names its author by actor id. The Matrix id an author had in the logs is kept
on the server only, as an alias of that actor; it is not part of keeper's contract.

**Its limits:**

- **No reasoning.** Imported sessions carry no thinking: the *Thinking* view mode has nothing to show
  there.
- An answer carries the model, usage, time to first token and duration where the log recorded
  them, and nothing invented where it did not.
- Log records with no table were not written: claims, labels, scopes, delegations, asks, memory,
  `run` and review records.
- Each drive's history is in its own database only. A device that does not hold that database does
  not see it.
- The drives were only read. Their session files and their agents' homes stay untouched, and an
  agent's drive memory was not imported (DW-E108-4).
- **Order.** Three different limits (keeper-hq `agent/crates/hq-import/src/plan.rs` at
  60b0da0):
  - a message's attachments keep the order they were attached in, whoever wrote the log
    (their ids carry the attachment's index);
  - rows written in the same millisecond keep the log's order only when one writer, in one run,
    wrote them; across writers or a restarted writer that order is not promised (DW-E108-10);
  - imported approvals whose timestamps tie have no promised order at all, whoever wrote them:
    each approval's id is derived from its own record (DW-E108-11).

**Owed:** the imported sessions of both databases in the Agents window on hesperia (R-HQ-65), and
NFR-127 there (below).

## A session's model and effort (109.17, 109.18)

keeper-core reads `session_settings` with the saved nullable model and effort, the union of the
session agent's published model/effort choices, and each instance's defaults separately
(`AgentSessionSettingsVm`, `AgentSettingsDefaultVm`). Another agent's profiles never supply
choices. No shared default is invented when instances disagree.

`Hq::change_session_settings` writes one `AgentSettingsChange` for a person member (`owner` or
`person`): a model (the saved effort kept), an effort (the saved model kept), or `reset`, which
writes **null for both**. A saved half the agent no longer offers is written back as null rather
than refusing the other change, because the runtime already answers with the default for it.
Off-list choices and nonmembers are refused locally with the offered choices named; the server
remains authoritative. Settings coverage is held for archived sessions too. Until that coverage
is complete, reading or writing returns `HqError::Arriving` rather than mistaking missing rows for
defaults.

Choices affect subsequent turns, not a turn already running. An answer records the model
actually used and its effort; when a selected model is no longer offered, it falls back to the
definition default and records why in `metadata.model_fallback`.

**The pickers (109.18).** The session view carries `AgentSessionViewVm.settings`
(`AgentSessionPickerVm`), absent for an archived session, while the settings arrive, and for a
reader who may not change them. Rust decides every part of it:

- `models` lists only the offered models, each `current` (the saved choice, or the instances'
  defaults while no offered choice is saved) and `default`; a saved model the agent dropped is
  never listed.
- `efforts` lists only the offered efforts with the same flags. Nothing offered — Nixi as
  shipped, whose effort is withheld — leaves `efforts` empty and `effortLabel` absent: the effort
  menu and `/effort` are **absent**, not disabled.
- `resettable`: a choice is saved, so Reset has something to write back to null.
- `running`: a turn streamed and has no answer yet; a change then answers "This answer keeps its
  model and effort; your change applies from the next answer." (also the return of the change).
- `fallback`: the newest answer's `metadata.model_fallback`, shown beside the picker.
- `write`: the row's per-row outcome (`Replica::row_writes`, as the instance choice reads it):
  `pending`, or `refused` with the server's V16 code in plain words. A new change supersedes a
  standing refusal.

The composer footer (`session-settings-picker.tsx`) is t3code's shape: a model popover with a
search box, starred models first and a "Default" badge; an effort menu with a "Default" badge;
Reset. An agent with nothing to choose draws no group at all. Stars are a lens, not a setting:
they live in the `keeper_model_favorites` cookie like the other remembered views (no settings
row, no IPC write), device-wide, and only order Rust's offers — a starred model the agent does
not offer is never listed. Enter and Space on a star star it; they never reach the list, which
would take Enter as choosing the highlighted model.

`/model <id>` and `/effort <level>` (109.19's registry) run the same change and appear only when
the pickers would offer the matching choice. `/effort` lists only the agent's offered levels, in
the menu's words, and takes either the menu's word (`extra high`) or the contract's (`xhigh`); a
level the agent does not offer is refused with the offered ones named.

**The last pick for new sessions (Q109-16 b, ruled per device).** Every change answers what it
wrote (`AgentSettingsChangedVm.pick`; `/model` and `/effort` answer it in
`AgentCommandResultVm.pick`), and this device keeps it per agent in the `keeper_agent_picks`
cookie (`src/lib/stores/agent-picks.ts`); a reset forgets the agent. When this device opens a new
session with that agent — the New session composer, `/new` or `/clean` — it hands the pick to
Rust, which writes the halves the agent still offers (`settings::initial`). A dropped half is left
to the agent's default; nothing offered, or no pick, writes no settings row. Nothing new is drawn.

A creation with a pick to write is three commits, in order: the session and its membership; the
settings row; the first message (`sessions::create_with_first`, R10918-07). Without one it stays a
single commit, as before. V16 judges the pick
against the server's offers, which this device may not hold yet (a session created offline, or a
replica behind a republished definition), and a refusal rolls back only its own commit: the
conversation and the person's first words stand, the turn runs on the agent's defaults, and the
refusal shows on the new session's picker. The server applies a client's commits in order and the
runtime starts no turn before its subscription to the session's settings has caught up
(keeper-hq `worker.rs`, `start_turn`), so an accepted pick reaches the first turn.
A spoken question (`hq::spoken`) opens its session in Rust alone, so it starts with the agent's
defaults.

Mock shell: `?sessions=launch&settings=nixi|refused|fallback` (with `&live=stream` for a running
turn).

**Owed:** the pickers on kalypso (iOS).

## Measured

The agents' three latency targets (keeper-hq `ARCHITECTURE-KEEPER-HQ.md`, the NFR table) and what
was measured against each. None of them has been measured on the production path yet — the
electra server with keeper's own client on hesperia over the tailnet. Every figure below says
where it came from.

**NFR-123 — a person's message reaches the agent at once** (p95 ≤ 100 ms on the tailnet, electra
server, Mac client).

- 103.1's spike, 2026-10-20: server on delectra, the person a Linux client over the tailnet,
  1,000 commits per database at 20 per second. From the person's commit to the agent's replica
  applying it, p95 **14.98 ms** on `tgdrive` and **15.47 ms** on `neuradrive` (keeper-hq
  `docs/measured.md` § 103.1).
- 104.2, 2026-10-20: the real agent on delectra, everything on one host. From the message's
  `created_at_ms` to the agent starting its turn, 30 messages, p95 **20 ms**, and 20 ms again
  after review (`docs/measured.md` § 104.2).
- **Not measured** with a Mac client against electra. That is keeper-hq's D9 and the 106 final
  client check, both owed.

**NFR-124 — the answer is live** (p95 ≤ 100 ms on the tailnet from the agent publishing a tail to a
viewer receiving it).

- 103.1's spike: 7,200 publishes per database at 40 tokens per second. Publish to the viewer over
  the tailnet showing it, p95 **2.75 ms** on `tgdrive` and **2.77 ms** on `neuradrive`.
- 104.2: from the model stub emitting a token to a reader's presence showing it, 600 tokens on one
  host, p95 **20.15 ms**, and **20.59 ms** after review. This is longer than NFR-124's leg, since it
  includes the agent's parsing and its turn loop.
- 106.3, keeper-core's own viewer (*Measured: a live answer against the real server*, above): p95
  21.5 ms on one host over loopback, not NFR-124's figure.
- **Not measured** from electra to hesperia (D9, owed). A loopback probe or a viewer on the
  server's own host is not the production path.

**NFR-127 — an archived session opens quickly** (newest page of a 1,000-message archived session
shown ≤ 1 s on the tailnet).

- 2026-10-20, `hq_live.rs`
  `agents_window_archive_and_an_archived_session_opens_within_a_second`
  (`src-tauri/crates/keeper-core/tests/hq_live.rs:1212`, `:1363-1396`): the keeper-hq server on
  delectra (Bun 1.2.23, both databases, a throwaway Postgres 18), keeper-core's client in a Linux
  container reaching it over the tailnet. One device wrote a session of 1,000 messages into
  `tgdrive`; a second device listed it, held its 1,000 messages, archived it (none left in its
  replica, the row still listed), then opened it archived 20 times. The newest page of 50,
  through the registered `sessionPage` query, reached keeper-core's complete session view in
  **min 11.8 ms, median 15.6 ms, p95 17.3 ms, max 21.2 ms**. The timer stops at
  `session_view().complete`, before Tauri IPC and rendering. Reading back fetched the next 50,
  and unarchiving restored the 1,000 rows.
- The 1,011 ms page read in keeper-hq's import rehearsal is the probe's one-second round, not an
  NFR-127 figure (keeper-hq `docs/e2e.md`).
- **Not measured:** the native gate, from opening an archived session in the installed app on
  hesperia to its newest page being painted, against electra — for an imported session too.

**On devices.** Each row is owed until a run on the device fills it in, with the device, its OS,
the build and the date:

| run | story | device | figure | result |
| --- | --- | --- | --- | --- |
| the tablet's first device run | 98.3 #7 | the Android tablet | the run's checks (`docs/android.md`) | owed |
| push latency through ntfy, send to notification, twenty pushes | 98.4 #10 | the Android tablet | p95 | owed |
| a spoken question and its answer, talking over it, echo cancelling | 98.4 #10 | the Android tablet | the run's facts (`docs/android.md` § *Voice*) | owed |
| push latency through Sygnal, send to banner, twenty notifications | 98.1 #12 | kalypso | p95, and the extension's peak memory under 24 MB | owed |
