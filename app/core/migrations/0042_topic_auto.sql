-- A topic the AI proposed. It stays marked until something is confirmed under it; an empty marked topic is removed (`Topics::delete_vacant_auto`).
ALTER TABLE topics ADD COLUMN auto INTEGER NOT NULL DEFAULT 0;

CREATE TRIGGER topic_auto_cleared_on_insert AFTER INSERT ON topic_links
WHEN NEW.state = 'confirmed'
BEGIN
  UPDATE topics SET auto = 0 WHERE id = NEW.topic_id AND auto = 1;
END;

CREATE TRIGGER topic_auto_cleared_on_update AFTER UPDATE OF state ON topic_links
WHEN NEW.state = 'confirmed'
BEGIN
  UPDATE topics SET auto = 0 WHERE id = NEW.topic_id AND auto = 1;
END;
