-- A due date is either an instant (`due_at`, UTC) or a calendar day
-- (`due_day`, YYYY-MM-DD, due by the end of that day wherever you are).
-- Day-only dues used to be stored as local 23:59:59 on that day.
ALTER TABLE items ADD COLUMN due_day TEXT;
UPDATE items SET due_day = substr(due_at, 1, 10), due_at = NULL WHERE due_at LIKE '____-__-__T23:59:59';
