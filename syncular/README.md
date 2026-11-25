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
| `limits.json` | the bounds both sides read: `attachmentMaxBytes` (26,214,400), the most bytes one attachment's blob may have; `filePartThresholdBytes` (67,108,864), the largest file that goes in one presigned `PUT`; `filePartBytes` (67,108,864), the size of every part but the last, grown to `ceil(size / 10000)` rounded up to a MiB when needed (no file size bound); `graphMaxEvents` (100,000), the longest accepted graph prefix one generation may hold; `graphMaxEntities` (1,000,000), the most graph entities one generation may hold; `graphExportMaxBytes` (1,073,741,824), the most bytes one graph export may have; `settingValueMaxBytes` (16,384), the longest `value` a `settings_values` or `config_values` row holds before it must be a blob (*Settings*); `modelFileMaxBytes` (5,368,709,120), the most bytes one file of a published model set may have, sent in one presigned `PUT` on the admin path (*Model sets*); and the generic job bounds (*Jobs*): `jobRowMaxBytes` (65,536), the most bytes an encoded `jobs.*` command, and so a row it writes, may have; `jobInlineMaxBytes` (16,384), the most canonical bytes of an inline `input` or `result`; `jobNoteMaxBytes` (1,024), the most bytes of a progress note or a failure's sentence; `jobRefsMax` (64), the most references one job's input may name; `jobEffectsMax` (256), the most effects one completion or failure may report; `jobPlanMaxBytes` (1,048,576), the most bytes of a plan blob; `jobOpenMaxPerDomain` (10,000), the most jobs not yet ended one domain may hold; `jobAttemptsMax` (10), the highest `max_attempts`; `jobLeaseMinMs` (10,000) and `jobLeaseMaxMs` (600,000), the shortest and longest lease a claim may ask for; hand-written, never generated |
| `vocabulary.json` | the closed word sets both sides read: `efforts` (the reasoning efforts a session may name), `grantTiers` (the tiers a standing grant may cover), `roles` (the four roles a membership of a domain may carry, *Domains and the map*), `handle` (the word lists a session's handle is made of, with examples) and `personAgentKey` (examples of the person-agent key); hand-written, never generated |
| `knowledge-graph.vectors.json` | the knowledge graph's shared vectors (*The knowledge graph*): its fixed vocabulary, canonical values and their digests, typed references, tag canonicalisation, query symbols, client scopes, request digests and event-operation digests, each accepted or refused with a stable code; hand-written, never generated, and reproduced by every implementation (keeper's in `keeper-graph-protocol` and keeper-core tests) |
| `accounts.vectors.json` | the accounts contract's shared vectors (*Domains and the map*): `GET /databases` entries, the operator's admin requests and answers, and the refusal codes with their statuses; hand-written, never generated, and reproduced by every implementation (keeper's in `keeper-accounts-api`) |
| `object-layout.vectors.json` | the file object-store contract's shared vectors (*Objects in parts*): file and staging prefixes, content and staging keys, refused content ids, encoded user metadata, and part-size limits; hand-written, never generated, and reproduced by every implementation (keeper's in `keeper-ports`) |
| `graph-capacity.vectors.json` | the knowledge graph's domain-capacity vectors (*Domain capacity*): event frames with their line lengths and SHA-256 digests, and histories with each step's cumulative creates, review creates, removals and export bytes and the worker's and the consumer's entity counts; hand-written, never generated, and reproduced by every implementation (keeper's in `keeper-graph-protocol` and `keeper-graph` tests, the latter through the real exporter and worker) |
| `files.vectors.json` | the drive contract's shared vectors (*The drive*): names, folds, media types, git modes, content ids, page keys, locators, default modes, roots, narrower-or-equal, event-row checks, reserved facts and end-to-end command cases, each accepted or refused with a stable code; hand-written, never generated, and reproduced by every implementation (keeper's in `keeper-files-api`) |
| `files.media-types.json` | the closed extension-to-media-type table (*The drive*, decision 9) both sides read, with `default` (`application/octet-stream`) and `extensions`; hand-written, never generated |
| `settings.vectors.json` | the settings contract's shared vectors (*Settings*): record ids and keys, who may write a record, a device record and shared configuration, adoption, the `settings_owner` scope, credential-shaped values, the domain-name check and the blob rule, each accepted or refused with a stable code; hand-written, never generated, and reproduced by every implementation (keeper's in `keeper-settings-api`) |
| `settings.travel.json` | every settings key's travel class (`every_device`, `this_device`, `never`) and which layer file may set it, generated from keeper-core's key registry (`config::keys::tests::travel::regenerate`) and pinned by its test |
| `jobs.vectors.json` | the jobs contract's shared vectors (*Jobs*): envelopes and bound checks accepted or refused with a stable code, and command transitions over a job and its claim, including the attempt cases, a second plan, two fixture kinds and the move of an ended job to its month; hand-written, never generated, and reproduced by every implementation (keeper's in `keeper-jobs-api`, keeper-hq's server tests after 122.2's re-pin) |

Everything generated comes from `@syncular/typegen` 0.97.0 (Apache-2.0), pinned in keeper's
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
client rebootstraps on its own schema bump. Removing or renaming a column, changing a type or a
primary key is a new table, not an edit. A table's scope patterns are the exception, from
Syncular 0.69 on: changing them is an edit, made only at a schema-version bump (a new migration
directory, which may carry no DDL at all), because Syncular re-keys a table whose scope patterns
differ between two versions for each version's clients. A reference column never crosses scope
patterns: the referencing table and the referenced one keep the same patterns (§6.11).

