-- The drive's manifest, schema version 10 (the README's *The drive*; story 116.1). 0001-0009 are
-- never re-opened: this migration only adds.
--
-- A drive's tree is rows: an entry sits in its parent `dir` by `parent_id`, `name` and the stored
-- caseless fold `name_fold`; a drive's root is a `dir` whose `parent_id` is its own id. No row
-- holds a path. Rows are written only by the server's `files.*` commands. Entry revisions and
-- sequences are server-assigned decimal strings; ids are ULIDs. A content id is the plain text
-- `sha256:<64 lowercase hex>` of the whole content, never a `blob_ref`: it names an object in the
-- files key space, which Syncular's blob checks never see. Nothing is ever physically deleted.
--
-- Scope columns: `drive_key` (`files_drive`, `<domain>/<drive>`) for entries; `page_key`
-- (`files_page`, `<domain>/<yyyy-mm>` or `<domain>/import`) for events and operations; `domain`
-- (`files_domain`) for the head and (`files_sources`) for entry sources; `client_scope`
-- (`files_client`) for receipts. No reference crosses scope patterns.

-- One entry of a drive: a `file` or a `dir`. A `dir` has null `content_id`, `size` and
-- `media_type`. `mode` is `normal` or `executable`; `lifecycle` is `live` or `deleted`.
CREATE TABLE files_entries (
  id TEXT PRIMARY KEY,
  domain TEXT NOT NULL,
  drive_key TEXT NOT NULL,
  parent_id TEXT NOT NULL,
  name TEXT NOT NULL,
  name_fold TEXT NOT NULL,
  kind TEXT NOT NULL,
  content_id TEXT,
  size INTEGER,
  media_type TEXT,
  mtime_ms INTEGER,
  mode TEXT NOT NULL,
  revision TEXT NOT NULL,
  lifecycle TEXT NOT NULL,
  created_event_id TEXT NOT NULL,
  updated_event_id TEXT NOT NULL,
  updated_seq TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

-- A folder's listing, and the one exact lookup the fold rule makes.
CREATE INDEX files_entries_parent ON files_entries (parent_id);
CREATE INDEX files_entries_fold ON files_entries (parent_id, name_fold, lifecycle);
-- The files module's sweep keeps every content id an entry names, whatever its lifecycle.
CREATE INDEX files_entries_content ON files_entries (content_id);

-- Reserved (116.1 decision 12): the graph sources one entry revision's content carries. Holds no
-- row until 120.2 defines who writes it; never allowed to a person.
CREATE TABLE files_entry_sources (
  id TEXT PRIMARY KEY,
  domain TEXT NOT NULL,
  revision TEXT NOT NULL,
  sources JSON NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

-- One accepted change: the header of an event whose operations are `files_event_ops`.
-- `context_id` and `approval_id` are reserved (null until 120.2); the job columns are null
-- unless `origin_class` is `job`.
CREATE TABLE files_events (
  id TEXT PRIMARY KEY,
  domain TEXT NOT NULL,
  seq TEXT NOT NULL,
  page_key TEXT NOT NULL,
  descriptor TEXT NOT NULL,
  request_id TEXT NOT NULL,
  request_digest TEXT NOT NULL,
  actor_id TEXT NOT NULL,
  client_id TEXT NOT NULL,
  origin_class TEXT NOT NULL,
  origin_actor TEXT,
  origin_chain JSON,
  context_id TEXT,
  approval_id TEXT,
  approval_basis JSON NOT NULL,
  proposal_id TEXT,
  change_set_id TEXT,
  job_id TEXT,
  job_ordinal INTEGER,
  job_attempt_id TEXT,
  reverses_event_id TEXT,
  event_time_ms INTEGER,
  recorded_at_ms INTEGER NOT NULL,
  op_count INTEGER NOT NULL,
  delta_digest TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE UNIQUE INDEX files_events_domain_seq ON files_events (domain, seq);
CREATE INDEX files_events_request ON files_events (request_id);
CREATE INDEX files_events_reverses ON files_events (reverses_event_id);
CREATE INDEX files_events_change_set ON files_events (change_set_id);
CREATE INDEX files_events_proposal ON files_events (proposal_id);
CREATE INDEX files_events_job ON files_events (job_id, job_ordinal);
-- History by actor and by origin (116.3).
CREATE INDEX files_events_actor ON files_events (domain, actor_id, seq);
CREATE INDEX files_events_origin ON files_events (domain, origin_class, seq);

-- The ordered operations of one event: `id` is `<event_id>/<ordinal>`. `before_value` and
-- `after_value` hold only the fields the operation changed; the content ids beside them are the
-- columns the files module's sweep reads to keep every earlier version's bytes.
CREATE TABLE files_event_ops (
  id TEXT PRIMARY KEY,
  domain TEXT NOT NULL,
  event_id TEXT NOT NULL REFERENCES files_events(id) ON DELETE RESTRICT,
  ordinal INTEGER NOT NULL,
  page_key TEXT NOT NULL,
  entity_type TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  operation TEXT NOT NULL,
  before_revision TEXT,
  after_revision TEXT,
  before_value JSON,
  after_value JSON,
  before_content_id TEXT,
  after_content_id TEXT,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE UNIQUE INDEX files_event_ops_event_ordinal ON files_event_ops (event_id, ordinal);
-- An entry's history (116.3).
CREATE INDEX files_event_ops_entity ON files_event_ops (entity_id);

-- The accepted head of the domain's manifest: one row, `id` the domain. `digest` is the newest
-- event's `delta_digest`; `drives` the domain's live drives, each name with its root entry id.
-- `event_id` declares no reference: events sit under another scope variable.
CREATE TABLE files_head (
  id TEXT PRIMARY KEY,
  domain TEXT NOT NULL,
  seq TEXT NOT NULL,
  event_id TEXT NOT NULL,
  digest TEXT NOT NULL,
  drives JSON NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

-- The terminal outcome of one files request, readable only by the client that sent it:
-- `id` is `<descriptor>/<request_id>/<client_scope>`.
CREATE TABLE files_receipts (
  id TEXT PRIMARY KEY,
  client_scope TEXT NOT NULL,
  descriptor TEXT NOT NULL,
  request_id TEXT NOT NULL,
  request_digest TEXT NOT NULL,
  outcome TEXT NOT NULL,
  reason TEXT,
  event_id TEXT,
  seq TEXT,
  detail JSON NOT NULL,
  recorded_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX files_receipts_request_id ON files_receipts (request_id);
