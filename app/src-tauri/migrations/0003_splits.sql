-- Topic-aware splits (queries/split.rs). A signal holding several messages can
-- be split: some messages move to a child item with its own topic link.
-- parent_id always points at the root item a family was split from (flat, one level).
ALTER TABLE items ADD COLUMN parent_id INTEGER REFERENCES items(id);
CREATE INDEX items_parent ON items(parent_id) WHERE parent_id IS NOT NULL;
-- Stable per-message key within its source (Slack: the message ts, from
-- raw_json.msgs; otherwise a content hash). Sync skips keys a child owns, so a
-- moved message never comes back to the parent.
ALTER TABLE item_messages ADD COLUMN ext_key TEXT;
CREATE INDEX item_messages_item ON item_messages(item_id);