Subscriptions are **units** (Syncular's wire 4): a template in `syncular.json` names one value per
scope variable and no table, and the unit it makes covers every table that declares all of its
variables. A window is a variable's, not a table's.

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

### Schema 8 release prerequisite

Schema 8 (`0008_knowledge_graph_review`) adds `kg_proposals`, `kg_reviews` and the bounded
owner read index `kg_contents_owner_id`, and nothing else; 0006 and 0007 are unchanged. The
server serves schemas 3 through 7 beside it with `kg_proposals` and `kg_reviews` omitted (and
`kg_contents` below 7, as before). A keeper whose graph worker reads projection version 3 ships
with or before the server release that first accepts a review or a reversal event: a version-2
worker refuses a `review` operation (fails closed) until it is upgraded.

### Schema 9 release prerequisite: the Syncular 0.97 move (wire 4)

Schema 9 (`0009_knowledge_graph_history`, story 115.6) changes no table, column or index. It moves
the graph's ledger — `kg_events`, `kg_event_ops`, `kg_head`, `kg_reviews` — from `kg_graph` to a new
variable, `kg_history`, with the same value (the domain's name) from the same `graph` column; the
state (`kg_nodes`, `kg_edges`, `kg_tags`, `kg_node_tags`, `kg_contents`) stays under `kg_graph`.
`kg_head` sits under `kg_history` only, not under both: it references `kg_events`, and the code
generator refuses a reference whose parent has other scope patterns (a reference never crosses
an authorization boundary). So a reader that holds only the state has no head; one that needs
the graph's freshness subscribes to `kg_history` for it. The server grants `kg_history` wherever
it grants `kg_graph`; a replica holds the ledger only when it subscribes the ledger's unit. The
server serves schemas 3 through 8 beside 9 with the ledger under `kg_graph` as before.

The same release moves keeper, keeper-hq and every agent replica to Syncular 0.97.0, which speaks
wire 4 only and refuses a wire-3 (0.32.1) client with `sync.invalid_request`. So the order is one
window: the server and the agent containers move first, then the keeper build is published and
installed on every device; no keeper build containing this contract is published before the
server has moved. A device not yet updated is refused until it updates. The contract's migrations
lock is format 3 (`syncular migrations upgrade-lock`); 0001–0008 and their hashes are unchanged.
On its first open the new client upgrades a 0.32.1 replica in place: it drops the old subscription
table and re-registers the units the device asks for, which then bootstrap again.

Under 0.97 the server answers `sync.commit_expired` for a commit resent below the watermark it
acknowledged, after it pruned that commit's result. The client journals the commit rejected and
drops it from the outbox; keeper shows it as a refusal with that code — the commit may well have
been applied — and does not reset the client id, although the server's error recommends
`resetClientId`.

### Schema 10 release prerequisite

Schema 10 (`0010_files`, story 116.1) adds the six drive tables (`files_entries`,
`files_entry_sources`, `files_events`, `files_event_ops`, `files_head`, `files_receipts`) and five
scope variables (`files_drive`, `files_domain`, `files_page`, `files_client`, `files_sources`). It
changes no existing table, column, index or scope variable; migrations `0001` through `0009` and
their hashes are byte-identical to schema 9. A server must serve schema 10 before a keeper build
carrying it is published; older clients keep their schema window and continue to sync against a
server serving schema 10 alongside schema 9.

### Schema 11 release prerequisite: settings

Schema 11 (`0011_settings`, story 121.1) adds three tables and changes none: `settings_values` and
`settings_devices` under a new variable, `settings_owner`, and `config_values` under `dir`. Number
10 is held by story 116.1; when it lands first the two are rebased in order and nothing in 0011
changes. The server serves the earlier schemas beside 11 with the three tables omitted, and grants
`settings_owner` (*Settings*) before a schema-11 keeper writes a record; until it enforces every
rule of *Settings* it refuses every write of the three tables.

### Schema 12 release prerequisite

Schema 12 (`0012_jobs`, story 122.1) adds `jobs`, `job_claims` and `job_capabilities` and nothing
else; earlier migrations are unchanged. The server serves older schemas beside it with these three
tables omitted, and refuses every `jobs.*` command until it enforces 122.2's fences, raw row writes
to the three tables refused, service writes included. The number 12 is provisional while `0010`
(116.1) and `0011` (121.1) are unmerged; the coordinator assigns the final one.

### Schema 13 release prerequisite

Schema 13 (`0013_knowledge_graph_file_refs`, story 116.5) adds `kg_file_refs` under `kg_graph` and
changes nothing else; migrations `0001` through `0012` and their hashes are byte-identical to
schema 12. The server serves older schemas beside it with `kg_file_refs` omitted, and refuses every
raw write of the table until it writes rows in the graph command's own commit (keeper-hq's commit
check admitting its upsert and delete).

## Settings

A person's settings are rows of their personal domain (D-26, D-27; story 121.1), and a domain's
shared configuration is rows of that domain. `keeper-settings-api` is keeper's copy of the rules;
`settings.vectors.json` pins them for both sides and `settings.travel.json` is the key registry's
travel data, generated from keeper-core's registry and never edited by hand.

- **Scope.** `settings_values` and `settings_devices` are under `settings_owner:{settings_owner}`
  on `owner_actor`, never `actor`: an agent holds the `actor` of every person it serves. The server
  resolves `settings_owner` to `[self]` for a person in their own personal domain, to nothing for a
  person anywhere else, refuses it to an agent instance or a tool (`settings.scope_refused`), and
  keeps it out of its own client's wildcard grant. A client subscribes with `settingsOwn`.
- **Records.** `id` is `<device>/<key>`: `*` for the person's record, which any of the person's own
  device clients writes, or a `device_id`, written only by the client its `settings_devices` row is
  bound to. A key is a registry key (a preference), `pin:<registry key>` (an account pin: under `*`
  the account's `keeper.toml`, under a device its `keeper.<device>.toml`, each allowed only where
  the registry's layer rule lets that file set the key), `offer:drive:…`, `offer:provider:…` or
  `offer:matrix:…` (person only), or `roles:drive:…` (a drive's roles on one device). A key in the
  never-leaves class is refused (`settings.never_leaves`), one this contract does not know is
  refused (`settings.unknown_key`), and a key outside its class is refused (`settings.wrong_class`):
  an every-device key travels in `*`, a this-device key in the device's own records.
- **No secret travels.** A credential-shaped value is refused (`settings.credential_value`), and a
  credential choice (`sync.credential_source.…`, `bots.provider_credential_source.…`) records only
  `account`, `own` or `none`, inline: such a key's value as a `value_blob` is refused
  (`settings.credential_value`), since the server cannot read a blob at commit.
- **Bounds.** A `value` longer than `limits.json`'s `settingValueMaxBytes` is refused
  (`settings.value_too_large`); it is `value_blob` instead, uploaded under the generic upload grant
  within `attachmentMaxBytes` (`settings.blob_too_large` above it) and read through the generic
  download route by the owning person's own device clients only; anyone else gets the answer an
  absent blob gets.
- **The device record (D-27).** The server binds a `settings_devices` row to the client that
  creates it (`client_id`). Only that client writes the row and its device's records
  (`settings.not_this_device`), and a raw write never changes `client_id`
  (`settings.binding_changed`). An adoption moves the binding to a new client of the same person
  when the claim's fingerprint, class and platform match the record, or, for a record with no
  fingerprint, when it is untouched for 30 days (`settings.adoption_refused` otherwise); the name
  follows the claimant, and the earlier client loses its write. The host name a client reports is
  never consulted. "Restored once" is `account.<id>.restored` in the install's own table.
- **Shared configuration.** `config_values` in the `makidrive` domain is written only on the
  operator's admin path — never by a device credential, the domain owner's included, an agent, a
  tool or the server's own client (`config.writer_refused`). In any other domain the admin path or
  a person holding `owner` there writes it. Its keys are `/`-separated lowercase segments
  (`config.malformed`); the setting seeds are `template/keeper.toml`, `template/settings.toml`,
  `template/class/<class>.toml` and `template/settings/<class>.toml`, each one file's text. A key
  or value naming a domain is refused (`config.names_domain`).

On a revision conflict the syncing device writes its pending value again over the newer one, so
per key the write the server accepts last wins: still "last to sync" (D-26).

### Model sets

Model sets are rows of `0011`'s `config_values` in the `makidrive` domain (story 121.3, DF/AD-16;
no schema change), pinned in `settings.vectors.json` (`modelKeys`, `modelVersions`, `modelSets`,
`modelUploads`, `modelWrites`, `configBlobReads`).

- **Keys and values.** `models/<version>/<path>` holds one file: value `{"size", "sha256"}` and
  `value_blob` with `blobId` `sha256:<sha256>` and `byteLength` `size`. `models/<version>` is the
  manifest: value `{"models_toml", "file_count"}`, inline (within `settingValueMaxBytes`, no
  `value_blob`). `models.active` is the active version id as a JSON string, no `value_blob`.
  `<version>` is 64-character lowercase hex sha256; `<path>` is `/`-separated `[A-Za-z0-9._-]`
  segments, none empty, none starting with `.`, and not `models.toml` (keys at most 256 bytes;
  `modelKeys`). The generic `config_values` write refuses `models.active` and `models/…`
  (`config.malformed`).
- **Version id.** Lowercase hex sha256 of `keeper-models-v1\n`, then `<path>\t<size>\t<sha256>\n`
  per file sorted by path bytes, an empty line, and `models.toml`'s bytes (`modelVersions`,
  `modelSets`).
- **Publication and sealing.** Every write is on the operator's admin path in the general domain
  only (`config.writer_refused` for every device credential, the owner's included, and for any
  other domain). Upload each file under an admin-path grant bound to `size` and `sha256` in one
  presigned `PUT` within `modelFileMaxBytes` (`config.model_too_large`; `modelUploads`), write the
  file rows, then write the manifest (refused `config.model_incomplete` until every file row is
  present and hashes to `<version>`), then write `models.active` last (refused
  `config.model_incomplete` for a version without its manifest; `modelWrites`). A key or
  `models.toml` naming a domain is refused (`config.names_domain`). Version rows are never
  rewritten and no file row is added after the manifest (`config.model_sealed`); only
  `models.active` moves.
- **Blob reads.** Reading `config_values.value_blob` through the generic download route is granted
  to a person who is a member of the row's domain; anyone else (non-member person, agent, tool,
  service) gets the absent-blob answer (`configBlobReads`). Model blobs live in the conversation
  key space, never the files module's (DF/AD-8).

## Domains and the map

