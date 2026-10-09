-- A claimed row is leased: if the sender dies mid-send, the row is taken back
-- once the lease runs out (app/docs/db-plan.md, Phase 3).
ALTER TABLE outbox ADD COLUMN lease_until TEXT;
