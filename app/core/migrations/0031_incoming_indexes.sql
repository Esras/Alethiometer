-- Incoming's new-person suggestions read recent items by time and match
-- their authors to people by (person, source kind): both were scans of every
-- work item on a 200k-item database (app/docs/db-plan.md, Phase 4).
CREATE INDEX items_occurred ON items(occurred_at);
CREATE INDEX person_handles_person ON person_handles(person_id, source_kind);
