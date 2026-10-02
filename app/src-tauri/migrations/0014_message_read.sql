-- Messages already in an item you'd triaged, when a sync brings it back (a new
-- thread reply under a triaged parent): shown faded, not as new.
ALTER TABLE item_messages ADD COLUMN read INTEGER NOT NULL DEFAULT 0;