A domain is one audience: a database of this contract, its schema, its drive and its graph, named
alike. Each has a kind — `general` (the organisation's one general domain, `makidrive`),
`personal` (one person's) or `group` (a group's) — and each person is a member of some of them,
with one of `vocabulary.json`'s `roles` — `reader`, `editor`, `reviewer`, `owner` — or none. The
same four roles name a person's standing in every module of a domain; what each admits is that
module's rule. `keeper-accounts-api` is keeper's copy of these types (story 114.2).

**The map is server-only.** Which domains exist, who is in each and with which role, which agents
serve whom and which sign-in subject is which person are the server's own tables in the general
domain's schema. They are not a table of this contract, a scope or a replica's: no row of them
reaches any device or agent instance. They change only on the operator's admin path, whose one
client is the operator's tool and whose one guard is the operator's token; a device credential —
an owner's included —, an agent instance, the service client and the import token change nothing
there, by any route, a Syncular commit or a command included. Every accepted change is an event of
the map's history attributed to the operator, written in the request's own transaction.

**What a person sees of a domain they are not in: nothing.** The owner's answer: "Co członek
widzi z domeny, do której nie należy: nic" — not its name, not that it exists. So:

- `GET /databases` lists only the domains the credential opens. Each entry is `name`, `mount`,
  `kind`, `display_name`, and, when they exist, `replaced_repository` (the git repository the
  domain replaced, told only to its members), `graph_root` (the id of the domain's root
  `Collection`, the node carrying `okf:profile_version`) and `files_writes` (`shadow` | `open`, the
  domain's file-write record). A client ignores an entry field it does not know, and skips an
  entry whose `kind` or `files_writes` is a value it does not know, reading the others: a kind
  added later hides that one domain from an older client, never the whole body. `kind` and
  `display_name` are required, so a body from a server older than story 114.2 does not parse; a
  client of story 114.5 or later does not support that server.
- No row, scope value, refusal, agent profile or ticked drive names a domain to someone who is not
  in it, and nothing lists the domains that exist.
- A domain the credential does not open and a name that is no domain get one answer, byte for byte
  apart from the date, on every route under the name and on the realtime upgrade: today's 401 for
  a mount the token does not open — Syncular's own on its routes, `{"error":
  "sync.auth_required"}` on the upgrade and the server's own routes.
- The general domain's directory carries `actors` and no `clients`: every person is in it, and
  `clients` would show each the others' devices.

A map-dependent admission — a graph command or scope, a first enrolment, a pairing redemption —
waits while no map has been applied since the server started (`starting`), while the map is being
applied (`reconciling`), after it failed to apply (`failed`) or when it was restored older than the
last applied one (`stale_restore`), and is refused meanwhile with `hq.map_closed`; conversations
and their attachments do not wait. Only `open` admits: a reader takes a barrier state it does not
know as closed. `hq.map_closed` is answered with 503 on an HTTP route — a first enrolment, a
pairing redemption; for a graph command it is a commit refusal with no HTTP status. The admin
route's own refusal while the map cannot be read stays `hq.admin.map_unreadable`.
`accounts.vectors.json` lists every code the accounts contract adds.

**The operator's admin path.** Every change is one request `{request_id, expected_revision,
command, confirm?}`; its fields are closed, and `hq.admin.malformed` answers one that does not
parse. The server checks the request id first: the first request it applies binds the id to the
request's digest, the SHA-256 of the RFC 8785 (JCS) bytes of the request as the contract
serialises it after parsing — every required field present, `serving_agents` and `accept` among
them (`[]` for none), an optional field without a value absent and never `null`. An optional
request field given as `null` is refused `hq.admin.malformed`, because the canonical form never
has `null`. `expected_revision`, and every integer in this contract, stays within ±(2^53−1), the
integers JCS and a JavaScript reader hold exactly; the server refuses a request carrying a larger
one `hq.admin.malformed`. The same id and digest again gets the recorded answer with no second
effect, and the same id with another digest is refused `hq.admin.request_reused`. Only then does
it check the revision, so a retry after a lost answer gets the recorded answer, not a stale
revision: a request whose `expected_revision` is not the map's revision is refused
`hq.admin.stale_revision`. The revision is the `seq` of the map's last history event, 0 before the
first, and `seq` is one sequence for the whole map, not one per domain. An answer of
`confirmation_required`, `unreconciled` or a refusal applies nothing and binds nothing.

**An agent's graph capability** in a domain is written by `set_agent_grant {domain, agent_id,
capability, expires_at_ms?}`, which replaces any earlier one, and withdrawn by
`remove_agent_grant {domain, agent_id}`, which needs no agent of that id to exist any more (stories
114.3, 114.6). Neither names the general domain: one that does is refused `hq.admin.malformed` at
parse, since no agent holds anything there. `set_agent_grant` names an agent of the credential
file (`hq.admin.unknown_person` otherwise), and both name a domain the map lists
(`hq.admin.unknown_domain` otherwise).
An agent's id is no person's: `add_person` under it is `hq.admin.person_exists`, and a
`set_membership` naming it `hq.admin.unknown_person`.
The capability is the graph's grammar, stored as given — an optional member absent stays absent,
none is ever `null` — and anything the grammar does not admit is `hq.admin.malformed`: `classes`
(up to 6 distinct of `capability_classes`: `read`, `content`, `vocabulary`, `guidance_body`,
`guidance_scope`, `reverse_own`; none only beside a non-empty `files`, with `allowedKinds` and
both root lists empty, `rootModes` absent and `domainWideContent` absent or `false` — a files-only
grant, which gives no graph change, so no graph event's grant basis cites it), `allowedKinds`
(distinct reserved kinds, never `Vocabulary`, empty without a write class), `collectionRoots`
(content or `reverse_own` only) and `guidanceRoots` (needed by, and only with, a guidance class) —
each at most 256 distinct node ids of at most 128 bytes with no control character, `/` or `:` —
and `domainWideContent?` (a boolean, `true` only with `content`; `content` needs it or a
collection root). Two members ride beside them, each non-empty when given. `rootModes?` maps a
collection root to the mode its `content`-class changes land by, one of `grant_modes` (`ask`,
`propose`, `apply`); a key that is not a collection root is refused. These modes are an agent
instance's: in an agent instance's grant a root not named is `ask`, and guidance, vocabulary and
what `domainWideContent` alone covers stay `ask`. A tool has no mode per root: its change inside
its whole grant, whichever class covers it, lands `apply`; whether a grant's holder is a tool or
an agent instance is the credential file's, not the capability's, so the server applies this
rule and the grammar cannot. `files?` is the file part, a list of `{roots, classes, mode}`, every
member required and none other: `roots` the directory entry ids it covers and `classes` the file
classes, each a non-empty list of distinct non-empty strings, and `mode` a non-empty string; the
words its `classes` and `mode` may hold are the files module's, so the contract carries them as
given.
Which modes the server lets the operator store is the server's rule, not a parse. The list answer
carries `agent_grants` only when a grant exists: each `{domain, agent_id, issuer, revision,
expires_at_ms?, capability}`, the capability as the request gave it and read with the same
grammar, so a member this contract does not have fails the reader rather than being dropped.

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
| `kg_graph` | the name of the database (its graph's domain) | only under an explicit graph-read policy for that database; membership alone grants none | only under an explicit graph-read policy for the instance; reaching a session's model takes an admitted context as well | `kg_nodes`, `kg_edges`, `kg_tags`, `kg_node_tags`, `kg_contents` (the state; schema 9) |
| `kg_history` | the name of the database (its graph's domain) | as `kg_graph` | as `kg_graph` | `kg_events`, `kg_event_ops`, `kg_head`, `kg_reviews` (the ledger; schema 9; through schema 8 these were under `kg_graph`) |
| `kg_client` | a client scope (*Ids*) | its own (actor, client) | its own (actor, client) | `kg_receipts` |
| `kg_proposal` | a proposer's actor id | itself | itself | `kg_proposals` |
| `files_drive` | `<domain>/<drive>` (`drive_key`) | every live drive of a domain where the person holds `reader` or above (never `'*'`, never from membership alone; a device holds a whole drive or none) | the drives of a domain covering the instance's file capability roots (and tools with a file class) | `files_entries` (schema 10) |
| `files_domain` | the name of the database (its drive's domain) | when the person holds `reader` or above in the domain | when the instance or tool holds a file class in the domain | `files_head` (schema 10) |
| `files_page` | `<domain>/<yyyy-mm>` (UTC month of `recorded_at_ms`) or `<domain>/import` | as `files_domain` | as `files_domain` | `files_events`, `files_event_ops` (schema 10; and `files_reviews` from 116.6) |
| `files_client` | a client scope (*Ids*) | its own (actor, client) | its own (actor, client) | `files_receipts` (schema 10) |
| `files_sources` | the name of the database (its drive's domain) | never, whatever the role | agent instances and tools with a file class in the domain | `files_entry_sources` (schema 10; reserved, holds no row until 120.2) |
| `jobs_page` | `<domain>/open` or `<domain>/<yyyy-mm>` (*Jobs*) | as the domain's job-read policy grants (membership of the domain) | when the instance serves the domain | `jobs`, `job_claims` (schema 12) |
| `jobs_nodes` | the domain's name | as `jobs_page` | as `jobs_page` | `job_capabilities` (schema 12) |

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
and no month of an archived session is subscribed (its session-scoped rows still sync under
`session`). The month lives inside the value, never as a variable of its own, because a variable
may be requested alone and a month variable would read across sessions.

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
  definition's ceiling less what its container does not mount, and that everyone who reads this
  database may read). Each is absent until an instance says, and a client then offers no picker
  for it. A row stored before a reader joined is narrowed by the server's own client when who reads
  the database, or a drive it names, changes, before the new reader's first pull; a replica that
  pulled the old row keeps it until its next pull.
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
  (`agent_profiles.drives`), and it grants reading, never writing. When who reads the database,
  or a ticked drive, changes, the server's own client unticks every drive that no longer passes
  V17's audience rule — the name and its tick id leave the row, the other ticks keep theirs —
  before a new reader's first pull.
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
   `efforts`. Every drive in `drives` is one everyone who reads this database may read; a row that
   names another is refused (`hq.profile.drive_audience`). The one exception: the service client
   may rewrite a stored row's `drives` to remove a name that no longer passes this rule, and
   change nothing else.
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
    the drive. The one exception: the service client removes a drive that no longer passes this
    rule. In the same write it removes that drive's id from `tick_ids.drives`, keeps every
    remaining drive's id, and sets `updated_at_ms` and `tick_ids.at` to one new value, so the
    remaining ticks keep their identity; it changes nothing else.
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
admitted it under. A tool identity (story 114.6) is no third kind of sender: it acts inside a run
session the server makes for it in the tool's own domain, whose only member is the tool, and
sends the context the server issued it there, under every rule of an agent's context. The
request's digest is the SHA-256 of the canonical bytes of
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
  credential_ref?, extensions?}`. `locator`, `selector` and `credential_ref` are admitted only
  when they match an allowlist grammar; everything else is refused before anything is stored.
  A **URL** (the text holds `://` or starts with `//`) is printable ASCII, its scheme one of
  `https`, `git`, `ssh`, `sftp`, `s3`, `postgres`, `postgresql`, `file` (any case), its
  authority exactly `host[:port]` (no `@` at all, no `%`; a host of unreserved characters or a
  bracketed IPv6 literal; empty only for `file`), its query and fragment free of `@`, and every
  `&`/`;` query or fragment key, decoded once, of unreserved characters and, ASCII-folded, neither
  a credential parameter (`token`, `access_token`, `password`, `secret`, `sig`, `signature`, the
  four `x-amz-*`/`x-goog-*` signatures, `pwd`, `passwd`, `sslpassword`) nor named like a
  credential. Otherwise a text holding `=` is a **libpq keyword DSN**, tokenised as libpq does
  (spaces around `=`, quoted values, backslash escapes) but stricter: printable ASCII, keys only
  `host`, `hostaddr`, `port`, `dbname`, `user`, `passfile`, `sslmode`, `application_name` (exact,
  no repeats), a space or the end after every value (libpq also takes a key straight after a
  closing quote), and unescaped values of unreserved characters, `/`, `:`, `+`, `,` and spaces.
  Anything else is a **plain reference** (`/srv/a.md`, `host:/srv/notes`, `keeper:drive/notes`):
  unreserved characters, `/`, `:`, `+`, `,`, and no `//`, so a path with a space or non-ASCII is
  refused. Any object key named like a credential is refused too. `scripts/check-locator-parsers.py`
  holds every `locators[]` vector to libpq's `PQconninfoParse` and Node's `URL`: an admitted vector
  yields no credential in either, and each pinned bypass vector does. `credential_ref` is an
  opaque handle that grants nothing, `capabilities` are observations, and no resolver is
  implemented. An envelope is read through `graph.get_node`, never queried.

**Tables.** Ids are ULIDs; revisions and graph sequences are decimal strings the server assigns.

- **`kg_events`** — one accepted change: `graph_seq` (1, 2, … per graph), `descriptor`,
  `request_id`, `request_digest`, `actor_id` and `client_id` of the request, `origin_class`
  (`human` | `agent`, from the credential and context, never from the payload; a tool's event is
  `agent`, told apart by its own `actor_id`, so a client of an older build reads it), `origin_actor`
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
- **`kg_file_refs`** (schema 13) — `entry_id` (key), `node_id`, `node_kind`; the graph's state;
  see **Files**.
- **`kg_proposals`** (schema 8) — one immutable proposal revision, `id`
  `<proposal_id>/<revision>`: `status` (`open` | `approved` | `rejected` | `expired` |
  `superseded`), `reason`, `digest`, `change` (the `graph.apply` payload), `reviewers`,
  `expires_at_ms`, the proposer's actor, client, context, origin and source labels, and the
  decision's time, person and event. Private: synced to its proposer only; never replayed.
- **`kg_reviews`** (schema 8) — one review of an accepted event, equal to its `review`
  operation's after-image: `target_event_id`, `target_delta_digest`, `kind` (`acknowledgement` |
  `reversal`), `reviewer_actor`, `revision` `1`, `created_event_id`. Never updated or deleted.
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
starts from; null for a tool's run session, which has none), `origin_chain` `[{"session_id",
"agent_actor"}, …]` from that root session down to the acting one, and `approval_basis`
`{"basis": "approval", "approval_id", "grant_id", "origin"}`; a person's has `origin_chain` null
and `approval_basis` `{"basis": "direct"}`. A change accepted with no approval of the call,
because the actor's graph grant lets it land — a tool's anywhere inside its whole grant, always
with `mode` `apply` since a tool has no mode per root, an agent instance's under a root its grant
gives `apply` (story 114.6) — carries `{"basis": "grant", "grant_id", "grant_revision", "mode"}`:
snake_case, all four always present. `grant_id` is the grant's id in the map, `<domain>/<actor_id>`
(a domain name holds no `/`, so the id splits at its first);
`grant_revision` the map revision that wrote the grant, the `revision` the admin path lists for
it (an integer, 1 to 2^53−1); `mode` the mode that applied, one of `ask`, `propose`, `apply`.
Every form is closed and every member present, `null` where a value may be absent;
`approval_bases` in the vectors pins the four, and a client keeps the column as JSON, so a form
it does not know never stops it reading the event.

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
`prefix_digest`, Syncular checkpoint, `contract_schema` 8, projection version (3) and engine —
and
then exactly N event frames: `{graph_seq, event_id, op_count, delta_digest, ops}`, each op its
row without the before-image (the consumer has already verified every event's digest and
before-images against its own replay). `prefix_digest` chains from 64 zeros: each event's is the
SHA-256 hex of the previous digest, `graph_seq` and `delta_digest`, newline-separated. A frame is
at most 2 MiB, a prefix at most 100,000 events. Every Linux worker runs with exactly
`GQL_STMT_CACHE=0` and `GLIBC_TUNABLES` equal to `worker.linux_glibc_tunables` in the vectors.
`prefix_chains[]`, `manifests[]` and `worker` pin these. A `review` operation is checked and
skipped: no review is an entity of a generation (projection version 3).

**Proposals and decisions** (story 111.2, `keeper-graph-protocol::proposal`). `graph.propose`'s
payload is exactly `{proposal_id, base_revision, change, reviewers, expires_at_ms}`: a new
proposal (`base_revision` null, revision 1) or the next revision of an open one, a `graph.apply`
payload without `approval_id` validated as a write is, 1 to 16 ascending distinct reviewer actor
ids, and an optional expiry. Its receipt is `proposed` with `{proposal_id, proposal_revision,
proposal_digest}`; the digest is the SHA-256 of the JCS of `{version, graph, proposal_id,
revision, change, reviewers, expires_at_ms}`. `graph.decide` (`{proposal_id, proposal_revision,
proposal_digest, decision}`, `approve` | `reject`) is a person's who is enumerated on that
revision: an approval is the proposal's accepted event (the decider's request, actor, client and
origin class, context null, the proposer's origin, `approval_basis {basis: "proposal",
proposal_id, proposal_revision, proposal_digest, proposer_actor, proposer_client,
proposer_context}`) or terminally `rejected proposal_stale`; a rejection is `proposed` with the
revision's new `status`. A reviewer reads a proposal through `graph.proposal`, authorised per
call.

**Reviews and reversals** (`review`, `inverse`). `graph.acknowledge` (`{target_event_id,
target_delta_digest}`, a person with the reviewer role) adds one event of one `review` creation
(kind `acknowledgement`) and its `kg_reviews` row; the acknowledged event never changes and stays
reversible; a second acknowledgement is refused `invalid_change`. `graph.unreviewed` pages the
agent-origin `graph.apply`/`graph.reverse` events no review targets — a tool's among them,
whatever basis it landed under —, as `graph.history` pages.
`graph.preview_reversal` answers one event's server-derived inverse (its planned operations, a
restored membership's id null), the `expected` revisions and whether it applies now (`ready`,
`approval_required` — ready, but an agent's reversal needs its per-call `approval_id`; a person
never gets it —, `conflict` or `rejected`; any other outcome is unreadable and fails closed).
`graph.reverse` (`{target_event_id, target_delta_digest, expected,
approval_id?, event_time_ms?}`, never operations) re-derives the inverse, requires the current
graph to be exactly the target's after-images and `expected` exactly the plan's, and adds one
event: the inverse operations and one `review` creation of kind `reversal`, with
`reverses_event_id` set. An update restores the before-image, a create is archived, a merge is
restored (redirect cleared, endpoints put back) and its reversal re-merges; every revision moves
forward, no id is reused, and verification is never restored (always `unverified`). A reversal
is judged by forward's own per-operation refusals and whole-candidate validation (references,
endpoints, containment, node members, description provenance, content owners, verification),
differing only by the four inverse exceptions listed once at `candidate::Exception`:
restore-merged (an `unarchive` clearing `merged_into_id`), re-merge (an `archive` setting it,
judged by the merge rules: a survivor that is `Guidance`, `Vocabulary` or carries `$assertion`
is refused `invalid_change`), endpoint moves (only along a redirect the same reversal restores
or sets, never on a `guidance_scope` edge or into a self-loop), and the X29 normalisation. A
later dependent of a node the reversal archives (an active edge, an owned content, a
domain-visible reference written after the target) is a `conflict` whose precondition is
`dependency`; a `graph.reverse` conflict is reported as `reversal_conflict`. `inverses[]`,
`inverse_caps[]` (the 128-operation cap as generators: `creates` node creates, reversed, fit
with their review at 127 and refuse `change_limit` at 128), `reversals[]`, `reviews[]`,
`previews[]` (preview rows decoded or refused), `proposals[]` and `outcomes[]` pin these.

**Files** (story 116.5). A `Resource` node points at a file entry with `$locator` kind `internal`
and text `files:<domain>/<entry_id>` (116.1 decision 1). The locator names the entry by id and
carries no version (a version is `(entry_id, revision)`), so it follows renames, moves, deletes and
restores. A `Collection` node binds a `dir` entry through top-level property `okf:directory` set to
the same text. Each drive has one `Collection` under the domain's root `Collection`, bound to the
drive's own root `dir` entry (H-16); the domain's root `Collection` (carrying
`okf:profile_version`) is never bound. The legacy `internal` locator (`keeper:drive/notes`) is
resolved by nobody (`files.resolve` answers `unresolved`) and counts for no entry.

At most one active node of either kind may name an entry in a final candidate; a second is refused
`conflict` with detail `{"entity":{"entity_type":"node","id":<the other active
node>},"precondition":"file_ref","current_revision":<its accepted revision>}`. Whole-candidate
validation enforces this across `create_node`, `update_node` retarget, `unarchive_node`,
`merge_nodes` and every reversal (`graph.reverse`). `kg_file_refs` holds at most one row per entry
by one rule (H-10): the active node counting for the entry, else the one archived last; a node
merged away counts only when its survivor names the entry. The server writes `kg_file_refs` in the
graph command's own commit, never as an event operation, so `delta_digest` never covers it. Raw
sync writes to `kg_file_refs` are refused for person, agent and service clients alike, and no
`files.*` command (`files.change`, `files.import`, `files.reverse`) writes it. Deleting a file
leaves its row in place; restoring the file finds the same node. No graph change writes a manifest
row or moves an entry's revision. `keeper-graph-protocol::file_ref` implements the rule, and
`file_refs[]` and `file_ref_admissions[]` in `knowledge-graph.vectors.json` pin it. Whether an
entry exists in the domain and is a `dir` is the server's (`file_ref::admit`); another domain's id
answers as one that exists nowhere (`invalid_change`).

**Generated text, reserved.** Top-level node property `okf:summary` and provenance names
`okf:model`, `okf:prompt`, `okf:generator` and `okf:source_content_id` are reserved and unused; the
later program decides their carrier, value and place. A hand-written note is a content
(`kg_contents`) the node owns, referenced by no reserved property and not by
`description_source_id`, and kept out of `description`.

**Vectors, reserved.** Table name `kg_vectors`, key `(subject_type, subject_id, model, chunk)` and
per-graph scope variable `kg_vectors` are reserved. Its rows will be derived and disposable, never
an event and subject to no graph limit, written by a job kind's tool identity using a per-domain
provider; a label-less reader sees no labelled entry, and nothing under untrusted globs is indexed
until D-34's mark (G-20). Neither the table nor any granted `kg_vectors` variable exists yet; a
test over `syncular.json` holds that.

### Domain capacity

One generation of a domain's graph holds at most the three graph limits of `limits.json`, which
equal the worker's `MAX_EVENTS`, `MAX_ENTITIES` and `MAX_EXPORT_BYTES`: `graphMaxEvents`
(100,000 events), `graphMaxEntities` (1,000,000 entities) and `graphExportMaxBytes`
(1,073,741,824 bytes). The server keeps a threshold below each limit and refuses an event that
would carry a domain past one with a terminal `rejected` receipt, code `change_limit`, and
`detail` `{"limit":"domain","measure":"events"|"entities"|"export_bytes"}`, before any approval
is consumed. The domain stays readable; nothing it holds shrinks, so acknowledgements and
reversals are refused too.

The server counts each measure over the accepted prefix. Events are the head's `graph_seq`.
Entities are every `create` operation, reviews included: at least the worker's count (every
non-review entity ever created, a removed membership kept) and the consumer's (live images,
reviews included). Export bytes are the sum of every event's frame line, its JCS text and
newline, the manifest line excluded; a line is
`157 + digits(graph_seq) + digits(op_count) + (op_count − 1) + Σ op + 1`, an operation
`118 + digits(ordinal) + len(entity_type) + len(operation) + R + V`, where `R` is 4 for a null
`after_revision` or its length plus 2, and `V` is 4 for a null `after_value` or the length of its
canonical text. `graph-capacity.vectors.json` pins the frame lines and each history's counts.

## The drive

Each database holds one domain's drives as synced manifest rows (schema 10, `0010_files`). Rows are
written only through registered `files.*` commands, never by a raw sync write, and nothing is ever
physically deleted. The contract names no server, database or bucket; a server serving schema 9
keeps serving older clients through the schema window (116.2 proves it). `files.vectors.json` and
`files.media-types.json` pin every shared rule, and `keeper-files-api` is keeper's implementation.

**Tables.** Every files table uses `id` as its single primary-key column. Entry, event, request and
change-set ids are ULIDs (`^[0-7][0-9A-HJKMNP-TV-Z]{25}$`); entry `revision` and event `seq` are
server-assigned decimal strings starting at `"1"` on create and incremented by 1 on each accepted
change that touches the row. No table has a `path` column: an entry sits in its parent `dir` by
`parent_id`, `name` and stored `name_fold`, and a drive's root is a `dir` row whose `parent_id` is
its own `id`.

- `files_entries` (`files_drive` on `drive_key` = `<domain>/<drive>`): `id`, `domain`, `drive_key`,
  `parent_id`, `name`, `name_fold`, `kind` (`file` | `dir`), `content_id` (plain text
  `sha256:<64 lowercase hex>`, null for `dir`), `size` (null for `dir`), `media_type` (null for
  `dir`), `mtime_ms` (null for `dir`), `mode` (`normal` | `executable`), `revision`, `lifecycle`
  (`live` | `deleted`), `created_event_id`, `updated_event_id`, `updated_seq`, `author_actor`,
  `author_client`.
- `files_entry_sources` (`files_sources` on `domain`; reserved, holds no row until 120.2): `id`,
  `domain`, `revision`, `sources` (JSON), `author_actor`, `author_client`.
- `files_events` (`files_page` on `page_key` = `<domain>/<yyyy-mm>` or `<domain>/import`): `id`,
  `domain`, `seq`, `page_key`, `descriptor`, `request_id`, `request_digest`, `actor_id`,
  `client_id`, `origin_class`, `origin_actor`, `origin_chain`, `context_id`, `approval_id`,
  `approval_basis`, `proposal_id`, `change_set_id`, `job_id`, `job_ordinal`, `job_attempt_id`,
  `reverses_event_id`, `event_time_ms`, `recorded_at_ms`, `op_count`, `delta_digest`,
  `author_actor`, `author_client`.
- `files_event_ops` (`files_page` on `page_key`): `id` (`<event_id>/<ordinal>`), `domain`,
  `event_id` (references `files_events(id)`), `ordinal`, `page_key`, `entity_type` (`entry` |
  `review`), `entity_id`, `operation` (`create` | `replace` | `move` | `delete` | `restore` for
  entries; `create` for reviews), `before_revision`, `after_revision`, `before_value`,
  `after_value`, `before_content_id`, `after_content_id`, `author_actor`, `author_client`.
- `files_head` (`files_domain` on `domain`): one row per domain, `id` = `domain`, `seq`, `event_id`
  (no declared reference, since events sit under `files_page`), `digest` (newest event's
  `delta_digest`), `drives` (`[{"name","root_id"}]` sorted by `name`), `author_actor`,
  `author_client`.
- `files_receipts` (`files_client` on `client_scope`): `id`
  (`<descriptor>/<request_id>/<client_scope>`), `client_scope`, `descriptor`, `request_id`,
  `request_digest`, `outcome`, `reason`, `event_id`, `seq`, `detail`, `recorded_at_ms`,
  `author_actor`, `author_client`.

**Scopes and grants.** A person is granted `files_drive`, `files_domain` and `files_page` only when
holding `reader` or above in the domain — never `'*'`, never from domain membership alone. A
credential with no role or file class in the domain is granted none of these rows. `files_sources`
is never allowed to a person, whatever the role; only agent instances and tools holding a file class
in the domain may be granted it. `drive_key` is the finest scope key, so a device holds a whole
drive or none; deeper folder roots are enforced on commands, never by scopes. `files_client` is each
caller's own `(actor, client)`.

**Requests.** Every `files.*` command's `params` is the closed envelope
`{"version": 1, "request_id": <ULID>, "context_id": <ULID> | null, "approval_id"?: <ULID> | null, "job"?: {"job_id", "ordinal", "attempt_id"} | null, "payload": {…}}`.
`request_digest` is the lowercase hex SHA-256 of RFC 8785 canonical JSON of
`{"version", "descriptor", "context_id", "approval_id", "job", "payload"}`. `delta_digest` is the
lowercase hex SHA-256 of canonical JSON of the event's ordered operations array. Every `files.*`
command runs with lock mode `shared`.

- `files.change`: `{"operations": [op…], "change_set_id"?: <ULID> | null, "event_time_ms"?: <int> | null}`
  (at least one operation; mixed kinds `create`, `replace`, `move`, `delete`, `restore`; one entity
  at most once per change).
- `files.put` (file `create` or `replace`), `files.move`, `files.delete`, `files.restore`:
  `{"operation": op, "change_set_id"?: <ULID> | null}` — one-operation shorthands of `files.change`.
- `files.import`: `{"operations": [op…], "change_set_id": <ULID>, "event_time_ms": <int>}` (both
  required), each operation carrying `provenance: {"drive", "commit", "path"}` and `git_mode`
  instead of `mode` on file creates/replaces, with no `mtime_ms`.
- `files.reverse`: `{"event_id": <ULID>}` (implemented in 116.3). `files.preview_reversal` and
  `files.history` are read-only queries (like `files.freshness`), not commands.
- Story 116.6 adds `files.propose`, `files.decide` and `files.acknowledge`.

**Outcomes.** `files_receipts.outcome` is `accepted`, `proposed` (from 116.6, `detail` carrying
proposal id, revision and digest), `conflict` or `rejected`. Receipts work as KG1's: a `request_id`
once bound to a `request_digest` returns its earlier outcome on replay and refuses a different
digest with `idempotency_mismatch`. Every accepted command writes its entry mutations, one
`files_events` row, its `files_event_ops` rows, the updated `files_head` and its `files_receipts`
row in one commit; a refused command mutates no manifest row and writes only a receipt.

**Evaluation order.** The first failing step decides:

1. `version` ≠ 1 → `unsupported_version`.
2. `request_id` bound to another digest → `idempotency_mismatch`; same digest → earlier outcome.
3. Reserved envelope fields: non-null `approval_id` or `job` → `reserved_field`; non-null
   `context_id` from person, tool or import → `reserved_field`; null `context_id` from agent →
   `forbidden`.
4. Command dispatch: `files.import` from anyone but the import identity, or import sending anything
   else → `forbidden`; unknown descriptor → `invalid_change`.
5. Grant read (rooted callers): unknown mode → `unknown_mode`; unknown class → `invalid_change`.
6. Payload parse and shape → `invalid_change`; any `sources` member → `reserved_field`; invalid
   entry or drive name → `invalid_name`.
7. Authority: person `reader` (write or reverse), person `editor` or rooted caller reversing another
   actor's event, or drive create by non-owner or rooted caller → `forbidden` (import may create a
   drive only as operation 0 of `files.import`); drive name folding to `.keeper-logs` →
   `reserved_name` for every sender; rooted caller touching any location outside its covering `write`
   (and, for a reversal, `reverse_own`) roots → `outside_roots`; strictest mode `propose` →
   `propose_required`.
8. Per-operation preconditions in order: create of an existing id → `conflict` (`absent`); missing
   entry or wrong revision → `conflict` (`revision`); wrong lifecycle → `conflict` (`lifecycle`);
   replace of a `dir` or move of a drive root → `invalid_change`; absent, deleted or non-`dir`
   parent → `parent_invalid`.
9. Final state: moving a `dir` into itself or a descendant → `move_into_self`; live touched entry
   without a live `dir` parent → `parent_invalid`; duplicate `name_fold` among live siblings or live
   drives → `conflict` (`name`); deleted `dir` (root included) with a live child → `not_empty`;
   named `content_id` absent from the domain's key space (or unreadable to a rooted caller) →
   `content_absent`; charged operations > 128 → `change_limit`.
10. Accept: assign `seq = head + 1`, write event, ops, row revisions, updated head and receipt.

**Refusal codes (closed).**

- `unsupported_version`: envelope `version` is not `1`.
- `idempotency_mismatch`: `request_id` was already bound to a different `request_digest`.
- `invalid_change`: malformed payload/operation, unknown descriptor/class, duplicate entity, or
  illegal op (such as replacing a `dir`, moving a drive root, `executable` dir, or bad `git_mode`).
- `invalid_name`: name is not a single `keeper-paths` segment, contains NUL, or exceeds 255 UTF-8
  bytes.
- `reserved_name`: drive name folds to `.keeper-logs`.
- `reserved_field`: a reserved field (`approval_id`, `job`, `context_id` from non-agent, any
  `sources` member, or non-null `approval_id`/`origin_chain`/`context_id` on a `release` event).
- `forbidden`: caller lacks role/identity authority (reader write, a reader reversing, an editor or
  agent/tool/job reversing another's event, non-owner drive create, non-import `files.import`, or
  agent with null `context_id`).
- `outside_roots`: rooted caller touches an entry or parent outside its covering `write` (and, for a
  reversal, `reverse_own`) roots.
- `propose_required`: strictest covering mode for a rooted caller's write or reversal is `propose`.
- `unknown_mode`: caller's grant carries a mode outside `ask`, `propose`, `apply`.
- `conflict`: precondition failure (`absent`, `revision`, `lifecycle`, `dependency` or sibling/drive
  `name`).
- `parent_invalid`: named parent does not exist, is `deleted`, or is not a `dir`.
- `move_into_self`: a `dir` is moved into itself or one of its descendants.
- `not_empty`: a `delete` targets a `dir` (including a drive root) that still has a live child.
- `content_absent`: a create or replace names a `content_id` not present in the domain's file key
  space (or not readable/uploaded by a rooted caller).
- `change_limit`: charged operations exceed 128.

**Operation bound and arithmetic.** One change may carry at most 128 charged operations. Every
ordinary operation charges 1; a cross-drive `move` charges 2 per row it carries (the entry and every
descendant row, any lifecycle) and is refused whole with `change_limit` above 128. The `review`
operation on a reversal or acknowledgement counts outside the bound, so a 128-operation change can
be reversed whole. At this bound, a first import of 157,516 entries requires at least
⌈157,516 / 128⌉ = 1,231 changes/events and 157,516 operation rows, all under page key
`<domain>/import`; a within-drive move of a folder holding 38,178 entries is 1 operation (one row
updated; descendants are not rewritten).

**Reversals (116.3).** `files.reverse` takes only the closed payload `{"event_id": <ULID>}` (any
other member, such as `entity_ids` or `operations`, is refused `invalid_change`; no partial reversal).
The server plans the inverse solely from the retained event's entry ops in reverse ordinal order
(carried cross-drive descendant ops and `review` ops are skipped, so the 38,178-entry within-drive
move reverses as one operation), each inverse op expecting the target op's `after_revision`:
`create` → `delete`, `restore` → `delete`, `delete` → `restore`, `replace` → `replace` with
`before_content_id`, before `size`, before `mtime_ms` (and before `mode` if recorded), and `move` →
`move` to before `parent_id`/`name`. An unknown event is refused `conflict` (`absent`, `entity_id` =
event id); an event with no entry op is refused `invalid_change`. Reversal is all or nothing (any
refusal writes nothing), and a reversal event is itself reversible (re-applying the original).
Authority: person `editor` may reverse only own events, `reviewer`/`owner` any event of the domain,
`reader` none (`forbidden`); rooted callers (agent/tool/job) may reverse only own events (`forbidden`
otherwise), and every touched location (where the entry sits now — before — and, for a move, the
parent it returns to — after) must lie inside both a `reverse_own` root and a `write` root
(`outside_roots` otherwise), taking the strictest `write` mode over those locations (`propose` →
`propose_required`; until 116.6 the server admits only `ask` for an agent and refuses others with
116.2's `mode_unavailable`, a server admission rule outside the closed set above). Where forward
would answer `not_empty` (inverse deletes a `dir` with a live child not also deleted by the inverse)
or `parent_invalid` (restored or moved-back entry would sit under an absent, deleted or non-`dir`
parent), reversal answers `conflict` (`dependency`, naming the smallest such child id or that parent
id); later edits stay `conflict` (`revision`/`lifecycle`) and another live entry now folding equal at
the move-back target stays `conflict` (`name`). An accepted reversal records `reverses_event_id` and
appends one `review` op (`entity_type` `review`, `operation` `create`, outside the 128 bound; 116.6
adds `files_reviews`). `files.preview_reversal` is a read-only query with closed params
`{"version": 1, "event_id": <ULID>}` (`version` ≠ 1 → `unsupported_version`, other shape →
`invalid_change`) that runs inverse, authority, preconditions and final state as `files.reverse`
would for this caller, writing nothing and binding no request, returning
`{"target_event_id", "status", "plan", "code", "detail"}` with `status` `ready`, `conflict` or
`rejected`. Vectors: `inverses[]`, `reversals[]`.

**History (116.3).** `files.history` is a read-only query with closed params
`{"version": 1, "after": <decimal>, "limit": 1..=100, "filter"?: {"entity_id"?, "actor_id"?, "origin_class"?}}`
(`version` ≠ 1 → `unsupported_version`; non-canonical decimal `after`, `limit` outside `1..=100`,
unknown member or empty-string filter value → `invalid_change`). It returns
`{"events": [{"event", "ops"}…], "scanned_through": <decimal>}`: events with `seq > after` matching
every given filter field, ascending numerically by `seq`, at most `limit`; each item's `ops` are
ascending by ordinal (under `entity_id`, only ops of that entity); `scanned_through` is the last
event's `seq` when the page is full (`len == limit`), else `max(after, head.seq)` (`"0"` with no
head). Queries read through indexes (`files_event_ops(entity_id)`, `files_events(domain, actor_id, seq)`,
`files_events(domain, origin_class, seq)` from `0010`), never a scan-then-filter (unlike
`graph.history`'s 1,000-position scan). The `<domain>/import` page is read like any other. Authority
is the server's: a caller not granted the domain's `files_page` rows is refused as for an unknown
domain. Vectors: `history[]`.

**Two devices, one file (116.3).** When a client's `files.put` replace is refused `conflict`
(`revision`), it writes its own bytes as a new entry (`files.put` create, new ULID, the entry's
current parent, its `content_id`) named `<stem>.sync-conflict-<now_utc>-<device>.<ext>` via
`keeper_paths::conflict_name`, trying numbered ordinals `2..=1000` (`conflict_name_numbered`) until
a `name_fold` not among the parent's live children is free (`None` if invalid or none free); the
server never invents a name. Modification beats deletion: a replace refused `conflict` (`lifecycle`,
deleted by another device) is written the same way, using the plain `name` when its fold is free. A
stale delete refused `conflict` (`revision`) is dropped and the edited file stays live. Vectors:
`conflict_copies[]`, `cases: conflict-copy-two-clients`, `cases: conflict-stale-delete-refused`.

**`files.freshness` and server-only tables.** `files.freshness` is a read-only query with params
`{"version": 1, "domain": <domain>}` returning
`{"last_commit_time_ms": <int> | null, "last_run": {"started_at_ms": <int>, "ended_at_ms": <int> | null, "result": "running" | "ok" | "failed"} | null}`,
answered from 118.4's server-only record. The server keeps `files_request_bindings`,
`files_event_sources`, `files_upload_grants`, `files_objects` and `files_unreviewed` server-only
(never synced); 120.2 adds server-only `files_releases` and `files_release_origins`, read only
through `files.releases` per caller.

**Decisions 1–18.** Each design choice fixed by story 116.1 is pinned in `files.vectors.json`:

1. **Entry id and locator.** An entry's `id` is a ULID stable across renames and moves. Its
   `internal` locator text (named by a graph `Resource`, 116.5) is `files:<domain>/<entry_id>`.
   Vectors: `locators: locator-tg-entry`, `cases: move-dir-38178-within-drive`.
2. **Content id.** Plain text `sha256:<64 lowercase hex>` of the whole content, equal to the git
   LFS oid and Syncular blob id form, including the empty file
   (`sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`) and files of any
   size, never a store's multipart checksum. It is a plain-text column, never a `blob_ref`, so
   writes never enter Syncular's conversation blob space. Vectors: `content_ids: content-id-empty-valid`, `content_ids: content-id-uppercase-invalid`, `cases: code-content-absent`.
3. **Capability classes, roots and modes.** Rooted callers hold classes `read`, `write` and
   `reverse_own` in a closed list of `{roots, classes, mode}` items (114.6's `FileItem`); there is
   no domain-wide flag — reach over a drive is its root entry id in `roots`. A root is a live `dir`
   id matched by walking `parent_id` (so it follows renames and moves); an empty `roots` list covers
   nothing. Modes are `ask` > `propose` > `apply` (strictest wins over touched locations); defaults
   are `apply` for tools and jobs, `ask` for agent instances; the mode comes from the grant, never
   the request (unknown mode → `unknown_mode`). `propose` refuses direct writes and reversals with
   `propose_required`; reversal lands by the `write` class's mode while `reverse_own` gates whose
   effects may be reversed. `narrower_or_equal` requires the same domain and, for every given item,
   a held item whose roots contain all given roots, classes ⊆, and mode no less strict. No grant
   hand-over is built here (120.2 adds delegated contexts). Vectors: `default_modes: default-mode-agent`, `modes: mode-auto-unknown`, `inside: inside-xf-in-x-after-rename`, `inside: inside-nxf-in-x-after-recreate`, `narrower: narrower-looser-mode-apply`, `cases: roots-agent-apply-sh`, `cases: roots-agent-mixed-apply-and-propose`, `cases: roots-agent-empty-roots`, `cases: roots-agent-reverse-own`.
4. **Retained versions (option (c)).** `before_content_id` and `after_content_id` on
   `files_event_ops`, together with `content_id` on every `files_entries` row whatever its lifecycle
   (and 116.6's `open` proposals), form the files sweep's keep-set. A version stays reversible for
   as long as its event exists — forever, with no history purge (as KG1). The bucket therefore holds
   every distinct content ever named, at the cost of the sum of their sizes, deleting only
   unreferenced uploads after grace. Vectors: `cases: replace-then-reverse-earlier-bytes`, `cases: move-dir-38178-reversed`.
5. **Tree and directory moves.** Entries store `parent_id`, `name` and `name_fold` under scope
   `drive_key` (`<domain>/<drive>`), with no `path` column. A within-drive move updates one row
   without touching descendants' revisions; a cross-drive move charges 2 per carried row (any
   lifecycle) and is refused whole above 128 with `change_limit`. A drive is created by one `create`
   (`kind: "dir"`, `parent_id: null`, stored `parent_id = id`, `drive_key = <domain>/<name>`) by an
   owner, the admin path, or import op 0; `.keeper-logs` is refused `reserved_name`. A root is
   excluded from its own listing and children's fold lookup; deleting a non-empty root is refused
   `not_empty`, and deleting an empty root removes it from `files_head.drives` while keeping the
   deleted row. Vectors: `cases: move-dir-38178-within-drive`, `cases: move-dir-38178-across-drives`, `cases: move-small-across-drives`, `cases: drive-create-owner`, `cases: drive-delete-empty`, `cases: drive-import-op0-accepted`.
6. **Deleting a non-empty directory.** Refused with `not_empty` — there is no silent cascade. The
   caller deletes the children in the same change (or an earlier one). Vectors: `cases: drive-delete-non-empty`.
7. **Imported file `mtime_ms`.** An imported file's `mtime_ms` is its import event's `event_time_ms`
   (the git commit time); a person's `create`/`replace` may carry its local `mtime_ms` or null; a
   `dir` always has null `mtime_ms`. Vectors: `cases: import-mixed-ops`.
8. **Event replication and paging.** Which event pages a device holds is deferred by name to 119.5.
   Decided now: every `files.import` event and its operations sit under page key `<domain>/import`
   (never a calendar month), read on demand through `files.history`, and `files_reviews` (116.6) is
   scoped by its event's `page_key` rather than with `files_head`. Vectors: `page_keys: page-key-import`, `cases: import-mixed-ops`.
9. **Media type (option (b)).** Derived by the server from the entry name's extension alone via
   `files.media-types.json` (`default: "application/octet-stream"`), using the ASCII-lowercased text
   after the last `.` when that `.` is not the first character (`dir` is null), and recomputed on
   every rename. Stored types are labels: served responses remain clamped as `served_mime` clamps
   attachments, so no stored type makes a file run as a page. Vectors: `media_types: media-img-heic-upper`, `renames: rename-md-to-pdf`, `cases: media-types-and-rename-recompute`.
10. **File `mode`.** Takes `normal` or `executable`. `create` defaults to `normal`; `replace`
    keeps the existing mode unless given; `move` keeps it; a `dir` is always `normal` (`executable`
    → `invalid_change`). `files.import` maps git `100644` → `normal` and `100755` → `executable`
    (anything else → `invalid_change`). Story 118.3 compares `mode`; it never makes a webview run a
    file. Vectors: `git_modes: git-mode-executable`, `git_modes: git-mode-symlink-refused`, `cases: mode-create-and-rename-preserve`.
11. **`files.import` shape.** Accepts `operations` (`create` dir/file, `replace`, `move`, `delete`),
    each carrying `provenance: {"drive", "commit", "path"}` (kept in server-only records; refused
    on other commands) and `git_mode` on file creates/replaces (no `mtime_ms`). Writes `descriptor:
    "files.import"`, `origin_class: "import"`, `approval_basis: {"basis": "import"}`, page key
    `<domain>/import`, required `event_time_ms` and `change_set_id` (shared across a run), under the
    same 128-op bound. Sent only by the import identity; its operation 0 may create its drive.
    Vectors: `cases: import-mixed-ops`, `cases: import-from-person-forbidden`, `cases: drive-import-op1-forbidden`.
12. **Reserved fields, sources and Syncular 0.97.0 confirmation.** Envelope `approval_id` and `job`
    are reserved and refused `reserved_field` when set; `context_id` is refused `reserved_field`
    from person, tool or import and required from an agent instance (null → `forbidden`).
    `files_entry_sources` and server-only `files_event_sources` are reserved and empty, and any
    `sources` member is refused `reserved_field`. A release (120.2) covers drive sources only and
    keeps its record server-side (`files_releases`, `files_release_origins` via `files.releases`).
    Re-read at `@syncular/server` 0.97.0 (115.6's release): rows reach a client only through
    `requested ∩ allowed` per unit and the host's `resolveScopes` (`dist/scopes.d.ts:2,23`;
    `dist/admin.d.ts:223-260` `scope-denied`), scope columns change only via `rescope`
    (`dist/push.js:215-231`), there are no trusted prefix or range scans (`README.md:418`), and
    `schemaWindow` (`dist/context.d.ts:121`) serves older schemas — so a person is never allowed
    `files_sources` and its rows never reach a person's device (116.2 acceptance 16 proves it).
    Vectors: `cases: reserved-person-context-id`, `cases: reserved-approval-id`, `cases: reserved-job-ref`, `cases: reserved-op-sources`, `cases: roots-agent-null-context`.
13. **Acceptance metadata and list receipts.** `approval_basis.basis` is `direct`, `approval`,
    `proposal`, `grant`, `release` (whose synced event row carries null `approval_id`,
    `origin_chain` and `context_id`; any set is `reserved_field`) or `import`. `origin_class` is
    `human`, `agent`, `tool`, `import` or `job`. Frozen columns include `origin_actor`,
    `origin_chain`, `context_id`, `approval_id`, `proposal_id`, `job_id`, `job_ordinal` and
    `job_attempt_id` (job columns all null unless `origin_class` is `job`). `files_event_ops` uses
    `entity_type` `entry` | `review`, with the review op outside the 128-op bound. For 116.6's
    multi-item `files.decide` and `files.acknowledge` (list receipt, H-13), the server writes one
    `accepted` receipt with null `event_id`/`seq` and an ordered per-item result array in `detail`,
    atomic as a commit and per-item in effect. Vectors: `event_rows: event-basis-direct` … `event-basis-import`, `event_rows: event-origin-job`, `event_rows: event-acknowledge-review`, `event_rows: event-release-with-origin-chain`, `cases: change-at-bound-reversed`.
14. **Names.** English ASCII lower snake case; `id` primary key on every files table; KG twins share
    column names (`descriptor`, `actor_id`, `client_id`, `origin_class`, `recorded_at_ms`,
    `entity_type`, `entity_id`, `operation`, `before_value`, `after_value`, `author_actor`,
    `author_client`), keeping `domain` and `seq` for this module. Renames from earlier drafts:
    `root`/`tree_key`/`files_tree` → `drive_key`/`files_drive`; `path` → `parent_id` + `name` +
    `name_fold`; `last_event_id` → `updated_event_id`; `last_seq` → `updated_seq`;
    `actor`/`client`/`origin`/`command`/`created_at_ms` →
    `actor_id`/`client_id`/`origin_class`/`descriptor`/`recorded_at_ms`; `job_effect` →
    `job_id`/`job_ordinal`/`job_attempt_id`; `entry_id`/`before`/`after` →
    `entity_id`/`before_value`/`after_value`. Vectors: `event_rows: event-unknown-origin-class`, `cases: code-unsupported-version`, `cases: code-idempotency-mismatch` (every refusal code has a vector: `keeper-files-api` test `every_refusal_code_has_a_vector`).
15. **Changed fields in `files_event_ops`.** `before_value` and `after_value` record changed fields
    only: `create` sets before `null` and after the full row (`kind`, `drive_key`, `parent_id`,
    `name`, `name_fold`, `content_id`, `size`, `media_type`, `mtime_ms`, `mode`, `lifecycle`);
    `replace` records `content_id`, `size`, `mtime_ms` always and `mode` if changed; `move` records
    `parent_id`, `name`, `name_fold` always, `media_type` if changed, and `drive_key` if changed
    (carried cross-drive descendant ops record `drive_key` only); `delete` and `restore` record
    `lifecycle`. `before_content_id` and `after_content_id` hold the file's content id before and
    after (both set on move/delete/restore of a file; null for dirs). Vectors: `cases: tree-rename-same-media`, `cases: mixed-change-and-reverse`.
16. **Parent row, directory nulls and re-creating a deleted name.** A child change never writes its
    parent's row. A `dir` has null `content_id`, `size`, `media_type` and `mtime_ms` and no derived
    totals. The fold lookup filters by `lifecycle = 'live'`, so a deleted entry does not hold its
    name. Re-creating a deleted name creates a **new** entry beside the deleted row, which stays
    `deleted`; restoring that deleted row later while the name is taken is refused `conflict` with
    precondition `name`. Vectors: `cases: tree-two-creates-parent-unchanged`, `cases: tree-create-beside-deleted`, `cases: tree-list-path-and-dir-fields`.
17. **One change command and change sets.** `files.change {operations: […]}` carries mixed operation
    kinds in one atomic event; `files.put`, `files.move`, `files.delete` and `files.restore` are
    one-operation shorthands. Callers may attach a ULID `change_set_id` to group related changes and
    proposals while each change remains atomic on its own. Vectors: `cases: mixed-change-and-reverse`, `cases: mixed-change-stale-revision-atomic`, `cases: mixed-change-set-id-grouping`.
18. **Reserved `files_facts` table.** Reserved here, not created until 122.6's contract rung: keyed
    by `(content_id, extractor, extractor_version)` with primary key `id` =
    `<content_id>/<extractor>/<extractor_version>`, columns `domain`, `media_type`, `facts` (bounded
    JSON), `extracted_at_ms` and `job_id`. Its scope variable `files_facts` is granted to people at
    `reader` or above and to the extraction kind's own tool identity only (no agent instance or
    other tool), and rows are written only by a command, never a raw sync write. The row carries no
    trust mark (untrusted-path labelling is applied on read, never stored — one row serves both
    trusted and untrusted paths). Extraction is modular (J-4): adding an extractor, media type or
    key is a new version of the extractors' data file, never a migration. Vectors: `facts_reserved: facts-reserved-shared-content-two-paths`.

## Objects in parts

Files travel between a device and its database's object store over presigned URLs the server
grants, never through the replica SQLite or a sync round (story 115.3). A file's content id is
`sha256:<64 lowercase hex>` of the whole file; a part's digest is standard base64 (with padding)
of its raw SHA-256 (the `x-amz-checksum-sha256` form).

**Key spaces and metadata.** Each database `<db>` keeps its files under `files/<db>/`, separate
from Syncular's conversation blob space (`conv/<db>/blob/<partition>/sha256/<hex>`); the two never
overlap:

- content key of `sha256:<hex>`: `files/<db>/sha256/<hex>`
- staging prefix: `files/<db>/staging/`; a plan's staging key: `files/<db>/staging/<plan_id>`
- user metadata: one header `x-amz-meta-keeper-file`, whose value is unpadded base64url of the
  compact UTF-8 JSON `{"contentId":"sha256:<hex>","size":<n>,"mediaType":"<type>","uploadedAtMs":<ms>}`
  in that exact key order, with `mediaType` omitted when absent.

`object-layout.vectors.json` pins the prefixes, keys, refused ids, metadata encodings and
part-size rule (`limits.json`: `filePartThresholdBytes` for one `PUT`, `filePartBytes` per part
grown to `ceil(size / 10000)` rounded up to a MiB when a file would exceed 10,000 parts, with no
file size bound).

**Grant wire.** JSON over HTTP relative to the database mount `<server>/<db>` with
`Authorization: Bearer <token>`. A non-2xx answer carries `{"code","message"}` (`401`/`403` maps
to `sync.credential_refused`; `5xx` is retryable):

- `POST /files/upload-grant` `{contentId, size, mediaType?, partBytes, parts:[{number, sha256}]}`
  → `{present:true}` | `{url, expiresAtMs}` (single `PUT`) | `{planId, parts:[{number, url, expiresAtMs}]}`
- `POST /files/plans/<planId>/part-urls` `{numbers}` → `{parts:[{number, url, expiresAtMs}]}`
- `GET /files/plans/<planId>/parts` → `{parts:[{number, size}]}`
- `POST /files/plans/<planId>/complete` `{}` → `{status:"placed"|"verifying"}` (hash mismatch:
  `409` `files.hash_mismatch`)
- `POST /files/download-grant` `{contentId}` → `{url, expiresAtMs}`

Two rules of this wire the client enforces:

- **`planId` grammar.** A `planId` matches `[A-Za-z0-9._-]{1,128}` and is neither `.` nor `..`
  (it names the staging key and goes into the paths above). The client refuses any other
  `planId` and fails the upload; a server must therefore mint its own id, never pass a store's
  multipart `UploadId` through.
- **A forgotten plan.** For a plan the server no longer holds (unknown, aborted by the staging
  sweep, or already completed), `GET …/parts`, `POST …/part-urls` and `POST …/complete` answer
  `404` `{"code":"files.plan_unknown"}`. On that answer to `GET …/parts` (which the client asks
  first when it resumes from a plan file) the client deletes its plan file and asks
  `upload-grant` again; from `part-urls` or `complete` it fails the attempt, and the next
  attempt does the same. Any other answer keeps the plan file, so every later upload of that file
  on that device fails until the plan file is removed by hand.

**Store wire and origin check.** The client sends no credential to the object store:

- single `PUT <url>` with `Content-Length: <size>` and `x-amz-checksum-sha256: <whole-file b64>` → `200`
- part `PUT <url>` with `Content-Length: <part len>` and `x-amz-checksum-sha256: <part b64>` → `200`
  (wrong length or bytes → `400`/`403`)
- `GET <url>` with optional `Range: bytes=<from>-` → `200` or `206` (`Content-Range: bytes <from>-<last>/<size>`);
  an expired URL answers `403`

A presigned URL is only ever contacted if its origin matches either the store origin the server
announced in `GET /databases`'s `store` field (`scheme://host[:port]`, with no path, query or
credential) or the server's own origin; any other origin is refused locally with
`transport.foreign_origin`.

