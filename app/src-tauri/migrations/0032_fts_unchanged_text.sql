-- Full-text index: a sync upsert sets title and body on every item it
-- reads, usually to what they already were. `UPDATE OF` fires on that too,
-- so the index entry was deleted and rewritten each time. Only a real
-- change rewrites it now.
DROP TRIGGER items_au;
CREATE TRIGGER items_au AFTER UPDATE OF title, body ON items
WHEN OLD.title IS NOT NEW.title OR OLD.body IS NOT NEW.body
BEGIN
  INSERT INTO items_fts(items_fts, rowid, title, body) VALUES ('delete', old.id, old.title, old.body);
  INSERT INTO items_fts(rowid, title, body) VALUES (new.id, new.title, new.body);
END;
