-- Slack bot users (users.info is_bot: Jira, Opsgenie...) are never suggested as people.
ALTER TABLE account_profiles ADD COLUMN is_bot INTEGER NOT NULL DEFAULT 0;