**Plan file.** A multipart upload's plan stays on the device at `<plan_dir>/<hex>.plan.json`
(`{contentId, size, partBytes, planId, parts, fileLen, fileModifiedMs}`), never in the replica
SQLite. It is written after the upload grant, removed on completion, and discarded on resume if
the local file's length or modification time has changed.

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

Which tables a unit covers is the server's own answer (`coveredTables`), never the script's. The
graph line "none across units" has a self-test, `scripts/check-syncular-live.test.ts`: it declares
the four ledger tables under `kg_graph` as well, regenerates that contract beside a copy of the
script, and expects the check to fail on that line. It needs a fresh schema of its own in
`KEEPER_SYNC_TEST_PG_MUTANT_URL` and is skipped without one.

## Jobs

A job is a row in the domain it belongs to, with one claim the server fences (DF/AD-17; no
Temporal). Its three tables change only through the registered `jobs.*` commands; a raw row write
to `jobs`, `job_claims` or `job_capabilities` is refused, service writes included (122.2). `kind` is
a namespaced lowercase name (`<namespace>.<name>`, at most 64 bytes) the contract never
enumerates: kinds are closed in keeper-hq's registry (122.2), and no scope pattern names one
(DF/AD-4). `keeper-jobs-api` is keeper's copy of these types and checks.

