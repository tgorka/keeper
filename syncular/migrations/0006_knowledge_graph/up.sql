-- The knowledge graph, schema version 6: one graph per database (its domain), written only by the
-- server's graph commands (the README's *The knowledge graph*). Every graph row's scope column is
-- `graph`, the name of the database it belongs to; a receipt's is `client_scope`, the client
-- scope of the actor and client whose request it answers. No other scope variable reaches these
-- tables.
--
-- Entity revisions and graph sequences are server-assigned decimal strings. `properties` and the
-- operation values are canonical JSON (`knowledge-graph.vectors.json`). Nothing here is ever
-- deleted except a `kg_node_tags` membership, whose removal is an explicit operation of an event.

-- One accepted change set: the header of an event whose operations are `kg_event_ops`.
CREATE TABLE kg_events (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
  graph_seq TEXT NOT NULL,
  descriptor TEXT NOT NULL,
  request_id TEXT NOT NULL,
  request_digest TEXT NOT NULL,
  actor_id TEXT NOT NULL,
  client_id TEXT NOT NULL,
  origin_class TEXT NOT NULL,
  origin_actor TEXT,
  context_id TEXT,
  approval_basis JSON,
  reverses_event_id TEXT,
  event_time_ms INTEGER,
  recorded_at_ms INTEGER NOT NULL,
  op_count INTEGER NOT NULL,
  delta_digest TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE UNIQUE INDEX kg_events_graph_seq ON kg_events (graph, graph_seq);
CREATE INDEX kg_events_request ON kg_events (request_id);
CREATE INDEX kg_events_reverses ON kg_events (reverses_event_id);

-- The ordered operations of one event: `id` is `<event_id>/<ordinal>`.
CREATE TABLE kg_event_ops (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
  event_id TEXT NOT NULL REFERENCES kg_events(id) ON DELETE RESTRICT,
  ordinal INTEGER NOT NULL,
  entity_type TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  operation TEXT NOT NULL,
  before_revision TEXT,
  after_revision TEXT,
  before_value JSON,
  after_value JSON,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE UNIQUE INDEX kg_event_ops_event_ordinal ON kg_event_ops (event_id, ordinal);
CREATE INDEX kg_event_ops_entity ON kg_event_ops (entity_id);

-- The accepted head of the database's graph: one row, `id` the database's name.
CREATE TABLE kg_head (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
  graph_seq TEXT NOT NULL,
  event_id TEXT NOT NULL REFERENCES kg_events(id) ON DELETE RESTRICT,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE TABLE kg_nodes (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
  kind TEXT NOT NULL,
  title TEXT NOT NULL,
  description TEXT,
  description_source_id TEXT,
  properties JSON NOT NULL,
  lifecycle TEXT NOT NULL,
  merged_into_id TEXT,
  revision TEXT NOT NULL,
  lifecycle_revision TEXT NOT NULL,
  created_event_id TEXT NOT NULL,
  updated_event_id TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX kg_nodes_kind ON kg_nodes (kind);
CREATE INDEX kg_nodes_merged_into_id ON kg_nodes (merged_into_id);

CREATE TABLE kg_edges (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
  from_id TEXT NOT NULL REFERENCES kg_nodes(id) ON DELETE RESTRICT,
  to_id TEXT NOT NULL REFERENCES kg_nodes(id) ON DELETE RESTRICT,
  type TEXT NOT NULL,
  properties JSON NOT NULL,
  lifecycle TEXT NOT NULL,
  revision TEXT NOT NULL,
  created_event_id TEXT NOT NULL,
  updated_event_id TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX kg_edges_from ON kg_edges (from_id, type);
CREATE INDEX kg_edges_to ON kg_edges (to_id, type);

-- A tag: `text` its canonical form (one per graph), `label` an optional display label.
CREATE TABLE kg_tags (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
  text TEXT NOT NULL,
  label TEXT,
  revision TEXT NOT NULL,
  created_event_id TEXT NOT NULL,
  updated_event_id TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX kg_tags_text ON kg_tags (graph, text);

-- One node's membership of one tag; a change of membership advances the node's revision.
CREATE TABLE kg_node_tags (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
  node_id TEXT NOT NULL REFERENCES kg_nodes(id) ON DELETE RESTRICT,
  tag_id TEXT NOT NULL REFERENCES kg_tags(id) ON DELETE RESTRICT,
  created_event_id TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX kg_node_tags_node_id ON kg_node_tags (node_id);
CREATE INDEX kg_node_tags_tag_id ON kg_node_tags (tag_id);

-- The terminal outcome of one graph request, readable only by the client that sent it:
-- `id` is `<descriptor>/<request_id>/<client_scope>`.
CREATE TABLE kg_receipts (
  id TEXT PRIMARY KEY,
  client_scope TEXT NOT NULL,
  descriptor TEXT NOT NULL,
  request_id TEXT NOT NULL,
  request_digest TEXT NOT NULL,
  outcome TEXT NOT NULL,
  reason TEXT,
  event_id TEXT,
  graph_seq TEXT,
  detail JSON NOT NULL,
  recorded_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX kg_receipts_request_id ON kg_receipts (request_id);
