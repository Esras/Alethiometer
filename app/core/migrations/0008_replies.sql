-- A thread reply stored under its parent message: the parent's ext_key (Slack ts).
ALTER TABLE item_messages ADD COLUMN reply_to TEXT;
CREATE INDEX item_messages_key ON item_messages(ext_key);
