-- Sync and writeback bookkeeping.
ALTER TABLE sources ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;
ALTER TABLE sources ADD COLUMN last_error TEXT;
ALTER TABLE outbox ADD COLUMN item_id INTEGER REFERENCES items(id);
ALTER TABLE outbox ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE outbox ADD COLUMN receipt_json TEXT;
ALTER TABLE outbox ADD COLUMN sent_at TEXT;
CREATE INDEX outbox_state ON outbox(state);
