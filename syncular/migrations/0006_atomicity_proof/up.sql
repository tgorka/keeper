-- Story 110.2 disposable feasibility contract. Never deploy or reserve this version.
-- Graph sequence is application order; transport commitSeq is returned separately.
CREATE TABLE kg_nodes (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
  value JSON NOT NULL,
  revision TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);
CREATE TABLE kg_edges (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
  from_id TEXT NOT NULL REFERENCES kg_nodes(id) ON DELETE RESTRICT,
  to_id TEXT NOT NULL REFERENCES kg_nodes(id) ON DELETE RESTRICT,
  value JSON NOT NULL,
  revision TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);
-- Tag membership: removable rows, so history carries explicit removal deltas
-- without ever deleting a canonical node.
CREATE TABLE kg_node_tags (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
  node_id TEXT NOT NULL REFERENCES kg_nodes(id) ON DELETE RESTRICT,
  tag TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);
CREATE TABLE kg_events (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
  graph_seq TEXT NOT NULL,
  request_id TEXT NOT NULL,
  digest TEXT NOT NULL,
  op_count INTEGER NOT NULL,
  delta_digest TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);
CREATE TABLE kg_event_ops (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
  event_id TEXT NOT NULL REFERENCES kg_events(id) ON DELETE RESTRICT,
  ordinal INTEGER NOT NULL,
  delta JSON NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);
CREATE INDEX kg_event_ops_event ON kg_event_ops(event_id);
CREATE TABLE kg_head (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
  graph_seq TEXT NOT NULL,
  event_id TEXT NOT NULL REFERENCES kg_events(id) ON DELETE RESTRICT,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);
CREATE TABLE kg_receipts (
  id TEXT PRIMARY KEY,
  client_scope TEXT NOT NULL,
  request_id TEXT NOT NULL,
  digest TEXT NOT NULL,
  outcome TEXT NOT NULL,
  detail JSON NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);