**Tables** (`migrations/0012_jobs/up.sql`; no declared reference between `jobs` and `job_claims`,
which move together):

- `jobs`: `id` (ULID), `domain`, `page_key`, `kind`, `input`, `input_blob`, `input_digest`, `runs_on`
  (`server`, `any` or `node:<capability>`), `state` (`queued`, `claimed`, `succeeded`, `failed`,
  `cancelled`, `dead`), `attempt`, `max_attempts`, `not_before_ms`, `plan_blob`, `plan_attempt_id`,
  `note`, `result`, `result_blob`, `effects`, `error_code`, `error_sentence`, `submitter_actor`,
  `submitter_client`, `submitter_context`, `submitted_at_ms`, `updated_at_ms`, `ended_at_ms`,
  `cancel_requested_at_ms`, `revision`, `author_actor`, `author_client`.
- `job_claims`: `job_id` (key), `domain`, `page_key`, `holder_actor`, `holder_client`, `attempt_id`,
  `epoch`, `lease_ms`, `acquired_at_ms`, `renewed_at_ms`, `released`, `revision`, authorship.
- `job_capabilities`: `id` (the client scope, *Ids*), `domain`, `actor_id`, `client_id`,
  `capabilities` (ascending, distinct), `declared_at_ms`, `revision`, authorship.

