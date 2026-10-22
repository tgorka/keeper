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
| `session` | a session id | the sessions the actor is a member of, and the next 16 unused ids of each of the actor's clients | the same | `sessions`, `session_members`, `claims`, `session_tool_switches`, `turn_chunks`, `attachments` |
| `member` | an actor id | itself | itself | `session_members` |
| `page` | a page key | for each granted session, the months from its creation to now | the same | `messages` |
| `trace` | a page key | as `page`, only for sessions where the person's membership has `reads_trace` | as `page`, for the sessions the agent is a member of | `reasoning`, `tool_calls`, `approvals` |
| `actor` | an actor id | itself | itself | `instance_prefs` |
| `agent` | an agent's actor id | the agents of this database that serve the person | itself | `agent_memory` |
| `family` | a family name | the families of this database that serve the person | its own family | `agent_memory` |

A member who is not a reader receives a session's messages and its streamed text, and never its
reasoning, tool calls (with their arguments and results) or approvals: those live only under
`trace`. `turn_chunks` carry text only.

## Ids

Every id a client creates can be minted offline:

- a session id is `<clientId>.<n>`, `n` a decimal integer the client counts up; the server grants
  each client its next 16 unused ids, so a device creates a session and its first message in one
  commit without a round trip;
- a row of `messages`, `turn_chunks`, `reasoning`, `tool_calls`, `approvals`, `attachments` or
  `agent_memory`, and a turn id, is a ULID;
- a row there is at most one of is keyed by what it belongs to: `session_members.id` is
  `<session_id>/<actor_id>`, `instance_prefs.id` is `<person_actor>/<agent_actor>`, `claims` and
  `session_tool_switches` are keyed by `session_id`, `agent_profiles.id` is the instance's client
  id;
- `actors` and `clients` are written by the server's own service client, never by a device.

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
  client then offers no tool switch).
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
- **`turn_chunks`** — the committed text of a streaming answer: `session_id`, `turn_id`, `seq`,
  `text`; deleted when the turn's message is written.
- **`attachments`** — `session_id`, `message_id`, `blob_ref` (a syncular blob), `name`, `mime`,
  `size` (bytes, at most 26,214,400), `created_at_ms`.
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
  | `expired`), `decided_by_actor`, `decided_by_client`, `consumed_by_client`, `expires_at_ms`.
- **`instance_prefs`** — one person's choice of instance for one agent: `person_actor`,
  `agent_actor`, `default_client`, `fallback` (a JSON array of client ids, in order),
  `updated_at_ms`.
- **`agent_memory`** — `agent_actor`, `family`, `kind` (`user` | `memory` | `journal` | `proposal`
  | `family`), `text`, `source_session`, `created_at_ms`. Every row carries both scope values. A
  client requests the two independently, because a subscription matches every variable it
  requests: `agentMemoryOwn` (`agent` alone) receives one agent's rows, and `agentMemoryFamily`
  (`family` alone) receives every row of a family, its peers' included. A subscription requesting
  both would receive only the rows of that agent in that family.

Declared references, enforced by the server at commit: `clients` and `agent_profiles` need their
actor; `claims`, `session_tool_switches`, `turn_chunks` and `attachments` need their session and go
with it; an approval needs its tool call.

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
server's own list of people, agents and the databases of each. A rule broken refuses the whole
commit.

1. **V1 — authorship.** `author_actor` is the commit's actor and `author_client` is the commit's
   client, on every row written. The server also refuses a commit whose client is not the client
   the caller's credential was issued to.
2. **V2 — the directory.** `actors` and `clients` are written only by the service client. An
   `agent_profiles` row is written only by the instance it describes: its `id` is the commit's
   client and its `actor_id` the commit's actor.
3. **V3 — sessions.** A new session's id starts with the commit's client id followed by `.`.
   `agent_actor`, `created_by_actor`, `created_by_client` and `parent_session` never change. A
   session is inserted only by a person, or by an instance of an agent the registry lets start work;
   an instance of any other agent inserts only a child session whose `parent_session` is a session
   of this database the agent is a member of.
4. **V4 — membership.** A client inserts only its own actor's `session_members` row, and only in
   the commit that inserts the session; every other membership row is written by the service
   client (an agent the session names, and each invited person). `reads_trace` is turned on for a
   person only by the session's owner. A member may change only its own `read_until`.
5. **V5 — page keys.** As *Page keys* above.
6. **V6 — one writer per answer.** `turn_chunks`, `reasoning`, `tool_calls`, agent `messages`, new
   `approvals` and `sessions.heard_until` are written only by the client holding the session's
   claim.
7. **V7 — claims.** `holder_actor` is the session's agent. A change of holder increases `epoch` by
   exactly one, and an update carries the version it read, so a second taker conflicts instead of
   overwriting.
8. **V8 — approvals.** `pending` becomes `approved` or `denied` only by an actor named in
   `approvers`, never by an agent, with `decided_by_actor` and `decided_by_client` the commit's;
   `approved` becomes `consumed` only by the claim holder, with `consumed_by_client` the commit's;
   a decision on a tier-4 approval comes only from a client whose host is not the claim holder's
   host. A consumption carries the version it read, so a second consumer conflicts.
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

An attachment's `size` is at most 26,214,400 bytes and its message is in the same session.

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
