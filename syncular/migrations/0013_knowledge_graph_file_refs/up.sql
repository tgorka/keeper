-- Knowledge graph file-entry bindings, schema version 13 (the README's *The knowledge graph*,
-- **Files**; story 116.5). 0001-0012 are never re-opened: this migration only adds.
--
-- `kg_file_refs` is part of the graph's state (`kg_graph`, column `graph`). It is written only in
-- the graph command's own commit, never as an event operation, never by a raw sync write (service
-- writes included) and never by a `files.*` command. At most one row exists per entry, naming the
-- node the one rule gives (H-10): the active node counting for the entry, else the one archived
-- last.

CREATE TABLE kg_file_refs (
  -- File entry id (`files_entries.id`).
  entry_id TEXT PRIMARY KEY,
  -- Scope column (`kg_graph`): the name of the database the graph belongs to.
  graph TEXT NOT NULL,
  -- The graph node bound to the entry.
  node_id TEXT NOT NULL REFERENCES kg_nodes(id) ON DELETE RESTRICT,
  -- Kind of `node_id`: `Resource` or `Collection`.
  node_kind TEXT NOT NULL
);
