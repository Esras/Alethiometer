-- What the source said an item's revision was when it was last applied
-- (Jira `updated`, GitHub `updated_at` + comment count, a calendar etag).
-- A poll that sees the same revision again changes nothing, so connectors
-- can re-read an overlap window safely (app/docs/db-plan.md, Phase 2).
ALTER TABLE items ADD COLUMN remote_rev TEXT;
