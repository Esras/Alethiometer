CREATE TABLE item_field_changes (
  id         INTEGER PRIMARY KEY,
  item_id    INTEGER NOT NULL REFERENCES items(id) ON DELETE CASCADE,
  ext_key    TEXT NOT NULL,
  at         TEXT NOT NULL,
  author     TEXT NOT NULL,
  author_id  TEXT,
  field      TEXT NOT NULL,
  from_text  TEXT,
  to_text    TEXT,
  read       INTEGER NOT NULL DEFAULT 0,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  UNIQUE (item_id, ext_key)
);

CREATE TRIGGER items_rev_field_change AFTER INSERT ON item_field_changes
BEGIN
  UPDATE items SET rev = rev + 1 WHERE id = NEW.item_id;
END;
