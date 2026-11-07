-- The knowledge graph's proposals and reviews, schema version 8 (the README's *The knowledge
-- graph*). 0006 and 0007 are never re-opened: this migration only adds.
--
-- A proposal is private metadata: never part of graph replay, never a graph head increment.
-- One row per immutable proposal revision, `id` `<proposal_id>/<revision>`: editing a proposal
-- adds the next revision and marks the one it replaces `superseded`; an approval, rejection or
-- expiry is a persisted transition of that one revision's `status`. `change` is the canonical
-- `graph.apply` payload it would accept, `digest` the proposal digest a decision must name,
-- `reviewers` the ascending person actor ids that may decide that revision, `source_labels` the
-- proposer's source labels captured at propose. Scoped by `kg_proposal`, the proposer's actor id:
-- the row syncs to its proposer only, and an enumerated reviewer reads it through an authorised
-- remote query, never a subscription.
CREATE TABLE kg_proposals (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
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
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

CREATE UNIQUE INDEX kg_proposals_revision ON kg_proposals (proposal_id, revision);
CREATE INDEX kg_proposals_proposer ON kg_proposals (proposer_actor);

-- A review of one accepted event: an acknowledgement or a reversal, inserted in the same
-- transaction as the accepted `review` event operation whose after-image it equals, and never
-- updated or deleted. `target_event_id` and `target_delta_digest` name the reviewed event; a
-- reversal's event also carries `reverses_event_id` equal to its target. Scoped by `graph`, like
-- every other graph row.
CREATE TABLE kg_reviews (
  id TEXT PRIMARY KEY,
  graph TEXT NOT NULL,
  target_event_id TEXT NOT NULL REFERENCES kg_events(id) ON DELETE RESTRICT,
  target_delta_digest TEXT NOT NULL,
  kind TEXT NOT NULL,
  reviewer_actor TEXT NOT NULL,
  revision TEXT NOT NULL,
  created_event_id TEXT NOT NULL,
  author_actor TEXT NOT NULL,
  author_client TEXT NOT NULL
);

-- The reviews of one event: the unreviewed feed is the agent-origin events with none.
CREATE INDEX kg_reviews_target ON kg_reviews (target_event_id, kind);

-- An owner's contents in id order, read with a bound (`ORDER BY id LIMIT n`): the owner-only
-- `kg_contents_owner` of 0007 sorts every row of the owner first. 0007 is never re-opened, so
-- that index stays.
CREATE INDEX kg_contents_owner_id ON kg_contents (owner_node_id, id);
