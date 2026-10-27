-- A signal ends no earlier than its latest stored reply; msg_count is left alone.
UPDATE items SET ended_at = (SELECT max(m.at) FROM item_messages m WHERE m.item_id = items.id AND m.reply_to IS NOT NULL)
 WHERE (SELECT max(m.at) FROM item_messages m WHERE m.item_id = items.id AND m.reply_to IS NOT NULL) > coalesce(ended_at, occurred_at);

CREATE TRIGGER item_messages_reply_ends AFTER INSERT ON item_messages
WHEN NEW.reply_to IS NOT NULL
BEGIN
  UPDATE items SET ended_at = NEW.at WHERE id = NEW.item_id AND coalesce(ended_at, occurred_at) < NEW.at;
END;

CREATE TRIGGER item_messages_reply_ends_changed AFTER UPDATE OF at, item_id, reply_to ON item_messages
WHEN NEW.reply_to IS NOT NULL
BEGIN
  UPDATE items SET ended_at = NEW.at WHERE id = NEW.item_id AND coalesce(ended_at, occurred_at) < NEW.at;
END;

-- A sync rewrites the range from the source's run, which has no replies.
CREATE TRIGGER items_end_after_replies AFTER UPDATE OF occurred_at, ended_at ON items
BEGIN
  UPDATE items SET ended_at = (SELECT max(m.at) FROM item_messages m WHERE m.item_id = NEW.id AND m.reply_to IS NOT NULL)
   WHERE id = NEW.id
     AND (SELECT max(m.at) FROM item_messages m WHERE m.item_id = NEW.id AND m.reply_to IS NOT NULL) > coalesce(NEW.ended_at, NEW.occurred_at);
END;
