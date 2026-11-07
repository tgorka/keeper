-- The knowledge graph's stable content, schema version 7 (the README's *The knowledge graph*).
-- 0006 is never re-opened: this migration only adds.
--
-- A content belongs to one owner node for good: its `id` and `owner_node_id` never change, an
-- update replaces `body` whole and moves `revision` by exactly one, and nothing else moves (not the
-- owner node's revision). Content has no lifecycle: its owner node's lifecycle governs it. `body`
-- is canonical JSON, either `{"inline_text": <at most 32 KiB of UTF-8>}` or
-- `{"reference": {"resource_node_id": <ULID>, "observed_revision": <decimal> | null}}`; a
-- reference never holds locator or body bytes. Written only by the server's graph commands, like
-- every other graph row, and scoped by `graph`, the name of the database it belongs to.
CREATE TABLE kg_contents (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
  owner_node_id TEXT NOT NULL REFERENCES kg_nodes(id) ON DELETE RESTRICT,
  revision TEXT NOT NULL,
  body JSON NOT NULL,
  created_event_id TEXT NOT NULL,
  updated_event_id TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

-- An owner's contents, read with a bound (an assertion's verification lists them all).
CREATE INDEX kg_contents_owner ON kg_contents (owner_node_id);

-- A node's active outgoing edges of one type, read with a bound whose scan work archived history
-- does not grow (an assertion's verification reads its active `sources` edges). A composite index,
-- not a partial one: the schema generator refuses `WHERE` on an index.
CREATE INDEX kg_edges_from_active ON kg_edges (from_id, type, lifecycle);
