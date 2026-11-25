-- Jobs and their claims, schema version 12 (the README's *Jobs*; story 122.1). 0001-0011 are
-- never re-opened: this migration only adds.
--
-- A job is a row in the domain it belongs to. It changes only through the server's `jobs.*`
-- commands; a raw row write of `jobs`, `job_claims` or `job_capabilities` is refused, service
-- writes included. `kind` is a namespaced string the contract never enumerates. A job's bytes are
-- never inline above `limits.json`'s bounds: `input_blob`, `plan_blob` and `result_blob` are
-- `BLOB_REF`s of the conversation key space; a file is named by entry id and content id as plain
-- strings inside `input`, never through a blob reference.
--
-- Scope columns: `page_key` (`jobs_page`: `<domain>/open` while the job has not ended, then
-- `<domain>/<yyyy-mm>` of the UTC month it ended in, moved by an explicit `rescope` in the command
-- that ends it) for jobs and claims; `domain` (`jobs_nodes`) for capability declarations. No
-- reference is declared between `jobs` and `job_claims`: both move in the same command.

-- One job. `state` is `queued`, `claimed`, `succeeded`, `failed`, `cancelled` or `dead`; the last
-- four are terminal and never change. `runs_on` is `server`, `any` or `node:<capability>`.
-- `attempt` counts acquisitions so far, at most `max_attempts`. `input_digest` is the SHA-256 of
-- the canonical input (or of the input blob's reference) a plan is bound to. `plan_blob` is written
-- once, by `jobs.progress`, with the attempt that wrote it. `effects` is what `jobs.complete` or
-- `jobs.fail` reported, each effect carrying its own identity `(job_id, ordinal)`.
-- `cancel_requested_at_ms` is the server's clock when `jobs.cancel` reached a `claimed` job: the
-- request is recorded and the current attempt honours it at its next renewal; a `queued` job is
-- cancelled at once and never carries one.
CREATE TABLE jobs (
  id TEXT PRIMARY KEY,
  domain TEXT NOT NULL,
  page_key TEXT NOT NULL,
  kind TEXT NOT NULL,
  input JSON,
  input_blob BLOB_REF,
  input_digest TEXT NOT NULL,
  runs_on TEXT NOT NULL,
  state TEXT NOT NULL,
  attempt INTEGER NOT NULL,
  max_attempts INTEGER NOT NULL,
  not_before_ms INTEGER NOT NULL,
  plan_blob BLOB_REF,
  plan_attempt_id TEXT,
  note TEXT,
  result JSON,
  result_blob BLOB_REF,
  effects JSON,
  error_code TEXT,
  error_sentence TEXT,
  submitter_actor TEXT NOT NULL,
  submitter_client TEXT NOT NULL,
  submitter_context TEXT,
  submitted_at_ms INTEGER NOT NULL,
  updated_at_ms INTEGER NOT NULL,
  ended_at_ms INTEGER,
  cancel_requested_at_ms INTEGER,
  revision INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

-- The open jobs of a domain (its bound and a worker's queue), and a month's listing.
CREATE INDEX jobs_domain_state ON jobs (domain, state, not_before_ms);
CREATE INDEX jobs_page_key ON jobs (page_key);

-- The claim of one job, keyed by the job. `attempt_id` is a ULID the server mints on every
-- acquisition; `epoch` is the acquisition count and rises on every acquisition (a first claim, a
-- takeover, a re-claim after release, a restart with the same client, A then B then A). The
-- session claim's rule (`claims`, epoch rising only when the holder client changes) is not reused.
-- `renewed_at_ms` is the server's own clock when it accepted the last `jobs.claim` or `jobs.renew`.
CREATE TABLE job_claims (
  job_id TEXT PRIMARY KEY,
  domain TEXT NOT NULL,
  page_key TEXT NOT NULL,
  holder_actor TEXT NOT NULL,
  holder_client TEXT NOT NULL,
  attempt_id TEXT NOT NULL,
  epoch INTEGER NOT NULL,
  lease_ms INTEGER NOT NULL,
  acquired_at_ms INTEGER NOT NULL,
  renewed_at_ms INTEGER NOT NULL,
  released BOOLEAN NOT NULL,
  revision INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX job_claims_page_key ON job_claims (page_key);

-- What one client says it can run (`jobs.declare_capability`), keyed by its client scope
-- (`<a>:<actor_id><c>:<client_id>`, the README's *Ids*). `capabilities` is the ascending, distinct
-- list of capability names a `node:<capability>` job may ask for.
CREATE TABLE job_capabilities (
  id TEXT PRIMARY KEY,
  domain TEXT NOT NULL,
  actor_id TEXT NOT NULL,
  client_id TEXT NOT NULL,
  capabilities JSON NOT NULL,
  declared_at_ms INTEGER NOT NULL,
  revision INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);
