# The agent-conversation contract

keeper keeps an agent's conversations — sessions, messages, answers as they stream, reasoning,
tool calls, approvals, claims and attachments — as rows of a [syncular](https://syncular.dev)
database that the person runs, replicated into a local SQLite on every device and every agent
instance that subscribes to it (D-37). This directory is the contract between those replicas and
the server: the tables, their scopes, the ids, and the rules a server enforces. keeper is a client
of it; any server that serves this schema and enforces these rules is a server for it, and one
server may serve several databases from it, each its own partition with its own directory (D-4:
keeper ships no server URL).

| file | what it is |
| --- | --- |
| `syncular.json` | the manifest: the tables in bootstrap order, their scope patterns, the subscription templates clients request |
| `migrations/NNNN_name/up.sql` | the column shapes, primary keys, indexes and declared references; append-only |
| `queries/*.syql` | the named queries: `sessionPage` and the reads a client's windows answer |
| `syncular.migrations.lock.json` | the locked migration history (generated; never edited by hand) |
| `syncular.ir.json` | the neutral schema IR a native client loads at runtime (generated) |
| `syncular.generated.ts` | `schema`, a `ServerSchema`, and the row types (generated) |
| `syncular.queries.ir.json`, `syncular.queries.ts` | the analysed queries and their descriptors, with the relation plans a server registers (generated) |
| `limits.json` | the bounds both sides read: `attachmentMaxBytes`, the most bytes one attachment's blob may have (26,214,400); hand-written, never generated |
| `vocabulary.json` | the closed word sets both sides read: `efforts` (the reasoning efforts a session may name), `grantTiers` (the tiers a standing grant may cover), `handle` (the word lists a session's handle is made of, with examples) and `personAgentKey` (examples of the person-agent key); hand-written, never generated |

Everything generated comes from `@syncular/typegen` 0.32.1 (Apache-2.0), pinned in keeper's
`package.json`, with `naming: "preserve"`: every surface uses the SQL column names.

```sh
bun run gen:syncular          # regenerate after changing the manifest, a migration or a query
bun run check:syncular        # part of `bun run check`: fails on an invalid schema or stale output
bun run check:syncular:live   # against a real Postgres, below
```

## Changing the contract

A migration that has been deployed is immutable: a change is a new `NNNN_name/up.sql` and a new
`schemaVersions` entry. Tables are added; columns are appended and nullable. A server upgrades
first and serves the previous version beside the new one through syncular's schema window; a
client rebootstraps on its own schema bump. Removing or renaming a column, changing a type, a
scope or a primary key is a new table, not an edit.

## Scopes

A scope variable is one dimension a row is authorised along; a client requests values, the server
grants the values its actor may hold, and a row reaches a client only when every variable the
client requested matches. No variable is ever granted as `'*'` on these tables: a subscription may
request a single variable alone, so a wildcard would be a public read of every table declaring it.

| variable | value | granted to a person | granted to an agent instance | tables |
| --- | --- | --- | --- | --- |
| `dir` | the name of the database the server serves | when the person belongs to it | when the instance subscribes to it | `actors`, `clients`, `agent_profiles` |
| `session` | a session id | the sessions the actor is a member of, and the next 16 unused ids of each of the actor's clients | the same | `sessions`, `session_members`, `claims`, `session_tool_switches`, `session_settings`, `session_drives`, `turn_chunks`, `attachments` |
| `member` | an actor id | itself | itself | `session_members` |
| `page` | a page key | for each granted session, the months from its creation to now | the same | `messages` |
| `trace` | a page key | as `page`, only for sessions where the person's membership has `reads_trace` | as `page`, for the sessions the agent is a member of | `reasoning`, `tool_calls`, `approvals` |
| `actor` | an actor id | itself | itself | `instance_prefs` |
| `agent` | an agent's actor id | the agents of this database that serve the person | itself | `agent_memory` |
| `family` | a family name | the families of this database that serve the person | its own family | `agent_memory` |
| `person_agent` | a person-agent key (*Ids*) | the key of (itself, agent) for each agent of this database that serves the person | the key of (person, itself) for each person of this database it serves | `standing_grants` |

A member who is not a reader receives a session's messages and its streamed text, and never its
reasoning, tool calls (with their arguments and results) or approvals: those live only under
`trace`. `turn_chunks` carry text only.

A `person_agent` value names one person and one agent, and no other pair has it, so a person
reads only the standing grants they wrote, and an agent instance only those written about its own
agent: neither reads a grant another person wrote about the same agent, or one the same person
wrote about another agent.

## Ids

Every id a client creates can be minted offline:

- a session id is `<clientId>.<n>`, `n` a decimal integer the client counts up; the server grants
  each client its next 16 unused ids, so a device creates a session and its first message in one
  commit without a round trip;
- a row of `messages`, `turn_chunks`, `reasoning`, `tool_calls`, `approvals`, `attachments`,
  `agent_memory` or `standing_grants`, and a turn id, is a ULID;
- a row there is at most one of is keyed by what it belongs to: `session_members.id` is
  `<session_id>/<actor_id>`, `instance_prefs.id` is `<person_actor>/<agent_actor>`, `claims`,
  `session_tool_switches`, `session_settings` and `session_drives` are keyed by `session_id`,
  `agent_profiles.id` is the instance's client id;
- `actors` and `clients` are written by the server's own service client, never by a device;
- an actor id never contains `/` or `:`, so `<session_id>/<actor_id>` and every key below split
  one way only; a server refuses a directory naming such an actor;
- a **person-agent key** (`standing_grants.person_agent_key`, the `person_agent` scope) is
  `<n>:<person_actor>/<agent_actor>`, `n` the number of UTF-8 bytes of `person_actor` in decimal
  without leading zeros: `6:tgorka/nixi`. The length decides where the person ends, so two
  different (person, agent) pairs never share a key, whatever the ids hold; `vocabulary.json`'s
  `personAgentKey.examples` holds keys every implementation must reproduce.

## Page keys

A row of `messages`, `reasoning`, `tool_calls` or `approvals` carries `page_key` =
`<session_id>/<yyyy-mm>`, the UTC month of its `created_at_ms`. A long session is therefore
windowed by month: a client subscribes its newest month or months and widens month by month,
and an archived session is not subscribed at all. The month lives inside the value, never as a
variable of its own, because a variable may be requested alone and a month variable would read
across sessions.

The server refuses any other value: a `page_key` whose session is not the row's `session_id`, or
whose month is not the month of the row's `created_at_ms`, or lies before the session's creation
month or after the server's current month. It refuses any change to `page_key` too (syncular
refuses a changed scope column on its own, SPEC §3.4), and to `created_at_ms`.

## `sessionPage`

`sessionPage(pageKey, beforeMs, beforeId, pageSize)` returns one month of one session's messages
older than the cursor `(created_at_ms, id)`, newest first, at most `pageSize` rows and never more
than 10,000; the first page passes `(9007199254740991, '')`, each next page the last row's
`(created_at_ms, id)`. It is the read-only path for an archived session: a server registers it as
a scoped remote query with `maxRows` 10,000, which checks that the caller holds the page key under
`page` before it runs; a client that holds the month reads its replica with the same query. Its
SQL is the subset SQLite and Postgres both run.

The other queries are the reads a client's windows answer: `pageReasoning`, `pageToolCalls`,
`pageApprovals` (one month of trace rows), `turnChunks` (the committed text of a turn still
streaming), `sessionAttachments` and `memberSessions` (the session list of one actor).

## Tables

Every row carries `author_actor` and `author_client`: the actor and the client of the commit that
last wrote it. Times are UTC milliseconds since the epoch.

- **`actors`** — a person or an agent: `kind` (`person` | `agent`), `display_name`, `family` (an
  agent's family; an agent outside a family is a family of one), `dir`.
- **`clients`** — a device or an agent instance: `actor_id`, `kind` (`device` | `instance`), `name`,
  `host` (the machine an instance runs on; the one a device named when it paired, or null),
  `created_at_ms`, `revoked_at_ms`, `dir`. No credential, token hash or push target: those stay in
  the server's own tables, outside every scope, because a synced row reaches every client of its
  scope.
- **`agent_profiles`** — what an agent instance says about itself: `actor_id`, `name`,
  `description`, `avatar_blob`, `definition_hash`, `image_digest`, `started_at_ms`, `dir`, `tools`
  (schema 3: a JSON array of the tool names the instance offers — its definition's tools less
  those its container withholds — written when it starts; absent until an instance says, and a
  client then offers no tool switch), and from schema 4 what a person may pick for a session of
  it: `models` (a JSON array of model ids, its definition's ceiling) and `default_model` (the one
  it uses when nobody picked), `efforts` (a JSON array of the reasoning efforts it can send, each
  one of `vocabulary.json`'s `efforts`) and `default_effort` (the one it sends when nobody picked;
  absent when it sends none), and `drives` (a JSON array of the drive names it can read: its
  definition's ceiling less what its container does not mount). Each is absent until an instance
  says, and a client then offers no picker for it.
- **`sessions`** — `title`, `status`, `agent_actor`, `created_by_actor`, `created_by_client`,
  `preferred_client` (the instance the person asked for), `parent_session` (a delegated child's
  parent), `invite` (a JSON array of actor ids), `archived`, `heard_until` (the claim holder's mark
  of where the person stopped hearing a spoken answer: a JSON object naming the message, the
  character offset, the sentence and the reason), `created_at_ms`, `last_activity_ms`, `trust`
  (schema 2: absent or `trusted`, or `untrusted` once the session has read outside content — a web
  search, a fetched page, an MCP server's answer — D-34; it never goes back).
- **`session_members`** — `session_id`, `actor_id`, `role` (`owner` | `person` | `agent`),
  `reads_trace`, `added_by_actor`, `added_at_ms`, `read_until` (the `created_at_ms` of the newest
  message this member has read).
- **`claims`** — the one row naming the instance that answers a session: `holder_actor`,
  `holder_client`, `epoch`, `renewed_at_ms`, `released`.
- **`session_tool_switches`** — one row per session: `tools_off`, a JSON array of the tool names a
  person switched off for it, and `updated_at_ms`. There is no column that switches a tool on.
- **`session_settings`** (schema 4) — one row per session: `model` and `effort`, what a person
  picked for the session's next turns, each absent for the agent's default, and `updated_at_ms`.
  The pick holds from the next turn until it is changed; a model the agent no longer offers falls
  back to its default, and the answer's `metadata` says so.
- **`session_drives`** (schema 4) — one row per session: `drives`, a JSON array of the drive names
  a person ticked for the session's agent to read, and `updated_at_ms`. No row, or an empty array,
  is no drive: none is ticked until a person ticks it. It can only name drives the agent offers
  (`agent_profiles.drives`), and it grants reading, never writing.
- **`turn_chunks`** — the committed text of a streaming answer: `session_id`, `turn_id`, `seq`,
  `text`; deleted when the turn's message is written.
- **`attachments`** — `session_id`, `message_id`, `blob_ref` (a syncular blob: canonical BlobRef JSON
  `{blobId, byteLength, mediaType?, name?}` in that key order), `name`, `mime`, `size` (bytes, at
  most `attachmentMaxBytes` from `limits.json`), `created_at_ms`. A client uploads the blob
  (`PUT /<database>/blobs/<blobId>`) before it commits the row, and commits the message and its
  `attachments` rows in one commit.
- **`messages`** — `session_id`, `page_key`, `turn_id` (an agent's answer), `role` (`person` |
  `agent`), `body`, `body_blob` (a body too large to ride inline), `metadata` (JSON: model,
  usage, `ttft_ms`, `duration_ms`, finish reason). An agent's question to the person is an agent
  message whose `metadata` holds `"kind": "ask"` and, when it offers answers, `"options"` (an
  array of strings); the person answers with an ordinary person message, and the ask is open until
  one is written after it.
- **`reasoning`** — `session_id`, `page_key`, `turn_id`, `seq`, `compacted` (true on the one row
  that replaces a turn's chunks after compaction), `text`.
- **`tool_calls`** — `session_id`, `page_key`, `turn_id`, `name`, `args` (JSON), `status`,
  `result`, `result_blob`, `started_at_ms`, `ended_at_ms`.
- **`approvals`** — `session_id`, `page_key`, `tool_call_id`, `tier`, `binding_digest`, `summary`,
  `approvers` (a JSON array of actor ids), `state` (`pending` | `approved` | `denied` | `consumed`
  | `expired`), `decided_by_actor`, `decided_by_client`, `consumed_by_client`, `expires_at_ms`,
  `grant_id` (schema 4: the standing grant that decided an approval born `approved`; absent on
  every approval a person decides).
- **`instance_prefs`** — one person's choice of instance for one agent: `person_actor`,
  `agent_actor`, `default_client`, `fallback` (a JSON array of client ids, in order),
  `updated_at_ms`.
- **`agent_memory`** — `agent_actor`, `family`, `kind` (`user` | `memory` | `journal` | `proposal`
  | `family`), `text`, `source_session`, `created_at_ms`. Every row carries both scope values. A
  client requests the two independently, because a subscription matches every variable it
  requests: `agentMemoryOwn` (`agent` alone) receives one agent's rows, and `agentMemoryFamily`
  (`family` alone) receives every row of a family, its peers' included. A subscription requesting
  both would receive only the rows of that agent in that family.
- **`standing_grants`** (schema 4) — one person's "always allow" for one agent:
  `person_agent_key` (the person-agent key of the two, *Ids*), `person_actor`, `agent_actor`,
  `session_id` (absent: a standing grant, for every session of this database the two share; set:
  "This session only", the grant applies in that one session and nowhere else), `tool` (a tool
  name as `tool_calls.name` records it), `args_match` (the argument scope, below), `max_tier` (the
  highest tier it covers, one of `vocabulary.json`'s `grantTiers`), `expires_at_ms` (absent: until
  revoked), `revoked_at_ms`, `granted_by_client` (the device it was granted from), `created_at_ms`.
  It stands in for that person's decision on a later approval of the same agent, tool and
  argument scope, in a session of this database, under V8 below; every use is still its own
  approval row, with its own digest, consumed once. `args_match` is a JSON object: for `run`,
  `{"program": …}` with the program as the call landed it (its `argv[0]`), and optionally
  `"arg"`, its first argument; for every other tool, `{}`. A grant never covers a call that
  writes a drive (*V15*).

Declared references, enforced by the server at commit: `clients` and `agent_profiles` need their
actor; `claims`, `session_tool_switches`, `session_settings`, `session_drives`, `turn_chunks` and
`attachments` need their session and go with it; an approval needs its tool call.

## Session handles

A session's handle is a name a person can say, such as `noble-hedgehog-struts`: three words drawn
from `vocabulary.json`'s `handle` lists by the SHA-256 of the session id's UTF-8 bytes. Bytes 0–1,
2–3 and 4–5 of the digest, each read as a big-endian unsigned 16-bit number modulo its list's
length, pick the adjective, the noun and the verb, joined by `-`. `handle.examples` holds session
ids with the handle each must give, so every implementation can check itself. A handle is display
only: it is a pure function of the id, never a key, a scope value or a lookup path, and two
sessions may share one, which a client tells apart by the session number.

## The live answer

An answer being written reaches its viewers by two paths that keep no order between them. The
instance holding the claim commits its text as `turn_chunks` — seq 0 an empty anchor written
before the turn starts, then each committed piece under the next seq — and publishes what it has
not committed yet as presence: `{turn, from_seq, text, tool}` on `session:{session_id}` (`tool`
the name of the tool being called, or null) and, for readers only, `{turn, from_seq, reasoning}`
on `trace:{page_key}` of the month the turn opened in. A tail's `from_seq` is the seq its text will
be committed under. The reasoning commits as `reasoning` rows from seq 1, compacted at the end into
one row (`compacted`, seq 0) holding all of it. The turn ends in one commit: the agent's `messages`
row (`turn_id` the turn), every `turn_chunks` row of the turn deleted; the documents are cleared
after.

**The viewer rule.** A viewer renders a turn that is still being written as the turn's
`turn_chunks` contiguous from seq 0, followed by the latest `text` it has seen on
`session:{session_id}` whose `from_seq` is the seq after the last of them; it keeps the latest tail
for each `from_seq` — through a presence leave — until that seq's chunk arrives, and replaces both
with the agent's `messages` row for the turn (`turn_id`) once it holds it. The reasoning follows
the same rule from seq 1, and the compacted row replaces it. A tail that runs ahead of the chunks is
buffered until they arrive, so what a viewer shows only grows, and is always a prefix of the
answer.

## What the server enforces

A row validator sees the commit's actor; the whole-commit validator also sees its client and
every operation of the commit. These rules hold against a client that does not follow them. A
validator is the server's own code and appears in no generated file, so this list is the
contract. "The service client" is the server writing on its own behalf; "the registry" is the
server's own list of people, agents and the databases of each, and of who may read each drive. A
rule broken refuses the whole commit.

1. **V1 — authorship.** `author_actor` is the commit's actor and `author_client` is the commit's
   client, on every row written. The server also refuses a commit whose client is not the client
   the caller's credential was issued to.
2. **V2 — the directory.** `actors` and `clients` are written only by the service client. An
   `agent_profiles` row is written only by the instance it describes: its `id` is the commit's
   client and its `actor_id` the commit's actor. Its `models`, `efforts` and `drives` are each
   absent or a JSON array of distinct non-empty strings, every effort one of `vocabulary.json`'s
   `efforts`; `default_model` is absent or one of `models`, and `default_effort` absent or one of
   `efforts`.
3. **V3 — sessions.** A new session's id starts with the commit's client id followed by `.`.
   `agent_actor`, `created_by_actor`, `created_by_client` and `parent_session` never change. A
   session is inserted only by a person, or by an instance of an agent the registry lets start work;
   an instance of any other agent inserts only a child session whose `parent_session` is a session
   of this database the agent is a member of. `invite` is changed only by the session's owner (the
   person whose `session_members` row has role `owner`), compared as a set of actor ids. An invited
   member or an agent instance that changes it is refused (`hq.v3.owner_only`). An instance that
   inserts a delegated child may invite only people who are members of its parent session, and a
   session an instance opens at the top level invites no one.
4. **V4 — membership.** A client inserts only its own actor's `session_members` row, and only in
   the commit that inserts the session. Every other membership row is written by the service
   client: the agent the session names, and each actor that an owner's change of `invite` newly
   names. Someone who left or was removed comes back only when the owner invites them again;
   staying in an `invite` they were already in does not bring them back. Another member's row is
   deleted only by the owner, through `remove_member`. A member leaves by deleting its own row, and
   the server then takes it out of `invite`; the owner's own row is not deleted
   (`hq.v3.owner_cannot_leave`). `reads_trace` is turned on for a person only by the
   session's owner. A member may change only its own `read_until`.
5. **V5 — page keys.** As *Page keys* above.
6. **V6 — one writer per answer.** `turn_chunks`, `reasoning`, `tool_calls`, agent `messages`, new
   `approvals` and `sessions.heard_until` are written only by the client holding the session's
   claim.
7. **V7 — claims.** `holder_actor` is the session's agent. A change of holder increases `epoch` by
   exactly one, and an update carries the version it read, so a second taker conflicts instead of
   overwriting.
8. **V8 — approvals.** `pending` becomes `approved` or `denied` only by an actor named in
   `approvers`, never by an agent, with `decided_by_actor` and `decided_by_client` the commit's,
   and only by a person or owner member of the session whose `reads_trace` holds in the commit's
   candidate state — a removed or withdrawn reader is refused `hq.v8.not_reader`;
   `approved` becomes `consumed` only by the claim holder, with `consumed_by_client` the commit's;
   a decision on a tier-4 approval comes only from a client whose host is not the claim holder's
   host. A consumption carries the version it read, so a second consumer conflicts.
   A new approval is `pending`, with no decision and no `grant_id`, unless it is born `approved`
   by a standing grant: then its `grant_id` names a `standing_grants` row of this database which,
   in the commit's candidate state, is not revoked and not expired by the server's clock; whose
   `agent_actor` is the session's agent; whose `person_actor` is named in `approvers` and is a
   person or owner member of the session whose `reads_trace` holds; whose `session_id` is absent
   or is the approval's session, which is then open and not archived; whose `tool` is the
   approval's tool call's `name` and whose `args_match` holds of that call's `args`; and whose
   `max_tier` is not below the approval's `tier`. The tier is one of `grantTiers`, never `T4`, the
   session is not `untrusted`, and the call writes no drive: a grant never decides a drive write,
   whatever its tier, the session's trust or the spelling of its tool's name. Its `decided_by_actor` is the grant's `person_actor`, its `decided_by_client`
   the grant's `granted_by_client`, and it is consumed as any approved row is. `grant_id` never
   changes.
9. **V9 — preferences.** `instance_prefs.person_actor` is the commit's actor.
10. **V10 — memory.** An agent instance writes only rows whose `agent_actor` is its actor and
    whose `family` is its family, and only `journal`, `proposal` and `family` entries; `user` and
    `memory` entries change only by a person or by the server's consolidation; an agent's `user`
    entries hold at most 1,375 characters together and its `memory` entries at most 2,200, counted
    as Unicode scalar values plus a three-character separator between each two entries.
11. **V11 — one database.** Every actor a session names — `agent_actor`, each member's `actor_id`,
    each `invite` entry — is an actor of this database's directory, and a `parent_session` is a
    session of this database. Nothing in one database names an actor or a row of another.
12. **V12 — tool switches.** `session_tool_switches` is written only by a person who is a member of
    the session (role `owner` or `person`); its session is a session of this database and never
    changes. The row can only name tools switched off, so no write can offer a tool the agent's
    definition or its container withholds.
13. **V13 — trust.** `sessions.trust` is absent, `trusted` or `untrusted`, and only rises: a write
    that lowers it is refused. It is raised only by the client holding the session's claim or by a
    person who is a member of the session.
14. V14 is not a client's rule: a server's own import of older records uses the number.
15. **V15 — standing grants.** `standing_grants` is written only by a person, never by an agent:
    `person_actor` is the commit's actor, `agent_actor` an agent of this database that serves
    them, and `person_agent_key` the person-agent key of the two. A grant is inserted with
    `granted_by_client` the commit's client, no `revoked_at_ms`, a `created_at_ms` the server's
    clock accepts, an `expires_at_ms` absent or later, a `max_tier` among `grantTiers`, and an
    `args_match` of the tool's shape (*Tables* above). Its `tool` is one the server knows writes no
    drive — an allow-list of tools, never a refusal of names — and never `delegate`: a grant never
    covers a drive write, whatever its tier, the session's trust or how its tool's name is
    spelled. A `session_id` names a session of this database, open and not archived, whose
    `agent_actor` is the grant's agent and in which the grant's person is a person or owner member.
    After that the one change is its revocation: its person sets `revoked_at_ms`, once, from any of
    their devices, and nothing else moves — `session_id` never changes. A session-bound grant ends
    with its session: when the session is archived or deleted the server revokes it
    (`revoked_at_ms`), so a session archived and brought back does not bring the grant back. A grant is never deleted, so every approval that names one
    can still be read beside it. No write widens a grant: a wider one is a new grant.
16. **V16 — session settings.** `session_settings` is written only by a person who is a member of
    the session (role `owner` or `person`), never by an agent; its session is a session of this
    database and never changes. `model` is absent or one of the `models` an `agent_profiles` row
    of the session's agent offers; `effort` is absent or one of `vocabulary.json`'s `efforts` and
    one of the `efforts` such a row offers.
17. **V17 — session drives.** `session_drives` is written only by a person who is a member of the
    session (role `owner` or `person`), never by an agent; its session is a session of this
    database and never changes. `drives` is a JSON array of distinct drive names, each one an
    `agent_profiles` row of the session's agent offers, and each readable by every person of this
    database: a drive is ticked in a database only when everyone who reads that database may read
    the drive.

A server that does not enforce one of these rules yet refuses every write the rule governs: a
table it does not serve under its rule is written by nobody, and an approval naming a grant is
refused while grants are.

An attachment's `size` is at most `limits.json`'s `attachmentMaxBytes` (26,214,400) bytes, and so is
its blob; its message is in the same session. A client refuses a larger file before it uploads
anything, and the server refuses it again: the blob at the first byte past the bound
(`blob.too_large`), the row by its `size` (`hq.attachment_size`).

## Commands

Registered commands go to `POST /<database>/operations` with `kind: "command"` (REMOTE §3).
`params` is a JSON object with camelCase keys. Each command belongs only to the session's owner, on
the database the session lives in, and lands as one ordinary commit authored by the calling client.
A refusal answers `kind: "error"` with `code: "operation.forbidden"`. Params that are not
`{ sessionId, actorId }` strings and an `expectedAddedAtMs` integer answer
`operation.invalid_request` for `remove_member`, and `operation.forbidden` for `grant_trace` and
`revoke_trace`.

Each command names the membership it acts on: `expectedAddedAtMs` is the member's `added_at_ms`
as the caller shows it, which the server stamps later for every new membership of a person in a
session than for any before it, so no two memberships share one. Inside the command's
transaction, a member whose membership now has another `added_at_ms` (removed and invited again
since) is refused with `message: "hq.member.changed"`, and so is a member with none for
`grant_trace` and `revoke_trace`; nothing is written. A command never lands on a membership
nobody decided about.

- `grant_trace` `{ sessionId, actorId, expectedAddedAtMs }` — turns `reads_trace` on for a person
  member (D-34). The trace reaches their replicas after the scope refresh.
- `revoke_trace` `{ sessionId, actorId, expectedAddedAtMs }` — turns it off. Their replicas evict
  the trace.
- `remove_member` `{ sessionId, actorId, expectedAddedAtMs }` — takes a person member out of
  `invite` and deletes their `session_members` row in one commit. Their replicas drop the session
  and its rows, except their own writes still in the outbox. A refusal names why in `message`:
  `hq.remove_member.not_owner` (the caller is not the session's owner person on this database),
  `hq.remove_member.self` (the owner themself), `hq.remove_member.not_member` (the actor is not a
  member), `hq.remove_member.agent` (an agent member, including the session's own agent) or
  `hq.member.changed`. `grant_trace` and `revoke_trace` refuse without a reason, except
  `hq.member.changed`.

A command sent again under a `requestId` the server already applied is answered from its record,
before the member or the membership is looked at: `status: "cached"` with the first answer's
`results` and `commitSeq`, never a second commit. A client whose answer was lost — or arrived
unreadable, which says nothing about whether the command ran — asks again under the same
`requestId` and the same params, and treats `cached` as the end of that request: it ran before, and
the replica's next sync shows where things stand. It never follows a replayed answer with a
request of its own; a new decision is a person's new action, under a new `requestId`, about the
membership shown then. Only an answer it can read (applied, cached, rejected, or an `error`)
settles the request. keeper asks again under the same id only about the same membership: one whose
`added_at_ms`, `added_by_actor` and `role` (immutable, V4) are unchanged.

## The live check

`bun run check:syncular:live` runs a server's startup — `PostgresServerStorage.migrate()` and
`ensureSyncServerReady` over `syncular.generated.ts` — against a real Postgres, twice, for every
URL in `KEEPER_SYNC_TEST_PG_URL` (space-separated). Each URL logs in as a role whose `search_path`
names its own schema, which must be empty: a schema that already holds anything is refused. It
fails unless the first start puts every contract table in that schema and none in `public`, the
second start changes no table, column, index or schema marker, and the rows this run commits
through the push path — in a fresh partition, under fresh ids and commit ids, so no write is a
cached replay of an earlier run — come back page by page from a registered `sessionPage`, which
refuses a page key the actor does not hold; and unless an agent instance's `agentMemoryOwn` and
`agentMemoryFamily` subscriptions receive its own rows and its family peers' rows, and never
another family's. Point it at a fresh, throwaway database.
