-- An item's revision, bumped by the database whenever it gains a message or
-- becomes unseen. Incoming lists it; triage sends back the revision it saw,
-- so it applies to what you saw (app/docs/db-plan.md, Phase 3).
ALTER TABLE items ADD COLUMN rev INTEGER NOT NULL DEFAULT 0;

CREATE TRIGGER items_rev_message AFTER INSERT ON item_messages
BEGIN
  UPDATE items SET rev = rev + 1 WHERE id = NEW.item_id;
END;

CREATE TRIGGER items_rev_message_moved AFTER UPDATE OF item_id ON item_messages
WHEN NEW.item_id != OLD.item_id
BEGIN
  UPDATE items SET rev = rev + 1 WHERE id = NEW.item_id;
END;

CREATE TRIGGER items_rev_unseen AFTER UPDATE OF seen ON items
WHEN OLD.seen = 1 AND NEW.seen = 0
BEGIN
  UPDATE items SET rev = rev + 1 WHERE id = NEW.id;
END;