**Scope and isolation.** While a job has not ended, its `jobs` and `job_claims` rows live under
`jobs_page` = `<domain>/open`. The command that ends it (`jobs.complete`, a `jobs.fail` leaving it
`failed`, `dead` or `cancelled`, `jobs.cancel`, `jobs.release`, or a `jobs.claim` ending it `dead`
or `cancelled`) moves both rows to `<domain>/<yyyy-mm>`, the UTC month it ended in, in the same
commit, by Syncular's explicit scope move: an upsert and a `rescope` of the
same row (0.97 `dist/push.js:215-275`, §2.2: only scope columns may differ; the delete under the
old scopes and the upsert under the new are emitted at the next version). A client subscribing the
current month and `<domain>/open` therefore holds every running job however old, and every job
that ended this month (`scripts/jobs-scope-move.test.ts` runs the move on the real server). The
scope promises **no per-kind isolation**: any reader of the domain, and any worker serving it,
reads every job of the months it subscribes, inputs, plans and results included. A ceiling of
kinds is enforced by the server on commands and byte grants, and by the trusted runtime on reads
(DF/AD-7). A person who does not read a domain receives nothing of its jobs. A terminal state
(`succeeded`, `failed`, `cancelled`, `dead`) never changes.

**Commands.** Every `jobs.*` command's `params` is KG1's envelope `{version: 1, request_id: <ULID>,
context_id: <ULID> | null, payload: {…}}` (*The knowledge graph*, Requests); its digest is the
SHA-256 of the JCS bytes of `{version, descriptor, context_id, payload}`. The receipt is
`{descriptor, request_id, request_digest, outcome: accepted | conflict | rejected, code?,
commit_seq?, attempt_id?, epoch?}`; the same request id with the same digest is answered from the
record, with another digest `idempotency_mismatch`. A judgement that ends a job while refusing the
command (a claim beyond the attempt bound or after a cancel request) is answered `rejected` with
its `code` and the `commit_seq` of the commit that ended the job.

