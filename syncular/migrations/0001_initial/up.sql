-- The agent-conversation contract, schema version 1. typegen reads this file
-- for the shape of every synced table (columns, types, primary keys, indexes,
-- declared references); the server builds its own Postgres projection from
-- the compiled schema and never executes this SQL. Value rules that SQL here
-- cannot state (closed value sets, id shapes, who may write what) are the
-- server's write validators, written down in ../../README.md.
--
-- Every row carries author_actor and author_client: the actor and the client
-- of the commit that last wrote it. A commit names its actor on the wire but
-- not its client, and a bootstrap names neither.

-- dir: the directory of the database this server serves.

CREATE TABLE actors (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  display_name TEXT NOT NULL,
  family TEXT,
  dir TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE TABLE clients (
  id TEXT PRIMARY KEY,
  actor_id TEXT NOT NULL REFERENCES actors(id) ON DELETE RESTRICT,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  host TEXT,
  created_at_ms INTEGER NOT NULL,
  revoked_at_ms INTEGER,
  dir TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX clients_actor_id ON clients (actor_id);

CREATE TABLE agent_profiles (
  id TEXT PRIMARY KEY,
  actor_id TEXT NOT NULL REFERENCES actors(id) ON DELETE RESTRICT,
  name TEXT NOT NULL,
  description TEXT NOT NULL,
  avatar_blob BLOB_REF,
  definition_hash TEXT NOT NULL,
  image_digest TEXT NOT NULL,
  started_at_ms INTEGER NOT NULL,
  dir TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX agent_profiles_actor_id ON agent_profiles (actor_id);

-- session: one conversation and the rows every member of it reads.

CREATE TABLE sessions (
  id TEXT PRIMARY KEY,
  title TEXT NOT NULL,
  status TEXT NOT NULL,
  agent_actor TEXT NOT NULL,
  created_by_actor TEXT NOT NULL,
  created_by_client TEXT NOT NULL,
  preferred_client TEXT,
  parent_session TEXT,
  invite JSON NOT NULL,
  archived BOOLEAN NOT NULL,
  heard_until JSON,
  created_at_ms INTEGER NOT NULL,
  last_activity_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX sessions_parent_session ON sessions (parent_session);

CREATE TABLE session_members (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  actor_id TEXT NOT NULL,
  role TEXT NOT NULL,
  reads_trace BOOLEAN NOT NULL,
  added_by_actor TEXT NOT NULL,
  added_at_ms INTEGER NOT NULL,
  read_until INTEGER,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX session_members_session_id ON session_members (session_id);
CREATE INDEX session_members_actor_id ON session_members (actor_id);

CREATE TABLE claims (
  session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
  holder_actor TEXT NOT NULL,
  holder_client TEXT NOT NULL,
  epoch INTEGER NOT NULL,
  renewed_at_ms INTEGER NOT NULL,
  released BOOLEAN NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX claims_session_id ON claims (session_id);

CREATE TABLE session_tool_switches (
  session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
  tools_off JSON NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX session_tool_switches_session_id ON session_tool_switches (session_id);

CREATE TABLE turn_chunks (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  turn_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  text TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX turn_chunks_session_id ON turn_chunks (session_id);
CREATE UNIQUE INDEX turn_chunks_turn_seq ON turn_chunks (turn_id, seq);

CREATE TABLE attachments (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
  message_id TEXT NOT NULL,
  blob_ref BLOB_REF NOT NULL,
  name TEXT NOT NULL,
  mime TEXT NOT NULL,
  size INTEGER NOT NULL,
  created_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX attachments_session_id ON attachments (session_id);
CREATE INDEX attachments_message_id ON attachments (message_id);

-- page: a session's history, one scope value per session and month.

CREATE TABLE messages (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  page_key TEXT NOT NULL,
  turn_id TEXT,
  role TEXT NOT NULL,
  body TEXT NOT NULL,
  body_blob BLOB_REF,
  metadata JSON,
  created_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX messages_page ON messages (page_key, created_at_ms, id);
CREATE INDEX messages_session_id ON messages (session_id);

-- trace: what only a session's readers receive, keyed like page.

CREATE TABLE reasoning (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  page_key TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  compacted BOOLEAN NOT NULL,
  text TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX reasoning_page ON reasoning (page_key, created_at_ms, id);
CREATE INDEX reasoning_turn ON reasoning (turn_id, seq);

CREATE TABLE tool_calls (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  page_key TEXT NOT NULL,
  turn_id TEXT NOT NULL,
  name TEXT NOT NULL,
  args JSON NOT NULL,
  status TEXT NOT NULL,
  result TEXT,
  result_blob BLOB_REF,
  started_at_ms INTEGER,
  ended_at_ms INTEGER,
  created_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX tool_calls_page ON tool_calls (page_key, created_at_ms, id);
CREATE INDEX tool_calls_turn_id ON tool_calls (turn_id);

CREATE TABLE approvals (
  id TEXT PRIMARY KEY,
  session_id TEXT NOT NULL,
  page_key TEXT NOT NULL,
  tool_call_id TEXT NOT NULL REFERENCES tool_calls(id) ON DELETE RESTRICT,
  tier TEXT NOT NULL,
  binding_digest TEXT NOT NULL,
  summary TEXT NOT NULL,
  approvers JSON NOT NULL,
  state TEXT NOT NULL,
  decided_by_actor TEXT,
  decided_by_client TEXT,
  consumed_by_client TEXT,
  expires_at_ms INTEGER NOT NULL,
  created_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX approvals_page ON approvals (page_key, created_at_ms, id);
CREATE INDEX approvals_tool_call_id ON approvals (tool_call_id);

-- actor: one person's own settings.

CREATE TABLE instance_prefs (
  id TEXT PRIMARY KEY,
  person_actor TEXT NOT NULL,
  agent_actor TEXT NOT NULL,
  default_client TEXT NOT NULL,
  fallback JSON NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

-- agent + family: an agent's memory and its family's shared knowledge.

CREATE TABLE agent_memory (
  id TEXT PRIMARY KEY,
  agent_actor TEXT NOT NULL,
  family TEXT NOT NULL,
  kind TEXT NOT NULL,
  text TEXT NOT NULL,
  source_session TEXT,
  created_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX agent_memory_agent_kind ON agent_memory (agent_actor, kind);
