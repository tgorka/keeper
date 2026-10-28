-- the identity of each tick (109.28, R605-14): a JSON object from each ticked drive's name to the
-- ULID of the write that ticked it. A client mints a fresh id when it ticks a drive, keeps the ids
-- of the drives it leaves ticked, and drops the id of a drive it unticks, in the same row write.
-- Absent, or a ticked drive it does not name: a tick a schema-4 client or the server wrote, with
-- no identity. A person's device warns of a tick until it has shown that tick's warning.

ALTER TABLE session_drives ADD COLUMN tick_ids JSON;
