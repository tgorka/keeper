-- schema 4: always-allow grants, a session's model and effort, a session's drives.

-- What an agent instance publishes beside `tools`: the models a person may pick for a session of
-- it (its definition's ceiling, a JSON array of model ids) and the one it uses when nobody picked
-- (`default_model`); the reasoning efforts it can send (a JSON array, each one of
-- `vocabulary.json`'s `efforts`) and the one it sends when nobody picked (`default_effort`, absent
-- when it sends none); and the drives it can read (a JSON array of drive names, its definition's
-- ceiling less those its container does not mount). Absent: the instance has not said, and a
-- client offers no picker.

ALTER TABLE agent_profiles ADD COLUMN models JSON;
ALTER TABLE agent_profiles ADD COLUMN default_model TEXT;
ALTER TABLE agent_profiles ADD COLUMN efforts JSON;
ALTER TABLE agent_profiles ADD COLUMN default_effort TEXT;
ALTER TABLE agent_profiles ADD COLUMN drives JSON;

-- An approval born approved names the standing grant that decided it. Absent on every approval a
-- person decides.

ALTER TABLE approvals ADD COLUMN grant_id TEXT;

-- session: one row per session, written by a person member.

CREATE TABLE session_settings (
  session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
  model TEXT,
  effort TEXT,
  updated_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX session_settings_session_id ON session_settings (session_id);

CREATE TABLE session_drives (
  session_id TEXT PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE,
  drives JSON NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX session_drives_session_id ON session_drives (session_id);

-- pair: one person's standing decisions about one agent, `<person_actor>/<agent_actor>`.

CREATE TABLE standing_grants (
  id TEXT PRIMARY KEY,
  pair_key TEXT NOT NULL,
  person_actor TEXT NOT NULL,
  agent_actor TEXT NOT NULL,
  tool TEXT NOT NULL,
  args_match JSON NOT NULL,
  max_tier TEXT NOT NULL,
  expires_at_ms INTEGER,
  revoked_at_ms INTEGER,
  granted_by_client TEXT NOT NULL,
  created_at_ms INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX standing_grants_pair ON standing_grants (pair_key, created_at_ms, id);
