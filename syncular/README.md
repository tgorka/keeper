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
| `knowledge-graph.vectors.json` | the knowledge graph's shared vectors (*The knowledge graph*): its fixed vocabulary, canonical values and their digests, typed references, tag canonicalisation, query symbols, client scopes, request digests and event-operation digests, each accepted or refused with a stable code; hand-written, never generated, and reproduced by every implementation (keeper's in `keeper-graph-protocol` and keeper-core tests) |

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

### Schema 5 release prerequisite

The server must adopt this contract, migrate `session_drives.tick_ids`, and serve schema 4
alongside schema 5 before a schema-5 keeper ships. The schema-4 projection omits the new column;
old clients continue to read and write `drives` and `updated_at_ms`.

For a schema-4 write, the server must also invalidate the stored `tick_ids` in the **same row
transaction**, including on an untick. An omitted column must not preserve an earlier identity
map. The client's `at == updated_at_ms` check rejects a stale map when the timestamp changes,
but timestamps are not grant identities: two writes can use the same millisecond, or an old
device's clock can repeat it. Clearing the map makes every legacy tick unacknowledged until a
schema-5 client explicitly acknowledges it. Contract adoption, version-window projection and
legacy-write invalidation need keeper-hq verification; client-only checks do not establish them.

### Schema 6 release prerequisite

Schema 6 adds the knowledge-graph tables and nothing else. The server adopts it, serves schemas 3
through 5 beside it with the graph tables omitted from their projections (and schema 4's
`tick_ids` invalidation unchanged), and starts with the graph disabled until it enforces the
graph rules below; only then may a schema-6 keeper ship. A server that has migrated to schema 6
is never rolled back to a binary that predates it.

### Schema 7 release prerequisite

Schema 7 adds `kg_contents` and two indexes (`kg_contents_owner`, `kg_edges_from_active`) and
nothing else; 0006 is unchanged. The server serves schemas 3 through 6 beside it with
`kg_contents` omitted. A keeper whose graph worker reads projection version 2 ships with or
before the server release that first accepts a content operation or a reserved `$` member: a
version-1 worker answers `unsupported_version` (fails closed) on such an event until it is
upgraded.


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
| `kg_graph` | the name of the database (its graph's domain) | only under an explicit graph-read policy for that database; membership alone grants none | only under an explicit graph-read policy for the instance; reaching a session's model takes an admitted context as well | `kg_events`, `kg_event_ops`, `kg_head`, `kg_nodes`, `kg_edges`, `kg_tags`, `kg_node_tags`, `kg_contents` |
| `kg_client` | a client scope (*Ids*) | its own (actor, client) | its own (actor, client) | `kg_receipts` |

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
- a **client scope** (`kg_receipts.client_scope`, the `kg_client` scope) is
  `<a>:<actor_id><c>:<client_id>`, `a` and `c` the number of UTF-8 bytes of each id in decimal:
  `6:tgorka8:device-1`. As with the person-agent key, the lengths decide where each id ends;
  `knowledge-graph.vectors.json`'s `client_scopes` holds values every implementation must
  reproduce.

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
  From schema 5, `tick_ids` (nullable JSON) names the identity of each tick:
  `{"at": <this row's updated_at_ms>, "drives": {<drive>: <ULID>}}`. A schema-5 client writes it in
  every write of the row: a fresh ULID for each drive it ticks anew, the same id for each drive it
  leaves ticked, none for a drive it unticks, and `at` equal to the `updated_at_ms` it writes. A map
  whose `at` is not the row's `updated_at_ms` (a schema-4 client rewrote the row and left it
  behind), or a ticked drive it does not name, is a tick with no identity. A person's device warns
  of a tick (R-HQ-79) until it has shown that tick's warning there, so a tick with no identity
  warns; a client may give it one, as the person, by rewriting the row. A schema-4 client never
  receives `tick_ids`. Its writes must invalidate the stored map as described above; preserving
  it is unsafe even when the timestamp guard usually detects it.
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
  one is written after it. Two more shapes of `metadata`:
  - **The `applied` control row** — keeper's word on a `drive_write` approval it carried out: a
    person message whose id is `applied.` followed by the approval's id, with no `turn_id`, and
    whose `metadata` is exactly `{"kind": "applied", "approval", "call", "outcome", "device"}`
    plus `"commit"` (the 40-hex commit) when `outcome` is `applied`, or `"reason"` (at most 1,000
    characters) when it is `not_applied`; `outcome` is `applied`, `unchanged` or `not_applied`,
    `call` the approval's `tool_call_id` and `device` the writing client's name (V18). It is a
    control row: nothing answers it, and a client draws it on its approval's card, not as a
    message.
  - **`focus`** — `{"focus": {"drive", "path", "heading", "line"}}` on an ordinary person message
    sent from the Notes Assistant: what the person was looking at when they sent it — the drive
    and the drive-relative `path` of the open note (at most 1,024 bytes), the caret's 1-based
    `line` (1 to 2³²−1) and the `heading` it sits under (at most 200 characters) or `null`. It has
    no `kind`, so the message starts a turn as any other.
