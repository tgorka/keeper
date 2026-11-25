-- The drive's proposals, the reviewers' inbox and reviews, schema version 14 (the README's
-- *The drive*, *Proposals, decisions and reviews*; story 116.6). 0001-0013 are never re-opened:
-- this migration only adds, and no column of 0010's tables changes.
--
-- A proposal is private metadata: never an event, never a head move. One row per immutable
-- revision, `id` `<proposal_id>/<revision>`: a next revision marks the one it replaces
-- `superseded`; an approval, a rejection or an expiry found at decide time is a transition of that
-- one revision's `status` (`open`, `approved`, `rejected`, `expired`, `superseded`). `change` is
-- one `files.change` payload, `digest` the proposal digest a decision must name, `reviewers` 1 to
-- 16 ascending person actor ids or `{"role": "reviewer"}`, `source_labels` the proposer's labels
-- captured at propose. `kg_proposals`' columns with `domain` for `graph`, plus the change set and
-- the job a job's proposal carries. Scoped by `files_proposal`, the proposer's actor id: the row
-- syncs to its proposer only; a reviewer reads it through `files.proposal`, re-authorised per call.
CREATE TABLE files_proposals (
  id TEXT PRIMARY KEY,
  domain TEXT NOT NULL,
  proposal_id TEXT NOT NULL,
  revision TEXT NOT NULL,
  status TEXT NOT NULL,
  reason TEXT,
  digest TEXT NOT NULL,
  change JSON NOT NULL,
  reviewers JSON NOT NULL,
  expires_at_ms INTEGER,
  proposer_actor TEXT NOT NULL,
  proposer_client TEXT NOT NULL,
  proposer_context TEXT,
  origin_actor TEXT,
  origin_chain JSON,
  source_labels JSON NOT NULL,
  created_at_ms INTEGER NOT NULL,
  decided_at_ms INTEGER,
  decided_by TEXT,
  decided_event_id TEXT,
  change_set_id TEXT,
  job_id TEXT,
  job_ordinal INTEGER,
  job_attempt_id TEXT,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE UNIQUE INDEX files_proposals_revision ON files_proposals (proposal_id, revision);
CREATE INDEX files_proposals_proposer ON files_proposals (proposer_actor);
CREATE INDEX files_proposals_change_set ON files_proposals (change_set_id);

-- The reviewer's inbox: one row per reviewer and revision, written with the revision (for a
-- revision addressed to the role, one per person holding `reviewer` or `owner` then; nothing
-- back-fills a later holder) and moved to the revision's `status` by every decision and
-- supersession. Ids and status only, never content. Scoped by `files_reviewer`, the reviewer's
-- actor id. An expired revision's row stays `open`: a client hides it by `expires_at_ms`.
CREATE TABLE files_proposal_reviewers (
  id TEXT PRIMARY KEY,
  domain TEXT NOT NULL,
  reviewer_actor TEXT NOT NULL,
  proposal_id TEXT NOT NULL,
  revision TEXT NOT NULL,
  status TEXT NOT NULL,
  proposer_actor TEXT NOT NULL,
  change_set_id TEXT,
  expires_at_ms INTEGER,
  op_count INTEGER NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE INDEX files_proposal_reviewers_revision ON files_proposal_reviewers (proposal_id, revision);

-- A review of one accepted event: an acknowledgement or a reversal, inserted in the same
-- transaction as the accepted `review` event operation whose after-image it equals, and never
-- updated or deleted. `kg_reviews`' columns with `domain` for `graph`, plus `page_key`, its own
-- event's (`files_page`), so the review travels with the event that created it. No reference is
-- declared to the target: it may sit under another page key.
CREATE TABLE files_reviews (
  id TEXT PRIMARY KEY,
  domain TEXT NOT NULL,
  page_key TEXT NOT NULL,
  target_event_id TEXT NOT NULL,
  target_delta_digest TEXT NOT NULL,
  kind TEXT NOT NULL,
  reviewer_actor TEXT NOT NULL,
  revision TEXT NOT NULL,
  created_event_id TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

-- The reviews of one event, by kind.
CREATE INDEX files_reviews_target ON files_reviews (target_event_id, kind);