| descriptor | payload | caller | effect |
| --- | --- | --- | --- |
| `jobs.submit` | `{job_id, kind, input?: {refs, params}, input_blob?, runs_on, max_attempts, not_before_ms?}` | person, agent or tool | a `queued` job under `<domain>/open`, with `input_digest`; `input` and `input_blob` exclusive |
| `jobs.cancel` | `{job_id, expected_revision}` | the submitter | a `queued` job ends `cancelled` at once (claim released, both rows moved); a `claimed` job records `cancel_requested_at_ms`, which its current attempt meets at its next `jobs.renew` (refused `cancel_requested`); its `jobs.complete` or `jobs.fail` is still recorded; a stale `expected_revision` is `conflict` |
| `jobs.claim` | `{job_id, expected_claim_revision, lease_ms}` (`0` when no claim row) | a worker | acquires a due job; the receipt carries the new `attempt_id`, `epoch` and `commit_seq`; stale revision `conflict`; a job with a cancel request ends `cancelled` (receipt `cancel_requested`) and one with no attempt left ends `dead` (receipt `attempts_exhausted`), both moving the rows |
| `jobs.renew` | `{job_id, attempt_id}` | the holder | `renewed_at_ms` = the server's clock; refused `cancel_requested` once a cancel is requested |
| `jobs.release` | `{job_id, attempt_id}` | the holder | the claim `released`; the job `queued`, or `dead` at its last attempt, or `cancelled` when a cancel was requested |
| `jobs.progress` | `{job_id, attempt_id, note?, plan_blob?, input_digest}` | the holder | a note and, once, the plan bound to `input_digest`: a second plan `plan_exists`, another digest `input_moved` |
| `jobs.complete` | `{job_id, attempt_id, result? \| result_blob?, effects}` | the holder | ends the job `succeeded`, releases the claim, moves both rows |
| `jobs.fail` | `{job_id, attempt_id, error_code, sentence, retryable, effects?}` | the holder | retryable with attempts left: `queued` (a cancel request turns a re-queue into `cancelled`); otherwise ends `failed` (not retryable) or `dead`, and moves both rows |
| `jobs.declare_capability` | `{capabilities}` | any client | replaces the caller's own `job_capabilities` row (122.5 builds on it) |