- **`reasoning`** — `session_id`, `page_key`, `turn_id`, `seq`, `compacted` (true on the one row
  that replaces a turn's chunks after compaction), `text`.
- **`tool_calls`** — `session_id`, `page_key`, `turn_id`, `name`, `args` (JSON), `status`,
  `result`, `result_blob`, `started_at_ms`, `ended_at_ms`. A `drive_write` call's `args` are the
  request a person's device carries out, and its approval binds them (V8, V18).
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

A session's handle is a name a person can say, such as `noble-hedgehog`: two words drawn from
`vocabulary.json`'s `handle` lists by the SHA-256 of the session id's UTF-8 bytes. Bytes 0–1 and
2–3 of the digest, each read as a big-endian unsigned 16-bit number modulo its list's length, pick
the adjective and the noun, joined by `-`. `handle.examples` holds session ids with the handle
each must give, so every implementation can check itself. A handle is display only: it is a pure
function of the id, never a key, a scope value or a lookup path, and two sessions may share one,
which a client tells apart by the session number. With 64 words in each list there are 4,096
handles, so sharing is expected (even odds at about 75 sessions in one database); the session
number is what keeps them apart.

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
   `approved` becomes `consumed` only by the claim holder, with `consumed_by_client` the commit's —
   except an approval of a `drive_write` call, which is consumed only by the device that decided
   it (`decided_by_client`), never by the claim holder, because that device carries the write
   out; a decision on a tier-4 approval comes only from a client whose host is not the claim
   holder's host. A consumption carries the version it read, so a second consumer conflicts. An
   approval of a `drive_write` call is `T3`, and its drive is ticked for the session when it is
   made and when it is consumed.
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
18. **V18 — a write carried out.** A person message whose `metadata.kind` is `applied` reports what
    became of a `drive_write` approval: its id is `applied.` followed by the approval's id, it is
    written once and never changed, by the device that decided the approval, and it says
    `applied` or `unchanged` only once that device consumed the approval (`applied` naming the
    commit), or `not_applied` with a reason once the approval was decided.
19. **V19 — the knowledge graph.** The `kg_*` tables are written only inside the server's own
    graph commands (*The knowledge graph*): a commit from any client, the service client
    included, that writes one of them is refused, and so is a graph command's commit that breaks
    the rules of that section. A graph row's `graph` is the database's name, a receipt's
    `client_scope` the client scope of the request's actor and client.

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

## The knowledge graph

Each database holds one graph (its domain); nothing in one database's graph names a node of
another's. The graph is written only through registered graph commands, never by a row write or a
query, and a database whose operator has not given an actor a graph role gives that actor no
graph at all. This section is the shape both sides share; `knowledge-graph.vectors.json` pins
every rule in it that a client and a server compute, and `keeper-graph-protocol` is keeper's
implementation of them.

**Requests.** A graph command's `params` is the envelope
`{"version": 1, "request_id": <ULID>, "context_id": <ULID> | null, "payload": {…}}` and nothing
else. A `version` other than 1 is refused `unsupported_version` before anything else is read. A
person's device sends `context_id: null`; an agent sends the context the server issued and
admitted it under. The request's digest is the SHA-256 of the canonical bytes of
`{"version", "descriptor", "context_id", "payload"}`, `descriptor` the command's id
(`graph.<name>`); the actor, client, database and `request_id` are where it is looked up, not
part of it. A request id once bound to a digest is bound for good: the same id with another
digest is refused `idempotency_mismatch`.

**Values.** JSON with one number type: finite, at most 9,007,199,254,740,991 in magnitude, with
`1` and `1.0` one value and `-0` the value `0`; a larger exact integer is a string. No duplicate
key, ill-formed Unicode, NaN or infinity. Canonical bytes are RFC 8785 (JCS). Properties are an
object of at most 64 KiB canonical bytes, nested at most 8 deep; their keys never name a
structural field (below) and never start with `$`, except the typed reference
`{"$ref": {"entity_type": "node" | "content", "id": <ULID>, "revision": <decimal> | null}}`
(`resource_revision` and `blob` are reserved and refused until storage implements them).

**Vocabulary.** The reserved node kinds are `Concept`, `Collection`, `Resource`, `Tag`,
`Guidance` and `Vocabulary`; the reserved relationship types `HAS_TAG`, `contains`,
`guidance_scope` and `sources`.
A tag is a `kg_tags` row and a tag membership a `kg_node_tags` row, so `Tag` and `HAS_TAG` are
never a node's kind or an edge's type; they are how a query sees tags. Any other kind or type is
a custom name, `<namespace>:<name>`, which can therefore never spell a reserved one. A node's
structural fields are `id`, `kind`, `title`, `description`, `description_source_id`,
`lifecycle`, `lifecycle_revision`, `revision` and `merged_into_id`; an edge's `id`, `from_id`,
`to_id`, `type`, `lifecycle` and `revision`; a tag's `id` and `text`. The assertion names
`generated_at`, `observed_at`, `verified_at`, `verified_by`, `stale_after` and `status` are
reserved: no property may use them (`vocabulary.assertion_fields` in the vectors). A tag's
`text` is the canonical form keeper's tag policy gives (`tags` in the vectors). In the query
profile `KG-Cypher/1` a reserved name is its own symbol, and a custom kind, relationship type or
property is `K_`, `R_` or `P_` followed by the lowercase hex of its UTF-8 bytes; a `$`-prefixed
name never is.

**Reserved members** (story 111.1). A node's `properties` may hold, at the top level only, three
members each with its own closed shape (`node_properties[]` and `locators[]` in the vectors);
an edge's never does, and no member is a query symbol:

- `$definition` — required on a `Vocabulary` node and refused on every other kind:
  `{defines: <namespace>:<name>, of: kind|relation|property, status: active|deprecated,
  schema?, index_hints?, extensions?}`. A definition is versioned by its node's revision;
  deprecation is metadata only (entities of a deprecated kind stay readable and writable); its
  `schema` and `index_hints` are stored data, never executed; unknown `extensions` are kept
  byte-exact. No uniqueness rule holds across definitions.
- `$assertion` — exactly `{generated_at, observed_at, verified_at, verified_by, stale_after,
  status}`: times are integer ms or null, `status` `unverified` or `verified`, and
  `verified_at`/`verified_by` are set exactly when `verified`. A written one is always
  `unverified`: only `verify_assertion` verifies (*Changes*). From projection version 2 the six
  fields are structural keys of a node binding (null when absent).
- `$locator` — a `Resource` only: `{kind: internal|git|s3|relational|https|ssh_fs|local_fs,
  management: managed|external, locator, selector, version, format, capabilities?,
  credential_ref?, extensions?}`. Refused before anything is stored: URL userinfo (an authority
  after any `scheme://`, nested schemes such as `jdbc:postgresql://` and scheme-relative `//`
  included); a credential query or fragment parameter (case-insensitive, percent-decoded,
  `password`/`pwd`/`passwd`/`sslpassword` among them); a DSN `key=value` pair (libpq, ODBC, JDBC
  keyword forms) whose key, ASCII-lowercased with quotes, braces, `_` and `-` removed, is
  `password`, `pwd`, `passwd` or `sslpassword`, with whitespace allowed around `=` and every key
  position read (quoting is not honoured, so a quoted value can never hide a pair); and any key
  named like a credential. `credential_ref` is an opaque handle that grants nothing,
  `capabilities` are observations, and no resolver is implemented. An envelope is read through
  `graph.get_node`, never queried.

**Tables.** Ids are ULIDs; revisions and graph sequences are decimal strings the server assigns.

- **`kg_events`** — one accepted change: `graph_seq` (1, 2, … per graph), `descriptor`,
  `request_id`, `request_digest`, `actor_id` and `client_id` of the request, `origin_class`
  (`human` | `agent`, from the credential and context, never from the payload), `origin_actor`
  (the person an agent acted for, or null), `origin_chain` (JSON, or null: the delegation chain
  from that person to the acting context, server-derived from canonical parent sessions),
  `context_id`, `approval_basis` (JSON, or null),
  `reverses_event_id`, `event_time_ms` (the source's time, or null), `recorded_at_ms`, `op_count`
  and `delta_digest`. Never changed or deleted.
- **`kg_event_ops`** — its operations, `id` `<event_id>/<ordinal>`: `ordinal` (0 to
  `op_count` − 1), `entity_type` (`node` | `edge` | `tag` | `node_tag` | `content`; `review` is
  reserved), `entity_id`, `operation` (`create` | `update` | `archive` | `unarchive` | `remove`,
  the last only of a `node_tag`), `before_revision`, `after_revision`, `before_value`,
  `after_value`. An event's `delta_digest` is the SHA-256 of the canonical bytes of the array of
  its operations in ordinal order, each an object of exactly those eight members (absent as
  `null`). At most 128 operations, 128 KiB per operation and 2 MiB per event.
- **`kg_head`** — one row, `id` the database's name: `graph_seq` and `event_id` of the newest
  event.
- **`kg_nodes`** — `kind`, `title`, `description`, `description_source_id`, `properties`,
  `lifecycle` (`active` | `archived`), `merged_into_id`, `revision`, `lifecycle_revision`,
  `created_event_id`, `updated_event_id`.
- **`kg_edges`** — `from_id`, `to_id`, `type`, `properties`, `lifecycle`, `revision`,
  `created_event_id`, `updated_event_id`.
- **`kg_tags`** — `text` (unique in its graph), `label` (a display label, or null), `revision`,
  `created_event_id`, `updated_event_id`.
- **`kg_node_tags`** — `node_id`, `tag_id`, `created_event_id`; the one graph row ever deleted,
  by a `remove` operation.
- **`kg_contents`** (schema 7) — `owner_node_id` (fixed for good), `revision`, `body`,
  `created_event_id`, `updated_event_id`. A content has no lifecycle: its owner node's governs it.
- **`kg_receipts`** — the outcome of one request, `id` `<descriptor>/<request_id>/<client_scope>`:
  `descriptor`, `request_id`, `request_digest`, `outcome` (`accepted` | `proposed` | `conflict` |
  `rejected`; never `pending`), `reason` (a `rejected` receipt's stable code), `event_id` and
  `graph_seq` (an `accepted` receipt's, and only its), `detail` (bounded, safe JSON),
  `recorded_at_ms`.

An accepted change writes its entity rows, its event, every one of its operations, the head and
its receipt in one commit; a change refused for a business reason writes only its receipt. A
client replays a graph only from a complete prefix: events 1 to N with no gap, each with
exactly `op_count` operations whose digest is `delta_digest`.

**Outcomes.** A command's `applied` or `cached` answer is a transport acknowledgement whose
`commitSeq` bounds when the client's replica will see the receipt; it is never acceptance by
itself, and it lends its commit only to the request id it names. A client concludes from its
receipt (or the `graph.outcome` query), and only from the receipt whose id is its own
`<descriptor>/<request_id>/<client_scope>`: a request's receipt for another digest is
`idempotency_mismatch`. Without one, a `rejected` answer (the command ran and its commit was
refused) is terminal whatever its code; so is a refusal `idempotency_mismatch` or
`unsupported_version` (the stable code is the `operation.forbidden` message), and a server
fault (`kg.` code). Every other refusal, which may come before the command runs, and an answer
lost or unread, leaves the request pending under the same id until a receipt resolves it.

**Changes (`graph.apply`).** The payload is `{"ops": [<op>, …], "approval_id"?: <string>,
"event_time_ms"?: <integer> | null}`, 1 to 128 operations applied in order; every object is
closed (an unknown member is `invalid_change`). The operations, every member required unless
marked `?`:

| `op` | members |
| --- | --- |
| `create_node` | `id`, `kind`, `title` (non-empty), `description` (string or null), `properties` |
| `update_node` | `id`, `expected_revision`, `set`? (`{title?, description?, description_source?}`), `set_properties`?, `remove_properties`? (at least one; none empty) |
| `archive_node`, `unarchive_node` | `id`, `expected_revision` |
| `create_edge` | `id`, `from_id`, `to_id`, `type`, `properties`, `from_lifecycle_revision`, `to_lifecycle_revision` |
| `update_edge` | `id`, `expected_revision`, `set_properties`?, `remove_properties`? (at least one), `from_lifecycle_revision`, `to_lifecycle_revision` |
| `archive_edge`, `unarchive_edge` | `id`, `expected_revision`, `from_lifecycle_revision`, `to_lifecycle_revision` |
| `add_tag`, `remove_tag` | `node_id`, `tag`, `expected_revision` |
| `merge_nodes` | `target_id`, `target_revision`, `absorbed` `[{id, expected_revision}]` (at least one), `edges` `[{id, expected_revision, from_lifecycle_revision, to_lifecycle_revision}]` |
| `create_content` | `id`, `owner_node_id`, `body` |
| `update_content` | `id`, `owner_node_id`, `expected_revision`, `body` |
| `verify_assertion` | `id`, `expected_revision`, `sources` `[{edge_id, edge_revision, ref_id, ref_revision}]` (at most 64), `contents` `[{id, revision}]` (at most 64) |

The caller mints the ids of the nodes and edges it creates; a node's `kind` is fixed at creation.
A tag is named by its text, made canonical by keeper's tag policy (`tags` in the vectors); the
server reuses the graph's tag of that text or creates one (label null) and gives tags and
memberships their ids. Every precondition is the accepted state before the change:
`expected_revision` the entity's revision (a tag operation's, its node's), an edge's
`*_lifecycle_revision` its endpoint's `lifecycle_revision`; `null` (only where a node is named)
means the node is created by an earlier operation of the same change. `approval_id` is the
approval an agent acts under; the server derives every attribution. `event_time_ms` is the
source's time.

`description_source` names where a *generated* description comes from — a content of the same
node or a node carrying `$assertion` — and is set only together with a non-null `description`;
setting `description` without it makes the description authored and clears the provenance. A
`merge_nodes` or `verify_assertion` is the only operation of its change naming anything it names,
and a change holds at most one merge (`absorbed` and `edges` together at most 128).

A change with more than one create or explicit field/lifecycle operation for one entity, the same
canonical tag membership changed twice, a key both set and removed, a reference (an edge
endpoint or a tag operation, whatever precondition it names) to a node created only later, or
preconditions for one node that disagree (its revision across its operations, its lifecycle
revision across the edges naming it) is `invalid_change` from its payload alone. Against the graph
it is then, the first failure deciding:

1. each operation in order: a precondition the graph no longer holds — another revision, an
   entity it lacks, or a create of an id it has — is a `conflict`, whose `detail` is
   `{"entity": {"entity_type", "id"}, "precondition": "revision" | "lifecycle_revision",
   "current_revision": <decimal> | null}` (collapsed for a caller who may not read the graph);
   archiving what is archived, unarchiving what is active (or merged), removing a property or
   membership that is absent and adding a membership that exists are `invalid_change`; a node's
   or edge's final `properties` must still obey *Values*;
2. every typed reference written must name a node or content of the final graph at a revision
   it has had;
3. an edge this change creates, unarchives or updates and leaves active needs both endpoints
   active in the final graph, so an edge to an archived node is accepted only when the same
   change unarchives it; a `guidance_scope` edge must start at a `Guidance` node;
4. the active `contains` edges stay acyclic: for each `contains` edge `P → C` created or
   unarchived, a walk climbs from `P` a level at a time (a level: every active `contains` edge of
   the final graph into the frontier); past 64 levels, or past 10,000 edges read by all the
   change's walks together, it is `change_limit`, and a level with an edge from `C` (or `P` =
   `C`) is `invalid_change`. A level is read with a limit of one edge more than the budget still
   allows, never whole. Other relationship types may form cycles;
5. the final graph's own rules: every node written keeps its reserved members within its kind's
   rules; a description source resolves as above; a content's owner exists and a reference body
   names a `Resource`; and the verification rules below;
6. the event must fit its bounds (*Tables*), before-images included: `change_limit`.

**Merge.** `merge_nodes` absorbs nodes into an active, unmerged target: the target at
`target_revision`, each absorbed node active at its revision, and `edges` exactly the active
edges incident to any absorbed node, each under its `update_edge` preconditions taken from the
pre-merge endpoints — any difference is a `conflict`, so a dependency added or changed since the
read refuses with no effect. It expands, before the final checks, into one node `archive` per
absorbed node (`merged_into_id` the target, revision and lifecycle revision +1) and then one edge
`update` per listed edge (each absorbed endpoint rewritten to the target, same id, revision +1),
each with whole before- and after-images. Parallel edges keep their ids; archived incident edges,
tag memberships, contents and typed references stay with the absorbed node. `invalid_change`: a
`Guidance` or `Vocabulary` node, or one carrying `$assertion`, as target or absorbed; a rewritten
`guidance_scope` edge (a scope edge is retargeted only by archive and create); a rewrite that
would make a self-loop; a redirect cycle. A redirect chain longer than 8 hops is `change_limit`,
walked over `merged_into_id` with bounded reads. Rewritten `contains` edges are walked for
cycles like created ones. No operation clears `merged_into_id`: an unarchive of a merged node is
refused, and an `update_edge` never moves an endpoint.

**Verification.** `verify_assertion` (a person's alone; the server refuses it to an agent) is
accepted only against the exact state the person checked, read from the pre-change graph: the
node at `expected_revision`, active and carrying `$assertion`; `sources` exactly its active
outgoing `sources` edges (read through `kg_edges_from_active`), each at its revision, its one
top-level typed-reference property naming `ref_id` — the edge's `to_id` node, or a content that
node owns — at `ref_revision`, which is that entity's current revision; and `contents` exactly
the contents it owns (through `kg_contents_owner`), each at its revision. A difference is a
`conflict`. No active `sources` edge, a reference that is absent, null-revisioned or does not
resolve, an archived node or source target is `invalid_change`; more than 64 of either is
`change_limit`, read with a limit of 65. It writes one node `update` whose after-image differs
only in `status: verified`, `verified_at` (the server's acceptance time) and `verified_by` (the
authenticated person), never taken from the payload. A change may verify several assertions
(disjoint, since each locks what it names); each ends `verified`. A change that verifies may not
write any node or content one of its checked `sources` references — another assertion it
verifies included — because that verification would be stale on acceptance: `invalid_change`.
After it, every other write to a node
verified before the change — an update of any kind, a tag, an archive or unarchive — leaves it
`unverified` with null `verified_*` in that same operation, and a change to its `sources` edges
or contents is refused unless the same change holds an `update_node` of it; the content keeps its
own revision, and the node is never bumped on its behalf. Whether a verification is still
current is read, never written: it is, while the node is active and verified, it has 1 to 64
active `sources` edges, and every reference's revision is the referenced entity's current one
(`assertion_currency[]` in the vectors).

A refused or conflicting change writes only its receipt. An accepted one: a created node or edge
starts at revision 1 (a node at lifecycle revision 1, active), a new tag at revision 1; a node
touched in any way — fields, memberships, lifecycle — moves its revision exactly once, an archive
or unarchive also its lifecycle revision; an edge operation moves only the edge's revision. Its
event's operations follow the change's order: one node or edge operation of the same name per
node or edge operation; for a node only tag operations touch, one node `update` before its first
membership; for `add_tag` a tag `create` when the text is new, then the membership `create`;
for `remove_tag` the membership `remove`. Each image is the whole entity before the change and
after all of it: a node `{id, kind, title, description, description_source_id, properties,
lifecycle, merged_into_id, revision, lifecycle_revision}`, an edge `{id, from_id, to_id, type,
properties, lifecycle, revision}`, a tag `{id, text, label, revision}`, a membership `{id,
node_id, tag_id}`; an entity's row is its after-image. The accepted receipt's `detail` is
`{"affected": [{entity_type, id, revision, lifecycle_revision (nodes)}, …]}`, one per node,
edge and new tag in order of the change's first touch of each (which may differ from the
event's order). `algebra` in the vectors pins all of this.

**Content.** A content (`kg_contents`, schema 7) is created by `create_content` (revision 1;
its id new to the graph as a node, edge or content; its owner any node of the final graph) and
changed only by `update_content`, which names the content's id and owner node, both fixed for
good, and its expected revision; it replaces the body whole (`{"inline_text"}` of at most
32 KiB, or `{"reference": {resource_node_id, observed_revision}}` naming a `Resource`), moves
the content's own revision by exactly one, and moves nothing else — not the owner node's
revision. `contents` in the vectors pins the update rule; its operation is a `content`
`create` or `update` with the image `{id, owner_node_id, revision, body}`.

An agent's event carries `context_id`, `origin_actor` (the person whose session the chain
starts from), `origin_chain` `[{"session_id", "agent_actor"}, …]` from that root session down
to the acting one, and `approval_basis` `{"basis": "approval", "approval_id", "grant_id",
"origin"}`; a person's has `origin_chain` null and `approval_basis` `{"basis": "direct"}`.

**Reads.** Registered queries whose params carry `version` (1) and `context_id` (null for a
person's device); a caller who may not read the graph is refused alike whatever exists:

| query | params | rows |
| --- | --- | --- |
| `graph.outcome` | `request_id` | `[]` or the caller's own `kg_receipts` row for it |
| `graph.get_node` | `id`, `include_archived` | `[]` or `[{<node image>, "tags": [<canonical text>…]}]` |
| `graph.get_edge` | `id`, `include_archived` | `[]` or `[<edge image>]` |
| `graph.get_content` | `id`, `include_archived` | `[]` or `[<content image>]` |
| `graph.history` | `after` (decimal, `"0"` first), `limit` (1–100), `filter` `{entity_id?, actor_id?, origin_class?}` | `[{"events": [{"event": <header>, "ops": [<op>…]}], "scanned_through": <decimal>}]` |

Without `include_archived` a node is answered only while active, an edge only while it and both
its endpoints are, a content only while its owner node is. A history header is the event's row
without `graph` and the author columns,
JSON decoded. Every event in `(after, scanned_through]` that matches the filter is in `events`,
whole and ascending; `scanned_through` is the last position examined, matching or not, so an
empty or filtered page still moves the cursor and no later match is skipped. A page examines at
most 1,000 positions and holds at most `limit` events and 4 MiB of canonical event bytes beyond
its first event, and every event in it, the first included, is within the row bounds of
*Tables* (its header at most 128 KiB, its operations as for the event); a client refuses a page
outside these bounds, or an image outside its entity's shape. History is the retained events, never the pruned transport log.

**Read authority.** A graph read is exposed only under an authority the server answered for now,
through registered queries whose params carry `version` (1):

| query | params | rows |
| --- | --- | --- |
| `graph.admit_read` | `context`, `domain` | `[{context, session, permitted, sources, expires_at_ms, authority}]` |
| `graph.refresh_context` | `context` | the same row |
| `graph.read_status` | `domain` | `[{domain, authority}]`, for a person's device |

`authority` is `{"epoch": <decimal>, "digest": <lowercase SHA-256 hex>}`: the server's graph-read
authority epoch and its `(domain, principal)` digest. An agent's reader calls `graph.admit_read`
before it reads anything of a domain and `graph.refresh_context` again before it delivers a
result; a person's reader calls `graph.read_status`. A refusal (`operation.forbidden`, collapsed
for a caller who may not read), a domain missing from `sources`, or an authority other than the
one the result was computed under suppresses the result.

**Queries (`KG-Cypher/1`).** A native reader answers a bounded read-only Cypher subset from a
local projection of a complete accepted prefix, never from the server and never from the current
rows: one `MATCH` of one connected chain of at most 8 typed directed relationship segments; an
optional `WHERE` of at most 32 `AND`-ed comparisons of a property with a named parameter (`=`,
`<>`, `<`, `<=`, `>`, `>=`) or `IS [NOT] NULL`; `RETURN` of at most 16 aliased properties or one
`count(*)`/`count(variable)`; an optional `ORDER BY` of returned aliases; a literal `LIMIT`
1–1,000. Source at most 8 KiB; parameters a JSON object of at most 32 scalars and 64 KiB, numbers
normalised as values are. Labels, relationship types and property keys are reserved names or
their `K_`/`R_`/`P_` symbols; a node binding reads a node's structural fields, the six assertion
fields (projection version 2) or a tag's `id` and `text`, an edge binding an edge's. Anything
else is `query_unsupported`, any bound exceeded
`query_limit`. The renderer adds every binding's `id` ascending as the final sort keys, so ties
never depend on the engine. Only active nodes, tags, and active edges between active nodes are
visible; a membership is a `HAS_TAG` relationship to a `Tag` node. Archived entities are read
through the typed `get_node`/`get_edge`/`get_content` lookups, locally the same as the server's.
A content is never a query symbol. A result names
its domain, the prefix length N, the Syncular checkpoint, the schema, projection and engine
versions and whether the prefix is still the replica's whole history (`current`) or behind it
(`stale`). `queries[]` in the vectors pins the grammar and the renderer.

**The worker frames.** The query projection is built and read by a separate single-use worker
(`keeper-graph-query`). Its builder reads one manifest frame — domain, authority, N,
`prefix_digest`, Syncular checkpoint, `contract_schema` 7, projection version (2) and engine —
and
then exactly N event frames: `{graph_seq, event_id, op_count, delta_digest, ops}`, each op its
row without the before-image (the consumer has already verified every event's digest and
before-images against its own replay). `prefix_digest` chains from 64 zeros: each event's is the
SHA-256 hex of the previous digest, `graph_seq` and `delta_digest`, newline-separated. A frame is
at most 2 MiB, a prefix at most 100,000 events. Every Linux worker runs with exactly
`GQL_STMT_CACHE=0` and `GLIBC_TUNABLES` equal to `worker.linux_glibc_tunables` in the vectors.
`prefix_chains[]`, `manifests[]` and `worker` pin these.

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