**References** (never inline bytes): `{type: row, table, id}`, `{type: job_blob, job_id, column:
input_blob | plan_blob | result_blob}`, `{type: file, entry_id, content_id}` (plain ids, never a
`blob_ref`; 116.1), `{type: graph_node, id, revision}`, `{type: graph_event, id}`, `{type: proposal,
id}`. `input_blob`, `plan_blob` and `result_blob` are `BLOB_REF` columns of the conversation key
space (DF/AD-8's third class), never the files one.

**Effect identity** `(job_id, ordinal)`, carried in the effect where the domain replicates it: a
graph change writes `<job_id>/<ordinal>` as a property (122.3's fixture `test:effect`, a later
model kind the profile's reserved `okf:plan`); a proposal uses the id the plan fixed in advance; a
file change carries `job_effect` `{job_id, ordinal, attempt_id}` in the files envelope's `job`
field (116.1 decision 13). Through `job_id` every effect names its job, and the job its submitter.

**Attempts, epoch and lapse.** `epoch` counts acquisitions and rises on every one: a first claim,
a takeover, a re-claim after release, a restart with the same client, A→B→A. The session claim's
rule (a rise only on a new holder client) is not reused: a same-client restart would keep its
epoch and a stale attempt would pass. **Expiry (Ambiguity 1, settled):** the judge is the server's
own clock against the `renewed_at_ms` it recorded on accepting the attempt's last `jobs.claim` or
`jobs.renew` (lapsed when `now_ms >= renewed_at_ms + lease_ms`), as the turn ledger keeps its own
record; never one host's wall clock against another's, because hosts' clocks differ and one
worker's monotonic clock means nothing to another. A lapsed claim only permits a takeover; until
one, the attempt stays current. The fence is the server-minted `attempt_id`: once another
acquisition mints a new one, the old attempt's writes are refused `stale_attempt`.

**Bounds and codes.** A command is checked in a fixed order: its encoded size (`jobRowMaxBytes`),
the envelope (`unsupported_version` before anything else in it), the payload's shape, then each
bound of `limits.json`. The stable codes: `invalid_request`, `unsupported_version`,
`idempotency_mismatch`, `row_too_large`, `inline_too_large`, `note_too_large`, `too_many_refs`,
`too_many_effects`, `plan_too_large`, `lease_out_of_range`, `attempts_out_of_range`,
`too_many_open`, `job_exists`, `unknown_job`, `terminal`, `conflict`, `not_due`,
`attempts_exhausted`, `stale_attempt`, `plan_exists`, `input_moved`, `invalid_effect`,
`cancel_requested`.
